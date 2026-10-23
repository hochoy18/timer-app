//! 提醒出口：系统通知 + macOS 系统音效。
//!
//! 两条路，都不走 webview：
//!
//! - **通知**走 `tauri-plugin-notification` 的 Rust API（`app.notification().builder()`）。
//! - **音效**走 macOS 的 `AudioServicesPlaySystemSound`。不用 webview 的
//!   `AudioContext`：这个 app 启动即自己计时，用户可能一次都没点过屏幕，而
//!   `AudioContext` 在没有用户交互时是 suspended 状态——首次提醒会一声不吭。
//!
//! 跟 [`crate::tray`] 一个路子：上半截是纯逻辑（递送策略、失败降级），下半截
//! 才是 Tauri 接线。这么切是因为"通知递不出去怎么办"是最要紧的降级场景——
//! 它得能被单元测试驱动，而不是只能在真机上撞。
//!
//! 但有一件事得先说清楚：**macOS 上我们其实看不见失败**。插件的 `show()` 把真正
//! 的投递扔进后台任务、结果自己吞掉（源码里是 `let _ = notification.show();`），
//! 恒返回 `Ok(())`。所以"权限被拒"这件事 macOS 不会告诉我们，我们也不去猜——
//! 这正好是验收要的那条路：不 panic、不刷屏、安静降级，面板和托盘照常提醒。
//! [`deliver`] 里的闸门留给"插件自己报错"的情形（哪天插件改了行为，这里接得住）。

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Runtime};
use tauri_plugin_notification::NotificationExt;

// ── 纯逻辑 ────────────────────────────────────────────────────────────

/// 失败告警闸门：整个进程里最多说一次。
///
/// 通知只是提醒的三条路之一（另两条是面板和托盘），它失败时不该把人淹了：
/// 重复提醒每 10 分钟来一次，每次都打一行错误就是刷屏。折中：第一次说一声
/// （让人知道这条路不通），之后静默。
pub struct WarnOnce {
    warned: AtomicBool,
}

impl WarnOnce {
    /// 新建一个还没报过警的闸门。
    pub fn new() -> Self {
        Self {
            warned: AtomicBool::new(false),
        }
    }

    /// 这次失败该不该报：第一次 `true`，之后 `false`。
    pub fn should_warn(&self) -> bool {
        // `swap` 是原子的：并发地失败时也只有一个线程拿到 `true`。
        !self.warned.swap(true, Ordering::SeqCst)
    }
}

impl Default for WarnOnce {
    fn default() -> Self {
        Self::new()
    }
}

/// 通知的出口：把一条通知递到系统里去。
///
/// 抽成 trait 是为了让"递不出去"能被单元测试驱动——这是最要紧的降级场景，
/// 得有个真机之外也能验的办法。
pub trait NotificationSink {
    /// 递一条通知。递不出去时给出原因。
    fn show(&self, title: &str, body: &str) -> Result<(), String>;
}

/// 一次递送的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    /// 递出去了。
    Sent,
    /// 第一次没递出去：调用方该报个警（原因在此）。
    FirstFailure(String),
    /// 又没递出去：静默。
    LaterFailure,
}

/// 把一条通知递出去，递不出去就降级。
///
/// **不 panic**：通知只是提醒的一条路，递不出去时面板与托盘照常提醒。
pub fn deliver(
    sink: &impl NotificationSink,
    title: &str,
    body: &str,
    gate: &WarnOnce,
) -> Delivery {
    match sink.show(title, body) {
        Ok(()) => Delivery::Sent,
        Err(reason) => {
            if gate.should_warn() {
                Delivery::FirstFailure(reason)
            } else {
                Delivery::LaterFailure
            }
        }
    }
}

/// 系统提示音的 id：`kSystemSoundID_UserPreferredAlert`（AudioToolbox 头文件里的
/// `0x00001000`）。
///
/// 三个时机都用它：提醒就该用提醒音。用户可以在"系统设置 → 声音"里换掉它，
/// 那是他们的选择，不该被本 app 绕过去。
const SYSTEM_SOUND_ID: u32 = 0x0000_1000;

// `AudioServicesPlaySystemSound` —— AudioToolbox 里的一个 C 函数。
//
// 为什么手写 `extern` 而不引 crate：`objc2` 包的是 Objective-C 的类，
// `core_audio` 包的是 AudioObject（音频硬件）那一族 API；AudioServices 是
// AudioToolbox 里的普通 C 符号，就一个函数，为它引一整个 crate 不划算。
// 将来 Apple 真把它弃用了（头文件里写着"未来版本会弃用"），换成
// `AudioServicesPlaySystemSoundWithCompletion` 即可，只是多一个回调。
#[cfg(target_os = "macos")]
#[link(name = "AudioToolbox", kind = "framework")]
extern "C" {
    fn AudioServicesPlaySystemSound(inSystemSoundID: u32);
}

