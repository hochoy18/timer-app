//! 开发期时间加速开关的进程级验收。
//!
//! 票 09：把 60 / 15 / 10 分钟压成秒级，好让整个轮次能在几分钟内手动跑完。
//!
//! ## 为什么单独一个集成测试文件
//!
//! **环境变量是进程级的**——改它会影响整个进程。cargo 给每个集成测试文件一个
//! 独立的可执行文件，所以这里的改动不会漏到单元测试那边去；同一个文件里的
//! 用例则用一把锁串起来。
//!
//! ## 这个文件同时是两种构建的验收
//!
//! - debug 构建读 `FOCUS_TIMER_SCALE`（票 09 的正身）；
//! - release 构建无视它，时长恒为真实值——打包成 `.app` 之后的那一半归票 10。
//!
//! 所以同一份断言在两种构建下期望不同：`cargo test` 和 `cargo test --release`
//! 各验各的那一半。

use std::sync::Mutex;

use focus_timer_lib::config::TimerConfig;

/// 环境变量的名字。
///
/// 故意写死字面量，不引用 `config` 里的常量：这个名字是给开发者用的一纸
/// 契约（`FOCUS_TIMER_SCALE=0.01 npm run tauri dev`），改名就得改这里。
/// `the_env_var_name_is_the_contract` 那条用例再把它和库里的常量对一遍——
/// 两边必须指着同一个名字。
const SCALE_ENV_VAR: &str = "FOCUS_TIMER_SCALE";

/// 环境变量是进程级的：改它的用例必须串着跑。
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// 记下环境变量原来的值，退出时（无论用例是绿是红）放回去。
struct EnvGuard(Option<String>);

impl EnvGuard {
    /// 把环境变量设成一个值。
    fn set(value: &str) -> Self {
        let original = std::env::var(SCALE_ENV_VAR).ok();
        std::env::set_var(SCALE_ENV_VAR, value);
        Self(original)
    }

    /// 把环境变量拿掉。
    fn unset() -> Self {
        let original = std::env::var(SCALE_ENV_VAR).ok();
        std::env::remove_var(SCALE_ENV_VAR);
        Self(original)
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.0 {
            Some(v) => std::env::set_var(SCALE_ENV_VAR, v),
            None => std::env::remove_var(SCALE_ENV_VAR),
        }
    }
}

/// 取锁（锁中毒了就当它没中毒——数据本身没坏）。
fn lock_env() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ── 环境变量 → 时长 ────────────────────────────────────────────────────

#[test]
fn the_env_var_name_is_the_contract() {
    // 用它的人只会照着文档敲 `FOCUS_TIMER_SCALE=0.01 npm run tauri dev`。
    // 库里的常量改了这个名字，脚本就会静默失效——退回到真实时长，
    // 60 分钟一轮，没人发现得了。
    assert_eq!(
        SCALE_ENV_VAR,
        focus_timer_lib::config::SCALE_ENV_VAR,
        "环境变量的名字改了？契约就变了。"
    );
}

#[test]
fn the_scale_comes_from_the_environment() {
    let _guard = lock_env();
    let _env = EnvGuard::set("0.01");

    let cfg = TimerConfig::from_env_or_default();

    if cfg!(debug_assertions) {
        // debug 构建读这个变量：60 分钟 → 36 秒，15 分钟 → 9 秒，10 分钟 → 6 秒。
        assert_eq!(cfg.work_ms, 36_000);
        assert_eq!(cfg.break_ms, 9_000);
        assert_eq!(cfg.repeat_ms, 6_000);
    } else {
        // release 构建无视它：时长恒为真实值。
        assert_eq!(cfg, TimerConfig::default());
    }
}

#[test]
fn an_absent_env_var_means_the_real_durations() {
    let _guard = lock_env();
    let _env = EnvGuard::unset();

    assert_eq!(TimerConfig::from_env_or_default(), TimerConfig::default());
}

// ── 缩放之后的整个轮次 ────────────────────────────────────────────────

/// 整个轮次的验收只在 debug 构建里有意义。
///
/// release 构建里环境变量被无视，一轮 60 分钟——等不起，也没什么可验的
/// （那个构建的断言在上面的用例里：时长恒为真实值）。
#[cfg(debug_assertions)]
mod cycle {
    use super::*;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use focus_timer_lib::timer::clock::{ClockThread, Dispatcher};
    use focus_timer_lib::timer::state::{Event, TimerSnapshot, TimerState};

    /// 记下两类东西的假分发器：提醒的标题，状态变迁。
    ///
    /// 只记，不通知任何人——这个用例要验的是"什么时候、发了哪条提醒"，
    /// 不是"系统通知长什么样"（那是 `notify` 模块自己的事）。
    /// 提醒就是靠系统通知送到用户眼前的，所以记标题正好认得出是哪条提醒。
    #[derive(Default)]
    struct Recorder {
        reminders: Mutex<Vec<String>>,
        states: Mutex<Vec<TimerState>>,
    }

