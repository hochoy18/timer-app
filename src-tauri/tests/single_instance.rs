//! 单实例的进程级验收。
//!
//! 这是唯一一个**必须在真进程里**验的行为：第二个 `.app` 进程该不该立刻退出、
//! 第一个进程该不该还活着。假运行时（`tauri::test`）测不了这个——它压根不起
//! 进程，`src/single_instance.rs` 里那些用例验的是"决定"，不是"后果"。
//!
//! 怎么验的：把应用二进制真的拉起来，等它当上单例（socket 上有人听着），再拉
//! 起第二个，看它是不是立刻退出。连着拉五次，对应"反复双击 `.app`"。
//!
//! ## 跑这个用例会动到桌面
//!
//! - 菜单栏上会闪出一个托盘图标，几秒后自己消失；
//! - 第二个实例连上来时，第一个实例会**把面板显示出来**——那正是被测的行为。
//!
//! ## 已经有实例在跑的时候，这个用例会跳过
//!
//! 打包版开着、或者 `tauri dev` 开着，都算。那种情况下占着单例的是它，这个
//! 用例当不了"第一个"，只能让位。
//!
//! 跳过和失败是两回事：第一个进程**干净地退出（0）**说明它撞上了别人占着的
//! 单例（这是插件的正常路径），用例跳过；它**带着错误码退出**说明应用自己
//! 起不来，那是要失败的。

use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use focus_timer_lib::single_instance;

/// 应用二进制。cargo 在跑集成测试时会把它的路径塞进这个环境变量。
const EXE: &str = env!("CARGO_BIN_EXE_focus-timer");

/// 等一个进程退出的上限。
///
/// 第二个实例是"立刻"退出的，但它得先启动起来（起 Tauri 运行时、连 socket），
/// 所以给的是"很快"而不是"马上"。
const EXIT_TIMEOUT: Duration = Duration::from_secs(15);

/// 等第一个进程当上单例的上限。
const SINGLETON_TIMEOUT: Duration = Duration::from_secs(15);

/// 应用数据目录里配着的 identifier。
///
/// 插件的 socket 路径是按 identifier 算的，所以要知道路径就得先知道 identifier。
/// 它写在 `tauri.conf.json` 里。
///
/// 注意：构建时要是用 `TAURI_CONFIG` 覆盖过配置，运行时真正用的 identifier 就
/// 可能跟这里读到的不一样——那时这个用例会一直等下去，最后超时失败（失败信息
/// 里会写明 socket 路径，能对得上）。
fn identifier() -> String {
    let raw = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tauri.conf.json"))
        .expect("读不出 tauri.conf.json");
    let conf: serde_json::Value = serde_json::from_str(&raw).expect("tauri.conf.json 不是合法 JSON");
    conf["identifier"]
        .as_str()
        .expect("tauri.conf.json 里没有 identifier")
        .to_string()
}

/// socket 上有人听着吗？
///
/// **不能**用 `Path::exists` 当"单例已就位"的信号：上次跑崩了（SIGKILL）残留的
/// socket 文件还在，但它没人听——那个文件一直存在，直到插件自己清理掉它。拿
/// 它当信号，测试会在第一个进程还没当上单例的时候就开始拉第二个，于是第二个
/// 抢下了单例，失败信息还写着"第一个实例没当上单例"，指向错了方向。
///
/// 连一下才知道有没有人真在听：连得上，说明有个活着的单例。
fn someone_is_listening(socket: &Path) -> bool {
    UnixStream::connect(socket).is_ok()
}

/// 一个跑着的应用进程。
///
/// 掉了就把它杀掉：不然它会在后台继续跑，继续占着单例，下一次跑这个用例就
/// 撞上了。
struct Instance {
    child: Child,
    /// 它的 stderr 落在哪儿。应用起不来的时候，这里有话说。
    stderr_path: std::path::PathBuf,
}

