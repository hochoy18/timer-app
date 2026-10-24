pub mod config;
pub mod notify;
pub mod panel;
pub mod timer;
pub mod tray;

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, RunEvent, Runtime, Wry};

use crate::config::TimerConfig;
use crate::notify::Notifier;
use crate::timer::clock::{event_for_action, ClockThread, Dispatcher};
use crate::timer::state::{TimerSnapshot, TimerState};

/// Rust 推给前端的状态事件名。
///
/// Rust 侧只有这一个名字在用：时钟线程推状态、面板显示前补推快照，都走它。
///
/// 前端 `src/hooks/useTimerState.ts` 里另有一份字面量——Rust 的常量跨不过语言
/// 边界，两边只能靠人手对齐。对不上的时候前端会静默地一直停在"正在启动…"，
/// 极难查，所以改这个字符串时记得连前端一起改。
pub(crate) const STATE_EVENT: &str = "timer://state";

/// 把一段窗口 / 托盘的改动投到主线程去做。
///
/// Tauri 的托盘 / 窗口 setter 内部是"投递到主线程 + 等它跑完"。在时钟线程里
/// 直接调，就等于拿着状态机的锁去等主线程；而主线程正可能停在面板按钮或托盘
/// 菜单的点击里反过来等这把锁——两边互等，整个应用就卡死了。投递是 FIFO 的，
/// 所以标题、状态、菜单的先后照样成立。
///
/// **这条只在 `tracing` 关着时成立。** `tauri-runtime-wry` 的 `emit` /
/// `eval_script` 在 `tracing` feature 打开时会改用 channel + `rx.recv()` 等
/// 回执，也就是重新变回阻塞的；`TauriDispatcher::emit_state` 那一路就会跟着
/// 一起等。当前依赖图里没有 `tracing`（`cargo tree -i tracing` 是空的），所以
/// 成立。**谁给 `tauri-runtime-wry` 打开 `tracing`，谁就得回头看一眼这里。**
///
/// 句柄由这里克隆一份交给闭包，免得每个调用点自己管"搬走之前先克隆"。
pub(crate) fn on_main<R: Runtime>(
    app: &AppHandle<R>,
    task: impl FnOnce(AppHandle<R>) + Send + 'static,
) {
    let posted = app.clone();
    let _ = app.run_on_main_thread(move || {
        task(posted)
    });
}


/// 把时钟的状态机副作用落到 Tauri 上。
///
/// 时钟线程只管"要做什么"，这里管"怎么做"。
///
/// 这一层的每个方法都是 fire-and-forget：只把闭包投给主线程，不等它跑完。
/// Tauri 的托盘 / 窗口 setter 内部是"投递 + 等"，在时钟线程里拿着状态机的锁去
/// 等主线程，而主线程正可能停在面板按钮或托盘菜单的点击里反过来等这把锁——
/// 两边互等，整个应用就卡死了。投递是 FIFO 的，所以标题、状态、菜单的先后照样
/// 成立（见 `tray::on_main`）。
///
/// 快照那一路例外：它走 `emit`，不走这条队列。
struct TauriDispatcher {
    app: AppHandle,
    /// 提醒出口（系统通知 + 系统音效）。
    ///
    /// 它自己也持有一份 `AppHandle`：`Notifier` 是个独立的小组件，得自己拿住
    /// 句柄；而分发器要操作托盘，也需要一份。两个句柄是同一个 `Arc` 的克隆，
    /// 多这一份不花钱。
    notifier: Notifier<Wry>,
}

impl Dispatcher for TauriDispatcher {
    fn notify(&self, title: &str, body: &str) {
        self.notifier.notify(title, body);
    }

    fn play_sound(&self) {
        self.notifier.play_sound();
    }

    fn show_panel(&self) {
        // 显示但不抢焦点——提醒要显眼，不该打断手头的事。
        panel::show(&self.app);
    }

    fn set_tray_alert(&self, alerting: bool) {
        tray::set_alerting(&self.app, alerting);
    }

    fn set_tray_title(&self, title: &str) {
        tray::set_title(&self.app, title);
    }

    fn set_state(&self, state: TimerState) {
        tray::apply_state(&self.app, state);
    }

