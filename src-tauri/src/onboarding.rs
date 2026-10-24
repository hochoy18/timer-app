//! 引导：一次性面板 + 通知权限申请。
//!
//! 首次运行时弹一次引导面板，在里面申请通知权限；做过之后写一个标记文件，
//! 以后的启动就不再打扰（CONTEXT.md 的"引导"）。
//!
//! **磁盘上只留这一个文件。** 它记的是"引导与权限申请已经做过"，**不记任何
//! 计时进度**——那是 [ADR-0003](../../docs/adr/0003-no-state-persistence.md) 划
//! 下的唯一的例外。
//!
//! 分两层，跟托盘那边一个路子：
//!
//! - **纯逻辑**——标记文件在哪、要不要引导、做完了怎么记。
//! - **Tauri 接线**——给前端回话、在第一次运行时把面板弹出来。
//!
//! ## 为什么引导要存在
//!
//! macOS 的通知权限只弹一次：用户当时没留意、随手点了"不允许"，之后所有
//! 通知都静默失败，而应用这边**看不见**这个失败（插件恒返回 `Ok`，见
//! [`crate::notify`]）。如果等到一小时后第一次提醒才去申请，而人恰好不在
//! 座位上，这个 app 就永远静默了。所以要在**应用启动时**、用户眼前就把这件事
//! 办掉。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};

use crate::notify::Notifier;

/// 引导标记的文件名。
///
/// 它在应用数据目录里（macOS 上是 `~/Library/Application Support/<identifier>`），
/// 以点开头：用户翻到那个目录时，一眼能看出这是应用自己的内部文件。
pub const MARKER_FILE_NAME: &str = ".onboarded";

/// 探测通知的标题。
const PROBE_TITLE: &str = "专注计时已就绪";

/// 探测通知的正文。
const PROBE_BODY: &str = "工作满 1 小时会提醒你休息；不确认则每 10 分钟重复提醒一次。";

// ── 纯逻辑 ────────────────────────────────────────────────────────────

/// 引导的状态。前端据此决定画引导面板还是画计时面板。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingStatus {
    /// 引导还没做完吗？
    pub pending: bool,
}

/// 引导的状态：一个标记文件，外加一个"还没做完吗"的答案。
///
/// 状态只有两个走向：没做过 → 做过。做过之后不会再变回去，所以答案缓存一份，
/// 免得每次问都要摸一次磁盘。
pub struct Onboarding {
    /// 标记文件的位置。
    ///
    /// `None` 表示"没有地方记"（取不到应用数据目录时的兜底）：这种情况下引导
    /// 照做，只是做完了记不下来——下次启动还会再问一次。宁可多问一次，也不要
    /// 因为记不下来就干脆不问：那样权限永远申请不上，提醒会永远静默（见本模块
    /// 开头的"为什么引导要存在"）。
    marker: Option<PathBuf>,
    /// 引导还没做完吗？
    pending: AtomicBool,
}

impl Onboarding {
    /// 按标记文件的位置建一个引导状态。
    ///
    /// 标记文件在，就说明做过了一——这次启动不用再引导。
    pub fn new(marker: Option<PathBuf>) -> Self {
        let pending = marker.as_ref().is_none_or(|m| !is_onboarded(m));
        Self {
            marker,
            pending: AtomicBool::new(pending),
        }
    }

    /// 引导还没做完吗？
    pub fn pending(&self) -> bool {
        self.pending.load(Ordering::SeqCst)
    }

    /// 引导做完了：写下标记文件。
    ///
    /// 返回引导有没有真的结束。
    ///
    /// 写不下去时引导仍然是"没做完"——下次启动还会再问一次。这比悄悄记成
    /// "做过了"好：宁可多问一次，也不要在用户还没答应的时候就把申请权限的
    /// 机会丢掉。
    pub fn complete(&self) -> bool {
        let Some(marker) = &self.marker else {
            // 没地方记：做完了，但只是这次运行里记住了。
            self.pending.store(false, Ordering::SeqCst);
            return true;
        };
        match write_marker(marker) {
            Ok(()) => {
                self.pending.store(false, Ordering::SeqCst);
                true
            }
            Err(e) => {
                eprintln!(
                    "focus-timer: 引导标记写不下去（{e}）。下次启动还会再引导一次。"
                );
                false
            }
        }
    }
}