impl Instance {
    /// 起一个应用进程。
    fn spawn(tag: &str) -> Self {
        let stderr_path = std::env::temp_dir().join(format!(
            "focus-timer-single-instance-{}-{tag}.log",
            std::process::id()
        ));
        let stderr = std::fs::File::create(&stderr_path).expect("建不出临时日志文件");
        let child = Command::new(EXE)
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr))
            .spawn()
            .expect("起不了应用进程");
        Self { child, stderr_path }
    }

    /// 它还活着吗？
    fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// 它退出了吗？退出了就返回退出状态。
    fn exit_status(&mut self) -> Option<ExitStatus> {
        self.child.try_wait().ok().flatten()
    }

    /// 等它退出，返回退出状态。
    fn wait_for_exit(&mut self, limit: Duration) -> Option<ExitStatus> {
        let deadline = Instant::now() + limit;
        loop {
            if let Ok(Some(status)) = self.child.try_wait() {
                return Some(status);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// 它 stderr 上说了什么。
    fn stderr(&self) -> String {
        std::fs::read_to_string(&self.stderr_path).unwrap_or_default()
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.stderr_path);
    }
}

/// 第一个进程的下场。
enum First {
    /// 它当上了单例：socket 上有人听着，它自己也还活着。
    Singleton,
    /// 它自己退了，而且退得干净（0）：单例是别人占着的（插件的正常路径）。
    Superseded,
    /// 它自己退了，但退得不干净：应用自己起不来。
    Broken(ExitStatus),
    /// 等超时。
    Timeout,
}

/// 等第一个进程当上单例。
fn wait_for_singleton(first: &mut Instance, socket: &Path) -> First {
    let deadline = Instant::now() + SINGLETON_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(status) = first.exit_status() {
            return if status.success() {
                First::Superseded
            } else {
                First::Broken(status)
            };
        }
        if someone_is_listening(socket) {
            // 再确认一遍它还活着：socket 上有人听着，也可能听着的是**别人**
            // （那我们的进程正在退出，插件的正常路径）。稍等一下再确认一次，
            // 省得把别人的单例当成自己的。
            std::thread::sleep(Duration::from_millis(100));
            return match first.exit_status() {
                Some(status) if status.success() => First::Superseded,
                Some(status) => First::Broken(status),
                None => First::Singleton,
            };
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    First::Timeout
}

#[test]
fn a_second_launch_exits_at_once_and_the_first_one_keeps_running() {
    let socket = single_instance::socket_path(&identifier());
    let mut first = Instance::spawn("first");

    match wait_for_singleton(&mut first, &socket) {
        First::Singleton => {}
        First::Superseded => {
            eprintln!("跳过：已经有别的实例占着单例了。{}", first.stderr());
            return;
        }
        First::Broken(status) => {
            panic!("第一个实例没起来（{status}）：\n{}", first.stderr())
        }
        First::Timeout => panic!(
            "等不到单例（socket：{}）：\n{}",
            socket.display(),
            first.stderr()
        ),
    }

    // 再起四个：一个真进程 + 四个后来的 = 反复双击五次。
    for round in 1..5 {
        let mut second = Instance::spawn(&format!("second-{round}"));

        let Some(status) = second.wait_for_exit(EXIT_TIMEOUT) else {
            panic!(
                "第 {round} 个后来的实例没有退出：它该把位置让给第一个。stderr：\n{}",
                second.stderr()
            );
        };
        assert!(
            status.success(),
            "第 {round} 个后来的实例该干净地退出（0），实际是 {status}。stderr：\n{}",
            second.stderr()
        );
    }

    // 第一个还活着：菜单栏上该只有一个托盘图标，也只有一套提醒。
    assert!(
        first.is_alive(),
        "第一个实例不该被后来的顶掉。stderr：\n{}",
        first.stderr()
    );

    // 第一个该**有反应**：后来的那些把消息交给了它，它该把面板显示出来。
    //
    // 这一条验的是"回调跑到了"——面板窗口真的出现在屏幕上，是真机那一层的事
    // （假运行时和进程级用例都看不见窗口服务器）。
    assert!(
        first.stderr().contains("第二个实例"),
        "第一个实例该收到第二个实例的消息并把面板显示出来。stderr：\n{}",
        first.stderr()
    );

    // 收尾：插件在 `RunEvent::Exit` 里会删掉 socket。我们是用 kill 掐掉的，
    // 它来不及删——这里替它删，免得留个残根给下一次。
    drop(first);
    let _ = std::fs::remove_file(&socket);
}

#[test]
fn two_instances_started_at_once_leave_only_one_running() {
    // 两个进程几乎同时起。
    //
    // 插件自己的认亲是"先连后绑"：两个进程能同时挤进那两步之间，绑赢的是单例，
    // 绑输的插件会打一条错误日志、然后**照样把这个应用跑起来**——菜单栏上于是
    // 有了两个托盘图标、两套提醒。名分锁没有这个缝。
    //
    // 这条用例盯的是**不变量**：两个进程只该活一个。谁拿到名分是不确定的，
    // 所以断言的是"活着的进程数"，不是"哪个活着"。
    let mut a = Instance::spawn("race-a");
    let mut b = Instance::spawn("race-b");

    // 等它们表态：拿到名分的那个会一直活着，让位的那个会退出。
    let deadline = Instant::now() + EXIT_TIMEOUT;
    while Instant::now() < deadline {
        if !a.is_alive() || !b.is_alive() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let a_alive = a.is_alive();
    let b_alive = b.is_alive();
    let alive = usize::from(a_alive) + usize::from(b_alive);

    // 两个都退了：说明已经有别的实例占着名分了（插件的正常路径），让位。
    if alive == 0 {
        eprintln!("跳过：已经有别的实例占着单例了。");
        return;
    }

    assert_eq!(
        alive, 1,
        "两个进程只该活一个（a 活着={a_alive}，b 活着={b_alive}）。\
         两个都活着 = 菜单栏上两个托盘图标、两套提醒。stderr：\n{}{}",
        a.stderr(),
        b.stderr()
    );
}
