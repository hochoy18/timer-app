//! 面板：那个 320pt 的小窗口。
//!
//! 面板是应用里**唯一**的窗口，也是用户看得见的全部界面。
//!
//! **关掉面板不等于退出应用。** 计时在 Rust 侧的时钟线程里继续走，托盘还在
//! （见 CONTEXT.md 的"面板"）。应用只有从托盘菜单的"退出"才真的退出。
//!
//! 分两层，跟托盘那边一个路子：
//!
//! - **纯逻辑**——关闭请求该怎么处理、显示 / 藏起之后可见性是什么。
//! - **Tauri 接线**——真正去动窗口。

use tauri::{AppHandle, Emitter, Manager, Runtime, WindowEvent};

use crate::timer::clock::ClockThread;
use crate::{on_main, STATE_EVENT};

/// 面板窗口的 label。
pub const PANEL_LABEL: &str = "main";

// ── 纯逻辑 ────────────────────────────────────────────────────────────

/// 面板要拿主意的窗口事件。
///
/// 面板只关心"窗口被要求关闭"这一种。其余事件（尺寸、焦点、销毁……）都与它
/// 无关：面板没有可拖的边框，也没有要跟着变的内容。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelEvent {
    /// 窗口被要求关闭（系统关闭按钮、⌘W、"关闭窗口"菜单）。
    CloseRequested,
    /// 与面板无关的窗口事件。
    Other,
}

/// 面板对某个事件的反应。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelReaction {
    /// 拦下这次关闭，把面板藏起来。计时继续。
    Hide,
    /// 不理会。
    Ignore,
}

/// 面板对一个窗口事件该怎么反应。
///
/// 关闭请求一律"藏起来"，没有例外。关键在于**必须拦下来**：应用只有面板这
/// 一个窗口，窗口真被关掉的话 Tauri 会因为"一个窗口都不剩"而退出应用——
/// 计时就断了。应用只有从托盘菜单的"退出"才真的退出。
pub fn reaction_for(event: PanelEvent) -> PanelReaction {
    match event {
        PanelEvent::CloseRequested => PanelReaction::Hide,
        PanelEvent::Other => PanelReaction::Ignore,
    }
}

/// 显示 / 藏起切换之后，面板该是什么可见性。
pub fn visible_after_toggle(visible: bool) -> bool {
    !visible
}

// ── Tauri 接线 ────────────────────────────────────────────────────────

/// 显示面板。**只 `show()`，不 `setFocus()`。**
///
/// 不抢焦点是明确的产品决定：提醒要显眼，但不该打断手头正在做的事。
pub fn show<R: Runtime>(app: &AppHandle<R>) {
    on_main(app, move |app| {
        show_now(&app);
    });
}

/// 藏起面板。**不碰计时器**——计时在时钟线程里继续走。
pub fn hide<R: Runtime>(app: &AppHandle<R>) {
    on_main(app, move |app| {
        if let Some(window) = app.get_webview_window(PANEL_LABEL) {
            let _ = window.hide();
        }
    });
}

/// 显示 / 藏起切换。
pub fn toggle<R: Runtime>(app: &AppHandle<R>) {
    on_main(app, move |app| {
        let Some(window) = app.get_webview_window(PANEL_LABEL) else {
            return;
        };
        // 读可见性本身也要过一趟主线程，所以判断和动作得在同一个闭包里。
        let visible = window.is_visible().unwrap_or(false);
        if visible_after_toggle(visible) {
            show_now(&app);
        } else {
            let _ = window.hide();
        }
    });
}

/// 处理一个窗口事件。
///
/// 面板只对"窗口被要求关闭"有反应：拦下来，藏起来。其余事件不理。
pub fn on_window_event<R: Runtime>(app: &AppHandle<R>, event: &WindowEvent) {
    let panel_event = match event {
        WindowEvent::CloseRequested { .. } => PanelEvent::CloseRequested,
        _ => PanelEvent::Other,
    };
    match reaction_for(panel_event) {
        PanelReaction::Hide => {
            // 必须 `prevent_close`：不拦的话窗口会被销毁，而面板是应用里唯一的
            // 窗口，Tauri 会因为"一个窗口都不剩"而退出应用——计时就断了。
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
            }
            hide(app);
        }
        PanelReaction::Ignore => {}
    }
}

/// 真正把窗口显示出来：先补推一次快照，再 `show()`。
///
/// 补推快照是有必要的：面板可能已经藏了很久，webview 还在跑，但它手里那份
/// 快照可能已经过期——应用刚启动时甚至压根没收到过第一帧（时钟的第一帧比
/// 前端订阅早）。显示之前推一次，面板弹出来就是对的。
///
/// 先推后显示：顺序反过来的话，面板会先亮一帧"正在启动…"。
fn show_now<R: Runtime>(app: &AppHandle<R>) {
    if app.get_webview_window(PANEL_LABEL).is_none() {
        return;
    }
    push_current_snapshot(app);
    if let Some(window) = app.get_webview_window(PANEL_LABEL) {
        let _ = window.show();
    }
}

/// 把当前快照推给前端。
///
/// 从主线程调用：这里要拿状态机的锁。时钟线程持锁时主线程会等一下，但时钟
/// 线程从不反过来等主线程（`on_main` 只投递、不等），所以不会互锁。
fn push_current_snapshot<R: Runtime>(app: &AppHandle<R>) {
    let Some(clock) = app.try_state::<ClockThread>() else {
        return;
    };
    let snapshot = clock.snapshot();
    let _ = app.emit(STATE_EVENT, snapshot);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_close_request_is_swallowed_by_hiding_the_panel() {
        assert_eq!(
            reaction_for(PanelEvent::CloseRequested),
            PanelReaction::Hide
        );
    }

    #[test]
    fn other_window_events_are_left_alone() {
        assert_eq!(reaction_for(PanelEvent::Other), PanelReaction::Ignore);
    }

    #[test]
    fn toggling_flips_the_visibility() {
        assert!(visible_after_toggle(false));
        assert!(!visible_after_toggle(true));
    }
}
