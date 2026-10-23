pub mod config;
pub mod timer;

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, RunEvent};

use crate::config::TimerConfig;
use crate::timer::clock::{event_for_action, ClockThread, Dispatcher};
use crate::timer::state::TimerSnapshot;

/// 把时钟的状态机副作用落到 Tauri 上。
///
/// 时钟线程只管"要做什么"，这里管"怎么做"。托盘与提醒还没做完
/// （票 04 / 05 / 06），先把出口留好。
struct TauriDispatcher {
    app: AppHandle,
}

impl Dispatcher for TauriDispatcher {
    fn notify(&self, title: &str, body: &str) {
        // 票 05：接系统通知（tauri-plugin-notification）与系统音效。
        let _ = (title, body);
    }

    fn play_sound(&self) {
        // 票 05：调 macOS 系统音效。
    }

    fn show_panel(&self) {
        // 显示但不抢焦点——提醒要显眼，不该打断手头的事。
        if let Some(window) = self.app.get_webview_window("main") {
            let _ = window.show();
        }
    }

    fn set_tray_alert(&self, alerting: bool) {
        // 票 04：换醒目图标、标题转红。
        let _ = alerting;
    }

    fn set_tray_title(&self, title: &str) {
        // 票 04：设置托盘标题。
        let _ = title;
    }

    fn emit_state(&self, snapshot: &TimerSnapshot) {
        // 前端是纯视图：只在这一帧有变化时才推。
        let _ = self.app.emit("timer://state", snapshot);
    }
}

/// 前端按钮：把动作交给状态机。
///
/// 状态机住在时钟线程里，所以这里只是把动作递过去；时钟线程立刻应用它，
/// 不等下一个心跳。
#[tauri::command]
fn timer_action(clock: tauri::State<'_, ClockThread>, action: String) {
    if let Some(event) = event_for_action(&action) {
        clock.apply(event);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let dispatcher = Arc::new(TauriDispatcher {
                app: app.handle().clone(),
            });
            let clock = ClockThread::start(dispatcher, TimerConfig::from_env_or_default());
            app.manage(clock);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![timer_action])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app, event| {
        // 退出前显式停机。
        //
        // 不能指望 `Drop`：时钟线程在 Tauri 的状态表里，它的分发器又持有
        // `AppHandle`，而状态表本身就在 `AppHandle` 里面——引用成环，
        // `ClockThread` 永远等不到被 drop。详见 `ClockThread::stop`。
        if matches!(event, RunEvent::Exit) {
            if let Some(clock) = app.try_state::<ClockThread>() {
                clock.stop();
            }
        }
    });
}