/// 标记文件该放在哪：`<应用数据目录>/.onboarded`。
///
/// macOS 上应用数据目录是 `~/Library/Application Support/<identifier>`。
pub fn marker_path(data_dir: &Path) -> PathBuf {
    data_dir.join(MARKER_FILE_NAME)
}

/// 引导做过了吗：标记文件在就是做过。
pub fn is_onboarded(marker: &Path) -> bool {
    marker.exists()
}

/// 写下引导标记。
///
/// 文件是**空的**：它的存在本身就是全部内容，一个字节都不多写。这是
/// ADR-0003 划的那条线——磁盘上只允许有这一个文件，而且只记"引导做过"这件
/// 事，不记任何计时进度。
pub fn write_marker(marker: &Path) -> std::io::Result<()> {
    if let Some(dir) = marker.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::File::create(marker)?;
    Ok(())
}

// ── Tauri 接线 ────────────────────────────────────────────────────────

/// 按应用数据目录算出标记文件的位置，并据此建一个引导状态。
pub fn create<R: Runtime>(app: &AppHandle<R>) -> Onboarding {
    let marker = app
        .path()
        .app_data_dir()
        .ok()
        .map(|dir| marker_path(&dir));
    Onboarding::new(marker)
}

/// 前端问：引导还没做完吗？
#[tauri::command]
pub fn onboarding_status(onboarding: tauri::State<'_, Onboarding>) -> OnboardingStatus {
    OnboardingStatus {
        pending: onboarding.pending(),
    }
}

/// 前端说：引导做完了（权限申请也做了）。
///
/// 记下标记，顺手递一条探测通知（见 [`probe`]）。
///
/// 返回标记有没有真的写下去。写不下去时引导下次启动还会再来一次——但**这不
/// 影响这次**：权限的申请是 macOS 的一次性动作，错过了就没有第二次了，所以
/// 探测通知照递。
#[tauri::command]
pub fn onboarding_complete<R: Runtime>(
    app: AppHandle<R>,
    onboarding: tauri::State<'_, Onboarding>,
) -> bool {
    let done = onboarding.complete();
    probe(&app);
    done
}

/// 递一条探测通知。
///
/// **这条通知才是 macOS 上真正起作用的那一步。** 票 07 要求"点 `启用通知` 才
/// 触发系统权限弹窗"，而插件在桌面上**不会**弹任何系统框：它把
/// `window.Notification` 换成了自己的壳（`init-iife.js`），
/// `requestPermission()` 走 `invoke("plugin:notification|request_permission")`，
/// 而那条命令在桌面上恒回 `Granted`（插件的 `desktop::request_permission` 就是
/// `Ok(PermissionState::Granted)`）——不弹框，连"拒绝"都轮不到用户点。
///
/// macOS 那句「XX 想要发送通知」是**第一次投递通知时**才冒出来的：插件底层走
/// `notify-rust` → `NSUserNotificationCenter deliverNotification:`，未授权的
/// app 一投递就被系统拦下来问一句。所以这里补投一条——没授权 → 系统弹窗；
/// 已授权 → 当成"提醒已开启"的确认，正好让用户看见提醒长什么样。
///
/// 递不出去也没关系：通知只是提醒的三条路之一（另两条是面板和托盘）。
fn probe<R: Runtime>(app: &AppHandle<R>) {
    // 插件没装上递不出去也不要紧：那条守卫在 `notify::TauriSink::show` 里，
    // 递不出去会降级成一句警告，不会 panic（见 [`crate::notify`]）。
    Notifier::new(app.clone()).notify(PROBE_TITLE, PROBE_BODY);
}