    impl Recorder {
        fn reminders(&self) -> Vec<String> {
            self.reminders
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
        }

        /// 提醒的标题，打个折扣再写进失败信息。
        ///
        /// 重复提醒没有次数上限（spec §4.3），等不到的那几秒里它能刷出上百条
        /// ——失败信息里塞上百个标题，谁也读不下去。
        fn reminder_log(&self) -> String {
            let all = self.reminders();
            let head: Vec<&str> = all.iter().take(8).map(|s| s.as_str()).collect();
            if all.len() <= 8 {
                format!("{head:?}")
            } else {
                format!("{head:?}…（共 {} 条）", all.len())
            }
        }

        /// 状态变迁的顺序。
        fn states(&self) -> Vec<TimerState> {
            self.states
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
        }

        /// 等到状态变迁的顺序**以**期望的那一段**开头**。
        ///
        /// 用前缀而不是"包含"：休息结束之后又是一个新的工作时段，后面还会接着
        /// 提醒——前缀才说得清"回到工作"这件事发生在哪一步。
        fn wait_for_states(&self, expected: &[TimerState], within: Duration) -> bool {
            let deadline = Instant::now() + within;
            loop {
                if self.states().starts_with(expected) {
                    return true;
                }
                if Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        }

        /// 等到提醒的标题按顺序都出现过。
        ///
        /// 用"按顺序都出现过"而不是"开头就是"：重复提醒可能多来一条（60 毫秒
        /// 的间隔，差一点就多一条），多出来的那条不该让用例翻脸。
        fn wait_for_reminders_in_order(&self, expected: &[&str], within: Duration) -> bool {
            let deadline = Instant::now() + within;
            loop {
                let seen = self.reminders();
                let mut rest = seen.iter();
                if expected.iter().all(|e| rest.any(|s| s.as_str() == *e)) {
                    return true;
                }
                if Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }

    impl Dispatcher for Recorder {
        fn notify(&self, title: &str, _body: &str) {
            self.reminders
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(title.to_string());
        }

        fn play_sound(&self) {}

        fn show_panel(&self) {}

        fn set_tray_alert(&self, _alerting: bool) {}

        fn set_tray_title(&self, _title: &str) {}

        fn set_state(&self, state: TimerState) {
            self.states
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(state);
        }

        fn emit_state(&self, _snapshot: &TimerSnapshot) {}
    }

    #[test]
    fn the_whole_cycle_runs_in_seconds() {
        // 这是票 09 最后一条验收的自动化版本：
        // 工作 → 提醒 → 重复提醒 → 确认 → 休息 → 休息结束 → 回到工作。
        let _guard = lock_env();
        // 0.0001：60 分钟 → 360 毫秒，15 分钟 → 90 毫秒，10 分钟 → 60 毫秒。
        let _env = EnvGuard::set("0.0001");

        let cfg = TimerConfig::from_env_or_default();
        assert_eq!(cfg.work_ms, 360, "环境变量没生效，时长还是真实值");
        assert_eq!(cfg.break_ms, 90, "休息时段没跟着缩放");
        assert_eq!(cfg.repeat_ms, 60, "重复提醒没跟着缩放");

        let recorder = Arc::new(Recorder::default());
        let clock = ClockThread::start_with_interval(
            Arc::clone(&recorder) as Arc<dyn Dispatcher>,
            cfg,
            Duration::from_millis(10),
        );

        // 工作时段结束 → 提醒；再过一个重复间隔 → 重复提醒。
        assert!(
            recorder.wait_for_reminders_in_order(&["工作时间到", "还在工作？"], Duration::from_secs(5)),
            "没等到工作提醒和重复提醒：{:?}",
            recorder.reminder_log()
        );
        assert!(
            recorder.wait_for_states(
                &[TimerState::Working, TimerState::WorkAlerting],
                Duration::from_secs(5)
            ),
            "没走到待确认提醒：{:?}",
            recorder.states()
        );

        // 确认 → 休息时段。
        clock.apply(Event::Acknowledge);
        assert!(
            recorder.wait_for_states(
                &[
                    TimerState::Working,
                    TimerState::WorkAlerting,
                    TimerState::Breaking,
                ],
                Duration::from_secs(5)
            ),
            "确认后没进入休息：{:?}",
            recorder.states()
        );

        // 休息结束 → 一条"休息结束"提醒 + 回到工作时段。
        assert!(
            recorder.wait_for_reminders_in_order(
                &["工作时间到", "还在工作？", "休息结束"],
                Duration::from_secs(5)
            ),
            "休息结束该有一条提醒：{:?}",
            recorder.reminder_log()
        );
        assert!(
            recorder.wait_for_states(
                &[
                    TimerState::Working,
                    TimerState::WorkAlerting,
                    TimerState::Breaking,
                    TimerState::Working,
                ],
                Duration::from_secs(5)
            ),
            "休息结束后没回到工作：{:?}",
            recorder.states()
        );

        clock.stop();
    }
}