    fn emit_state(&self, snapshot: &TimerSnapshot) {
        // 前端是纯视图：只在这一帧有变化时才推。
        let _ = self.app.emit(STATE_EVENT, snapshot);
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

/// 面板的 ✕：只把面板藏起来。
///
/// **不是退出应用。** 计时在时钟线程里继续走，托盘还在（CONTEXT.md 的"面板"）。
///
/// 运行时是泛型的：这样这条命令在真机（`Wry`）和测试用的假运行时上都能注册，
/// 用例才能从 IPC 那一头点它。
#[tauri::command]
fn hide_panel<R: Runtime>(app: AppHandle<R>) {
    panel::hide(&app);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            // 常驻菜单栏的应用不该在 Dock 里占位——没有窗口要跟用户切换。
            // 用 `Accessory` 而不是 `Regular`：后者会在 Dock 和 Cmd-Tab 里露脸。
            let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // 托盘要先于时钟建好：时钟的第一帧就要设托盘标题。
            tray::create(app.handle())?;

            let dispatcher = Arc::new(TauriDispatcher {
                app: app.handle().clone(),
                notifier: Notifier::new(app.handle().clone()),
            });
            let clock = ClockThread::start(dispatcher, TimerConfig::from_env_or_default());
            app.manage(clock);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![timer_action, hide_panel])
        // 面板被要求关闭（系统关闭按钮、⌘W）：只藏起来，不退出。
        //
        // 这一层不拦的话，窗口会被销毁——而面板是应用里唯一的窗口，Tauri 会
        // 因为"一个窗口都不剩"而退出应用，计时就断了。
        .on_window_event(|window, event| {
            panel::on_window_event(window.app_handle(), event);
        })
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

#[cfg(test)]
mod tests {
    //! 面板按钮的端到端：点一下 → 状态机换状态 → 前端收到新快照。
    //!
    //! 这一层专门验**接缝**：前端发的是 `invoke("timer_action", { action })`
    //! （`src/__tests__/panel.test.tsx` 验那一半），这里验另一半——命令真被
    //! 注册了、动作字符串真能推动状态机、新的快照真被推给了前端。
    //!
    //! 用的是 Tauri 自带的假运行时（`tauri::test`）：不需要屏幕，也不需要真的
    //! 窗口，点按钮的路径跟线上一模一样。

    use super::*;
    use crate::config::TimerConfig;
    use crate::timer::clock::{ClockThread, Dispatcher};
    use crate::timer::state::TimerState;
    use crate::timer::test_support::{Call, FakeDispatcher};
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{
        get_ipc_response, mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY,
    };
    use tauri::webview::InvokeRequest;

    /// 一个带真命令、假面板、真时钟的 app。
    struct TestApp {
        app: tauri::App<MockRuntime>,
        recorder: Arc<FakeDispatcher>,
    }

    impl TestApp {
        /// 起一个假 app：注册真命令，挂上真时钟线程，再开一个假面板。
        fn start(cfg: TimerConfig) -> Self {
            let recorder = Arc::new(FakeDispatcher::default());
            let dispatcher: Arc<dyn Dispatcher> = recorder.clone();
            // 心跳调快些，好让"工作时段到期"这种时间驱动的转移不用等真的一小时。
            let clock = ClockThread::start_with_interval(dispatcher, cfg, Duration::from_millis(5));
            let app = mock_builder()
                .invoke_handler(tauri::generate_handler![timer_action, hide_panel])
                .build(mock_context(noop_assets()))
                .expect("建不出假 app");
            app.manage(clock);
            // 面板的 webview：IPC 请求得有个 webview 才能发进去。
            tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
                .build()
                .expect("建不出面板");
            Self { app, recorder }
        }

        /// 走真正的 IPC 通道点一个按钮。
        fn click(&self, action: &str) {
            self.invoke("timer_action", serde_json::json!({ "action": action }));
        }

        /// 走真正的 IPC 通道调一个别的命令（比如面板右上角的 ✕）。
        fn invoke(&self, cmd: &str, body: serde_json::Value) {
            let panel = self.app.get_webview_window("main").expect("没有面板");
            let request = InvokeRequest {
                cmd: cmd.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            };
            get_ipc_response(&panel, request).expect("命令没有返回");
        }

        /// 已经推给前端的快照。
        fn snapshots(&self) -> Vec<crate::timer::state::TimerSnapshot> {
            self.recorder
                .calls()
                .into_iter()
                .filter_map(|c| match c {
                    Call::EmitState(s) => Some(s),
                    _ => None,
                })
                .collect()
        }
    }

    impl Drop for TestApp {
        fn drop(&mut self) {
            // 时钟线程得停掉，不然它会一直跑到整个测试进程结束。
            if let Some(clock) = self.app.try_state::<ClockThread>() {
                clock.stop();
            }
        }
    }

    /// 现在的 epoch 毫秒。
    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("系统时间在 1970 之前")
            .as_millis() as u64
    }

    #[test]
    fn clicking_pause_switches_the_panel_to_paused() {
        let app = TestApp::start(TimerConfig::default());

        app.click("pause");

        assert!(
            app.snapshots().iter().any(|s| s.state == TimerState::Paused),
            "点了暂停，前端没收到已暂停的快照：{:?}",
            app.snapshots()
        );
    }

    #[test]
    fn clicking_resume_switches_the_panel_back_to_working() {
        let app = TestApp::start(TimerConfig::default());
        app.click("pause");
        let before = app.snapshots().len();

        app.click("resume");

        let after = &app.snapshots()[before..];
        assert!(
            after.iter().any(|s| s.state == TimerState::Working),
            "点了继续，前端没收到回到专注中的快照：{:?}",
            after
        );
    }

    #[test]
    fn clicking_restart_gives_a_full_work_session_again() {
        // 工作时段缩到一分钟：这样"重新计时"到底重置了没有，差别才量得出来。
        let cfg = TimerConfig {
            work_ms: 60_000,
            break_ms: 900_000,
            repeat_ms: 600_000,
        };
        let app = TestApp::start(cfg);
        // 先耗掉一点，再重新计时——不然新旧截止时刻几乎一样，测不出重置。
        std::thread::sleep(Duration::from_millis(1_200));
        app.click("pause");
        let before = app.snapshots().len();

        let clicked_at = now_ms();
        app.click("restart");

        let after = &app.snapshots()[before..];
        let restarted = after
            .iter()
            .find(|s| s.state == TimerState::Working)
            .expect("重新计时后没回到专注中");
        let deadline = restarted.deadline_ms.expect("重新计时后应该有倒计时");
        // 新的截止时刻 = 点的那一刻 + 一个完整工作时段。
        let expected = clicked_at + cfg.work_ms;
        assert!(
            deadline.abs_diff(expected) < 500,
            "重新计时后的截止时刻是 {deadline}，期望 {expected} 上下 500ms"
        );
    }

    #[test]
    fn clicking_acknowledge_starts_the_break() {
        // 工作时段一眨眼就到，好让状态机先站到"待确认提醒"上。
        let cfg = TimerConfig {
            work_ms: 30,
            break_ms: 600_000,
            repeat_ms: 600_000,
        };
        let app = TestApp::start(cfg);
        assert!(
            app.recorder
                .wait_for_state(TimerState::WorkAlerting, Duration::from_secs(2)),
            "没等到待确认提醒：{:?}",
            app.snapshots()
        );
        let before = app.snapshots().len();

        app.click("acknowledge");

        let after = &app.snapshots()[before..];
        assert!(
            after.iter().any(|s| s.state == TimerState::Breaking),
            "点了马上去休息，前端没收到休息中的快照：{:?}",
            after
        );
    }

    #[test]
    fn an_unknown_action_changes_nothing() {
        let app = TestApp::start(TimerConfig::default());
        let before = app.snapshots().len();

        app.click("胡来");

        assert_eq!(app.snapshots().len(), before, "看不懂的动作不该推任何快照");
    }

    #[test]
    fn closing_the_panel_does_not_disturb_the_timer() {
        // 面板右上角的 ✕：只藏窗口，计时继续。
        //
        // 这里能验的是"计时没被动过"——窗口真的藏没藏起来，得在真机上看
        // （假运行时的 `hide()` 是个空实现，而且投递到主线程的任务没人跑）。
        let app = TestApp::start(TimerConfig::default());
        let before = app.snapshots().len();

        app.invoke("hide_panel", serde_json::json!({}));

        assert_eq!(
            app.snapshots().len(),
            before,
            "藏面板不该推任何快照：{:?}",
            app.snapshots()
        );
    }
}