/// 测试用的公共脚手架：临时目录。
///
/// 住在外面是因为两个测试模块都要它：这个模块自己的用例，和 `lib.rs` 里走
/// IPC 通道点引导按钮的用例。
#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// 临时目录的编号：每次调用给一个独一无二的名字。
    ///
    /// 测试是并着跑的，同一个名字会让两个用例互相删对方的目录。
    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// 一个临时的应用数据目录。
    ///
    /// 没用 `tempfile`：那是个只在 Windows 上被间接带进来的依赖，macOS 上
    /// 拉不到。这里手搓一个：每次一个独一无二的名字，跑之前先清一遍。
    pub fn temp_data_dir(name: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "focus-timer-onboarding-{}-{name}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建不出临时目录");
        dir
    }

    /// 一个临时目录里的标记文件路径。
    pub fn temp_marker(name: &str) -> PathBuf {
        crate::onboarding::marker_path(&temp_data_dir(name))
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{temp_data_dir, temp_marker};
    use super::*;

    // ── 标记在哪 ────────────────────────────────────────────────────────

    #[test]
    fn the_marker_lives_in_the_app_support_directory() {
        // macOS 上就是 `~/Library/Application Support/<identifier>/.onboarded`。
        let marker = marker_path(Path::new(
            "/Users/me/Library/Application Support/com.focustimer.app",
        ));
        assert_eq!(
            marker,
            PathBuf::from("/Users/me/Library/Application Support/com.focustimer.app/.onboarded")
        );
    }

    // ── 要不要引导 ──────────────────────────────────────────────────────

    #[test]
    fn a_fresh_install_still_needs_the_onboarding() {
        let marker = temp_marker("fresh");
        assert!(!is_onboarded(&marker));
        assert!(Onboarding::new(Some(marker)).pending());
    }

    #[test]
    fn a_second_launch_does_not_need_the_onboarding_again() {
        let marker = temp_marker("second");
        Onboarding::new(Some(marker.clone())).complete();
        // 标记在了：这次启动直接进工作时段。
        assert!(is_onboarded(&marker));
        assert!(!Onboarding::new(Some(marker)).pending());
    }

    #[test]
    fn completing_the_onboarding_ends_it() {
        let marker = temp_marker("complete");
        let onboarding = Onboarding::new(Some(marker.clone()));
        assert!(onboarding.pending());

        assert!(onboarding.complete());

        assert!(!onboarding.pending(), "做完之后不该再问");
        assert!(marker.exists(), "标记该落盘");
    }

    // ── 标记写不下去 ────────────────────────────────────────────────────

    #[test]
    fn a_marker_that_cannot_be_written_leaves_the_onboarding_pending() {
        // 把标记的"目录"占成一个普通文件：建目录必然失败。
        let dir = temp_data_dir("unwritable");
        let blocker = dir.join("blocked");
        std::fs::write(&blocker, b"not a directory").unwrap();
        let marker = blocker.join(".onboarded");

        let onboarding = Onboarding::new(Some(marker));
        assert!(!onboarding.complete(), "写不下去就该如实说");
        assert!(
            onboarding.pending(),
            "标记没写下去，引导就还算没做完——下次启动还得再问"
        );
    }

    #[test]
    fn an_onboarding_without_a_marker_still_happens_once() {
        // 兜底：没有地方记的时候，引导照样做——不然权限永远申请不上，提醒会
        // 永远静默。做完了只是这次运行里记住了。
        let onboarding = Onboarding::new(None);
        assert!(onboarding.pending());

        assert!(onboarding.complete());

        assert!(!onboarding.pending());
        // 没地方记：这次运行里记住了，下次启动还会再问一次。
        assert!(Onboarding::new(None).pending());
    }

    // ── 标记里只许有"它自己存在"这一件事 ────────────────────────────────

    #[test]
    fn the_marker_records_nothing_but_its_own_existence() {
        // ADR-0003：磁盘上只允许有引导标记这一个文件，而且它只记"引导做过"。
        // 这里锁住"它是空的"——哪天有人往里塞计时进度，这条就会红。
        let marker = temp_marker("empty");
        Onboarding::new(Some(marker.clone())).complete();

        let content = std::fs::read(&marker).expect("读不出标记");
        assert!(
            content.is_empty(),
            "标记文件里不该有别的内容（ADR-0003）：{content:?}"
        );
    }

    #[test]
    fn writing_the_marker_creates_the_directory_if_it_is_missing() {
        let dir = temp_data_dir("mkdir");
        let nested = dir.join("a").join("b").join(".onboarded");
        assert!(!nested.parent().unwrap().exists());

        write_marker(&nested).expect("写不下标记");

        assert!(nested.exists());
    }
}
