//! 单实例：同一时刻只有一个计时进程在跑。
//!
//! 重复双击 `.app` 时，第二个进程不该自己再跑一套计时——那会多出一个托盘图标、
//! 两套提醒，两边到点了各响一次，用户还以为自己有两个计时器在跑。
//!
//! 这件事分两半：
//!
//! - **名分**（谁是那个唯一的实例）由我们自己发：[`claim`] 拿一把内核级的排他锁。
//! - **认亲**（后来的那个怎么告诉先到的那个"我来了"）交给
//!   `tauri-plugin-single-instance`：它把第二个进程的参数交给第一个，第一个的
//!   回调再把面板显示出来。
//!
//! 为什么名分不交给插件：插件在 macOS 上的认亲是"**先连后绑**"——连不上 socket
//! 才去绑它。这两步之间有个缝，两个进程能同时挤进去：绑赢的那个成了单例，绑定
//! 失败的那个插件会打一条错误日志，然后**照样把这个应用跑起来**。菜单栏上于是
//! 有了两个托盘图标、两套提醒——正是这张票要防的那件事。实测那个缝有二十来
//! 毫秒宽（两个进程间隔 20ms 以内起，100% 撞上；50ms 以外就撞不上了）。
//!
//! 排他锁没有这个缝：内核保证只有一个进程能拿到它。拿到的才是单例，没拿到的
//! 立刻退出。
//!
//! ## 第二个进程是什么时候退出的
//!
//! 名分是在 `run()` 里、**Tauri 起来之前**判的。所以后来的那个进程退出的时候，
//! 托盘图标、时钟线程、webview 都还没有——菜单栏上不会多出第二个图标，计时也
//! 不会白跑一小段。插件的 `setup` 钩子（`build()` 末尾）比我们的闸门晚，它
//! 认亲失败时"照样把应用跑起来"的那条路，后来的进程根本走不到。
//!
//! ## macOS 上靠一个 socket 认亲
//!
//! 插件在 macOS 上用 `/tmp/<identifier 把 . 和 - 都换成 _>_si.sock` 这个 unix
//! socket 认亲：连得上，说明已经有实例在了，自己就该退出；连不上，自己当那个
//! 唯一的实例，绑上 socket 听着。
//!
//! 这个 socket **不是状态**（[ADR-0003](../../docs/adr/0003-no-state-persistence.md)
//! 划的线），但它确实是个落在磁盘上的文件——那份决策里有专门一节记着它。
//!
//! ## 开发版和打包版共用一个 socket
//!
//! 两者的 identifier 相同，socket 也就相同：打包版在跑的时候，`tauri dev` 起的
//! 实例会立刻退出。这是好事——同一台机器上本来就只该有一个计时器在跑。

use std::fs::{File, OpenOptions, TryLockError};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, Runtime};

use crate::panel;

/// 面板窗口还没建好时，最多等多久。
const PANEL_WAIT: Duration = Duration::from_secs(1);

/// 等面板窗口时，多久查一次。
const PANEL_POLL: Duration = Duration::from_millis(20);

/// 通知已经在跑的那个实例的上限。
///
/// 对方可能还在启动（socket 还没绑上），所以重试一小会儿。正常情况下——对方早
/// 就跑起来了——第一下就连上了，这个上限用不着。
const NOTIFY_TIMEOUT: Duration = Duration::from_secs(1);

/// 通知时多久试一次。
const NOTIFY_POLL: Duration = Duration::from_millis(10);

/// 拿到的锁，存在这儿直到进程结束。
///
/// 锁跟着 fd 走：`File` 一 drop 就关 fd，锁也就跟着松了。所以这个 `File` 得一直
/// 活着，进程才一直占着名分。
static HELD_LOCK: OnceLock<File> = OnceLock::new();

// ── 纯逻辑 ────────────────────────────────────────────────────────────

/// 单例认亲用的 socket 路径。
///
/// 插件的规矩（macOS 上）：`/tmp/<identifier 把 . 和 - 都换成 _>_si.sock`。
/// 我们照抄一份，是为了能认出"这个 socket 是不是我们的"——后面的进程靠它通知
/// 先到的那个（见 [`notify`]）。
///
/// 插件哪天改了规矩，这里要跟着改：不然后来的进程通知不到先到的那个，面板就
/// 不会弹出来。
pub fn socket_path(identifier: &str) -> PathBuf {
    let name = identifier.replace(['.', '-'], "_");
    PathBuf::from(format!("/tmp/{name}_si.sock"))
}

/// 名分锁的文件路径。
///
/// 跟 socket 做邻居：同一个前缀、同一个目录，都是"单实例机制在 `/tmp` 里的落地
/// 物"。它是零字节的，什么也不记（[ADR-0003](../../docs/adr/0003-no-state-persistence.md)）。
///
/// 放在 `/tmp` 而不是应用的数据目录：数据目录里只该有引导标记（票 07），别把
/// 别的机制的东西也塞进去。
pub fn lock_path(identifier: &str) -> PathBuf {
    let name = identifier.replace(['.', '-'], "_");
    PathBuf::from(format!("/tmp/{name}_si.lock"))
}

