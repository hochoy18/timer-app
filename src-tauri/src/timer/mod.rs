pub mod clock;
pub mod state;

/// 测试用的公共脚手架：时长常量、规格里的三个时长，还有记录副作用的假分发器。
#[cfg(test)]
pub(crate) mod test_support {
    use crate::config::TimerConfig;
    use crate::timer::clock::Dispatcher;
    use crate::timer::state::{TimerSnapshot, TimerState};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    /// 一分钟（毫秒）。
    pub const MIN: u64 = 60_000;

    /// 规格里的三个时长：工作 60 分钟 / 休息 15 分钟 / 重复提醒 10 分钟。
    pub fn cfg() -> TimerConfig {
        TimerConfig {
            work_ms: 60 * MIN,
            break_ms: 15 * MIN,
            repeat_ms: 10 * MIN,
        }
    }

    /// 一次副作用的记录。
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Call {
        Notify(String, String),
        PlaySound,
        ShowPanel,
        SetTrayAlert(bool),
        SetTrayTitle(String),
        SetState(TimerState),
        EmitState(TimerSnapshot),
    }

    /// 记录所有副作用的假分发器。
    ///
    /// 住在公共脚手架里，是因为两个测试模块都要它：时钟线程自己的用例，和
    /// `lib.rs` 里走 IPC 通道点面板按钮的用例。
    #[derive(Default)]
    pub struct FakeDispatcher {
        calls: Mutex<Vec<Call>>,
    }

    impl FakeDispatcher {
        pub fn calls(&self) -> Vec<Call> {
            self.calls.lock().unwrap().clone()
        }

        /// 忘掉之前记下的一切：有些用例要先把启动那几帧清掉再看后面。
        pub fn clear(&self) {
            self.calls.lock().unwrap().clear();
        }

        pub fn titles(&self) -> Vec<String> {
            self.calls()
                .into_iter()
                .filter_map(|c| match c {
                    Call::SetTrayTitle(t) => Some(t),
                    _ => None,
                })
                .collect()
        }

        pub fn states(&self) -> Vec<TimerState> {
            self.calls()
                .into_iter()
                .filter_map(|c| match c {
                    Call::SetState(s) => Some(s),
                    _ => None,
                })
                .collect()
        }

        pub fn effects(&self) -> Vec<Call> {
            self.calls()
                .into_iter()
                .filter(|c| {
                    matches!(
                        c,
                        Call::Notify(..) | Call::PlaySound | Call::ShowPanel | Call::SetTrayAlert(..)
                    )
                })
                .collect()
        }

        /// 等到有一帧推给前端的快照落在期望的状态上。
        ///
        /// "按钮点了有没有换状态"这件事，前端看得见的正是这些快照——所以这里
        /// 等的是快照，不是内部状态。
        pub fn wait_for_state(&self, state: TimerState, within: Duration) -> bool {
            let deadline = Instant::now() + within;
            loop {
                if self
                    .calls()
                    .iter()
                    .any(|c| matches!(c, Call::EmitState(s) if s.state == state))
                {
                    return true;
                }
                if Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }

    impl Dispatcher for FakeDispatcher {
        fn notify(&self, title: &str, body: &str) {
            self.calls
                .lock()
                .unwrap()
                .push(Call::Notify(title.to_string(), body.to_string()));
        }

        fn play_sound(&self) {
            self.calls.lock().unwrap().push(Call::PlaySound);
        }

        fn show_panel(&self) {
            self.calls.lock().unwrap().push(Call::ShowPanel);
        }

        fn set_tray_alert(&self, alerting: bool) {
            self.calls
                .lock()
                .unwrap()
                .push(Call::SetTrayAlert(alerting));
        }

        fn set_tray_title(&self, title: &str) {
            self.calls
                .lock()
                .unwrap()
                .push(Call::SetTrayTitle(title.to_string()));
        }

        fn set_state(&self, state: TimerState) {
            self.calls.lock().unwrap().push(Call::SetState(state));
        }

        fn emit_state(&self, snapshot: &TimerSnapshot) {
            self.calls.lock().unwrap().push(Call::EmitState(*snapshot));
        }
    }
}