/// 播一次系统提示音。
#[cfg(target_os = "macos")]
fn play_system_sound() {
    // SAFETY: 纯 C 函数，只收一个 u32，不碰内存，任何线程都能调。
    unsafe { AudioServicesPlaySystemSound(SYSTEM_SOUND_ID) };
}

/// 播一次系统提示音（非 macOS：没有这一说，什么也不做）。
#[cfg(not(target_os = "macos"))]
fn play_system_sound() {
    // 本期只做 macOS（spec §2「本期不做：跨平台」）。
}

// ── Tauri 接线 ────────────────────────────────────────────────────────

/// 提醒出口：系统通知 + 系统音效。
///
/// 应用里只有一个，挂在时钟线程的分发器上。
pub struct Notifier<R: Runtime> {
    app: AppHandle<R>,
    notify_warned: WarnOnce,
}

impl<R: Runtime> Notifier<R> {
    /// 拿住 app 句柄。
    pub fn new(app: AppHandle<R>) -> Self {
        Self {
            app,
            notify_warned: WarnOnce::new(),
        }
    }

    /// 发一条系统通知。
    ///
    /// 递不出去时静默降级：不 panic、不刷屏——面板与托盘是另外两条路，照常提醒。
    /// 插件在 macOS 上恒返回 `Ok`，所以这里几乎走不到失败分支；真走到了，
    /// 也只说一次（见 [`deliver`]）。
    pub fn notify(&self, title: &str, body: &str) {
        match deliver(
            &TauriSink { app: &self.app },
            title,
            body,
            &self.notify_warned,
        ) {
            Delivery::Sent => {}
            Delivery::FirstFailure(reason) => eprintln!(
                "focus-timer: 通知没递出去（{reason}）。面板和托盘照常提醒，这条通知先当没有。"
            ),
            Delivery::LaterFailure => {}
        }
    }

    /// 播一次系统音效。
    pub fn play_sound(&self) {
        play_system_sound();
    }
}

/// 真发通知的那一档：走 `tauri-plugin-notification` 的 Rust API。
struct TauriSink<'a, R: Runtime> {
    app: &'a AppHandle<R>,
}

impl<R: Runtime> NotificationSink for TauriSink<'_, R> {
    fn show(&self, title: &str, body: &str) -> Result<(), String> {
        self.app
            .notification()
            .builder()
            .title(title)
            .body(body)
            .show()
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// 记下收到什么的假出口。
    struct FakeSink {
        /// 递送是否失败。
        fail: bool,
        seen: Mutex<Vec<(String, String)>>,
    }

    impl FakeSink {
        /// 造一个出口：`fail` 决定它是递得出去还是递不出去。
        fn new(fail: bool) -> Self {
            Self {
                fail,
                seen: Mutex::new(Vec::new()),
            }
        }

        fn seen(&self) -> Vec<(String, String)> {
            self.seen.lock().unwrap().clone()
        }
    }

    impl NotificationSink for FakeSink {
        fn show(&self, title: &str, body: &str) -> Result<(), String> {
            if self.fail {
                return Err("通知权限被拒".to_string());
            }
            self.seen
                .lock()
                .unwrap()
                .push((title.to_string(), body.to_string()));
            Ok(())
        }
    }

    // ── 递送 ────────────────────────────────────────────────────────────

    #[test]
    fn the_title_and_body_reach_the_sink_unchanged() {
        let sink = FakeSink::new(false);
        let gate = WarnOnce::new();
        assert_eq!(
            deliver(&sink, "工作时间到", "该休息 15 分钟了。", &gate),
            Delivery::Sent
        );
        assert_eq!(
            sink.seen(),
            vec![(
                "工作时间到".to_string(),
                "该休息 15 分钟了。".to_string()
            )]
        );
    }

    #[test]
    fn a_rejected_notification_is_reported_once_and_never_panics() {
        // 权限被拒：通知这条路走不通，但应用还得活着，面板和托盘照常提醒。
        let sink = FakeSink::new(true);
        let gate = WarnOnce::new();
        assert_eq!(
            deliver(&sink, "工作时间到", "该休息了", &gate),
            Delivery::FirstFailure("通知权限被拒".to_string())
        );
        // 又失败了一次：静默，别刷屏。
        assert_eq!(
            deliver(&sink, "工作时间到", "该休息了", &gate),
            Delivery::LaterFailure
        );
    }

    #[test]
    fn only_one_of_many_threads_gets_to_warn() {
        let gate = std::sync::Arc::new(WarnOnce::new());
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let gate = std::sync::Arc::clone(&gate);
                std::thread::spawn(move || gate.should_warn())
            })
            .collect();
        let winners = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .filter(|won| *won)
            .count();
        assert_eq!(winners, 1);
    }

    // ── 系统音效 ────────────────────────────────────────────────────────

    /// 真调一次系统音效：验 FFI 边界不崩、符号链得上。
    ///
    /// 会真出声，所以默认跳过——手跑：`cargo test -- --ignored`。
    #[test]
    #[ignore = "会真出声"]
    fn the_system_sound_call_does_not_crash() {
        play_system_sound();
    }
}