// ── 接线 ──────────────────────────────────────────────────────────────

/// 抢名分：把"我是不是那个唯一的实例"这件事定下来。
///
/// 抢到了返回 `true`——我是唯一的那个，该继续启动。没抢到返回 `false`——已经有
/// 别人在了，我该退出。
///
/// 抢锁出错了（文件打不开、锁本身报错）一律**放行**：宁可有两个实例，也不能让
/// 用户点不开应用。只有"确实有别人占着锁"这一种情况才拦。
pub fn claim(identifier: &str) -> bool {
    let file = match OpenOptions::new()
        .create(true)
        .write(true)
        .open(lock_path(identifier))
    {
        Ok(file) => file,
        Err(_) => return true,
    };
    match file.try_lock() {
        Ok(()) => {
            // 锁要活到进程结束，所以这个 `File` 不能被 drop——存进 static 盯着。
            // 万一 `set` 失败（不可能发生：只抢一次），手里这个 `File` 会被丢掉、
            // 锁跟着松掉，但 static 里那个还锁着，名分没丢。
            let _ = HELD_LOCK.set(file);
            true
        }
        // 别人占着：我不是那个唯一的实例。
        Err(TryLockError::WouldBlock) => false,
        // 锁本身出错了：放行。
        Err(TryLockError::Error(_)) => true,
    }
}

/// 告诉已经在跑的那个实例：把面板显示出来。
///
/// 走插件的 socket：连上去就走，什么都不写——插件的监听器读到空消息也会照样调
/// 回调（它把读到的空串当成"没有参数"）。
///
/// 对方可能还在启动，socket 还没绑上，所以重试一小会儿。
pub fn notify(identifier: &str) {
    let socket = socket_path(identifier);
    let deadline = Instant::now() + NOTIFY_TIMEOUT;
    loop {
        if UnixStream::connect(&socket).is_ok() {
            return;
        }
        if Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(NOTIFY_POLL);
    }
}

/// 第二个实例来了：把面板显示出来。
///
/// 插件的回调是从它自己的监听任务里发过来的，不在主线程上。面板的显示自己会
/// 走 [`crate::on_main`] 投递到主线程（见 [`crate::panel::show`]），所以这里
/// 直接调就行，不用自己再投一次。
///
/// ## 为什么要先等面板建好
///
/// 面板窗口是 Tauri 在事件循环起来（`Ready`）之后才建的，而 socket 在
/// `build()` 末尾就绑上了——中间差着起 webview 的那几百毫秒。这时候来的第二个
/// 实例，"显示面板"会落空：面板还不存在。
///
/// 这不是假想：`build()` 末尾绑好 socket → 第二个实例连上来 → 回调触发 → 面板
/// 还没建好 → 什么都没发生。用户看着自己点的 `.app` 跟没点一样。
///
/// 所以这里先等它一会儿再显示。这个等待跑在插件的监听线程上（不是主线程），
/// 主线程该干嘛干嘛。
pub fn on_second_instance<R: Runtime>(app: &AppHandle<R>) {
    // 说一声。面板突然弹出来的时候，用户（和开发者）该知道是为什么。
    eprintln!("[单实例] 第二个实例来了，把面板显示出来。");

    let deadline = Instant::now() + PANEL_WAIT;
    while app.get_webview_window(panel::PANEL_LABEL).is_none() {
        if Instant::now() >= deadline {
            // 等不出来就算了：面板还没建好，显示也无处可显。
            return;
        }
        std::thread::sleep(PANEL_POLL);
    }
    panel::show(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_socket_path_comes_from_the_identifier() {
        // 插件的规矩：`/tmp/<identifier 把 . 和 - 都换成 _>_si.sock`。
        assert_eq!(
            socket_path("com.focustimer.app"),
            PathBuf::from("/tmp/com_focustimer_app_si.sock")
        );
    }

    #[test]
    fn dots_and_dashes_become_underscores() {
        // unix socket 的路径有 100 字符的上限，插件把分隔符都换成了下划线。
        assert_eq!(
            socket_path("com.my-app.v2"),
            PathBuf::from("/tmp/com_my_app_v2_si.sock")
        );
    }

    #[test]
    fn the_lock_file_sits_next_to_the_socket() {
        // 锁跟 socket 做邻居：同一个前缀、同一个目录。
        assert_eq!(
            lock_path("com.focustimer.app"),
            PathBuf::from("/tmp/com_focustimer_app_si.lock")
        );
    }

    #[test]
    fn only_one_claim_wins_the_lock() {
        // 这是"名分"这道闸门的全部意义：两个进程同时抢，只有一个能拿到。
        // 拿不到的那个必须知道自己不是唯一的那个。
        let path = std::env::temp_dir().join(format!(
            "focus-timer-lock-probe-{}.lock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        let first = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&path)
            .expect("建不出锁文件");
        assert!(first.try_lock().is_ok(), "第一个该拿到锁");

        let second = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&path)
            .expect("建不出锁文件");
        assert!(
            matches!(second.try_lock(), Err(TryLockError::WouldBlock)),
            "第二个该抢不到锁"
        );

        let _ = std::fs::remove_file(&path);
    }
}
