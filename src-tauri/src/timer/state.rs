//! 计时状态机。
//!
//! 这一层是纯函数：不读时钟、不做 I/O、不碰 Tauri。所有时间都由调用方以
//! `now`（epoch 毫秒）传入，因此每一条转移都能被单元测试精确驱动。
//!
//! 不变量：**一切以绝对时间戳为准**。状态只记住"下一件事发生在什么时刻"，
//! 剩余时间永远由 `deadline - now` 现算，不做累加（见 ADR-0001）。

use crate::config::TimerConfig;

/// 计时器所处的四种状态。
///
/// 应用启动即进入 `Working`，不存在可停留的空闲态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TimerState {
    /// 工作时段进行中。
    Working,
    /// 工作时段被暂停，剩余时间冻结。
    Paused,
    /// 工作时段已结束、等待用户确认休息。
    WorkAlerting,
    /// 休息时段进行中。
    Breaking,
}

/// 能推动状态机前进的事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// 重新计时：丢弃当前进度，开启一个完整的工作时段。
    Restart,
    /// 暂停工作时段。只在 `Working` 下有效。
    Pause,
    /// 恢复工作时段。只在 `Paused` 下有效。
    Resume,
    /// 工作时段到期。只在 `Working` 且已过截止时刻时有效。
    WorkElapsed,
    /// 待确认提醒到期（距上次提醒已过重复间隔）。
    AlertRepeat,
    /// 用户确认去休息。只在 `WorkAlerting` 下有效。
    Acknowledge,
    /// 休息时段到期。只在 `Breaking` 且已过截止时刻时有效。
    BreakElapsed,
}

/// 状态转移带来的副作用。由外层（托盘 / 通知 / 音效 / 面板）执行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// 发一条系统通知。
    Notify { title: &'static str, body: &'static str },
    /// 播放一次系统音效。
    PlaySound,
    /// 显示并置顶面板。**不得抢占焦点**——提醒要显眼，但不该打断手头的事。
    ShowPanel,
    /// 设置托盘的提醒态样式（醒目图标）。
    SetTrayAlert(bool),
}

/// 计时器的全部可变状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimerCore {
    /// 当前状态。
    pub state: TimerState,
    /// 当前阶段到期的**绝对时刻**（epoch 毫秒）。`Paused` 时为 0。
    pub deadline_ms: u64,
    /// 暂停时冻结的剩余毫秒数。仅 `Paused` 时有意义。
    pub remaining_ms: u64,
}

impl TimerCore {
    /// 应用启动：立即进入一个完整的工作时段。
    pub fn start(now: u64, cfg: &TimerConfig) -> Self {
        Self {
            state: TimerState::Working,
            deadline_ms: now + cfg.work_ms,
            remaining_ms: 0,
        }
    }

    /// 距当前阶段结束还剩多少毫秒。暂停时返回冻结值。
    pub fn remaining_ms(&self, now: u64) -> u64 {
        match self.state {
            TimerState::Paused => self.remaining_ms,
            _ => self.deadline_ms.saturating_sub(now),
        }
    }

    /// 生成给前端的快照。
    ///
    /// 提醒态没有倒计时（`deadline_ms` 为 `None`），暂停态的截止时刻是冻结值
    /// 而非绝对时刻（同样为 `None`，改用 `remaining_ms`）。
    pub fn snapshot(&self) -> TimerSnapshot {
        match self.state {
            TimerState::Paused => TimerSnapshot {
                state: self.state,
                deadline_ms: None,
                remaining_ms: self.remaining_ms,
            },
            TimerState::WorkAlerting => TimerSnapshot {
                state: self.state,
                deadline_ms: None,
                remaining_ms: 0,
            },
            _ => TimerSnapshot {
                state: self.state,
                deadline_ms: Some(self.deadline_ms),
                remaining_ms: 0,
            },
        }
    }
}

/// 传给前端的计时快照。
///
/// 前端是纯视图：拿 `deadlineMs` 本地插值渲染倒计时，拿 `state` 决定画哪个面板。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerSnapshot {
    /// 当前状态。
    pub state: TimerState,
    /// 当前阶段到期的绝对时刻（epoch 毫秒）。无倒计时时为 `None`。
    pub deadline_ms: Option<u64>,
    /// 暂停时冻结的剩余毫秒数。其余状态为 0。
    pub remaining_ms: u64,
}

/// 一次状态转移的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    /// 转移后的新状态。
    pub core: TimerCore,
    /// 需要外层执行的副作用。
    pub effects: Vec<Effect>,
}

// 通知文案。
const WORK_ALERT_TITLE: &str = "工作时间到";
const WORK_ALERT_BODY: &str = "你已连续工作 1 小时，该休息 15 分钟了。";
const REPEAT_ALERT_TITLE: &str = "还在工作？";
const REPEAT_ALERT_BODY: &str = "距离上次提醒已过去 10 分钟，请立即休息。";
const BREAK_END_TITLE: &str = "休息结束";
const BREAK_END_BODY: &str = "15 分钟休息已结束，开始新的 1 小时专注。";

/// 推进状态机一步。
///
/// 非法的事件组合（状态与事件不匹配，或事件早于截止时刻）一律是空转移：
/// 状态不变、无副作用。这让调用方可以放心地"随时问一次"。
pub fn reduce(core: &TimerCore, event: Event, now: u64, cfg: &TimerConfig) -> Transition {
    use TimerState::*;
    use Event::*;

    match (core.state, event) {
        // ── 工作时段 ──────────────────────────────────────────────────────
        (Working, Restart) => Transition {
            core: TimerCore::start(now, cfg),
            effects: vec![Effect::SetTrayAlert(false)],
        },
        (Working, Pause) => Transition {
            core: TimerCore {
                state: Paused,
                deadline_ms: 0,
                remaining_ms: core.deadline_ms.saturating_sub(now),
            },
            effects: vec![],
        },
        (Working, WorkElapsed) if now >= core.deadline_ms => Transition {
            core: TimerCore {
                state: WorkAlerting,
                deadline_ms: now + cfg.repeat_ms,
                remaining_ms: 0,
            },
            effects: vec![
                Effect::Notify {
                    title: WORK_ALERT_TITLE,
                    body: WORK_ALERT_BODY,
                },
                Effect::PlaySound,
                Effect::ShowPanel,
                Effect::SetTrayAlert(true),
            ],
        },

        // ── 暂停 ─────────────────────────────────────────────────────────
        (Paused, Resume) => Transition {
            core: TimerCore {
                state: Working,
                deadline_ms: now + core.remaining_ms,
                remaining_ms: 0,
            },
            effects: vec![],
        },
        (Paused, Restart) => Transition {
            core: TimerCore::start(now, cfg),
            effects: vec![Effect::SetTrayAlert(false)],
        },

        // ── 待确认提醒 ───────────────────────────────────────────────────
        (WorkAlerting, Acknowledge) => Transition {
            core: TimerCore {
                state: Breaking,
                deadline_ms: now + cfg.break_ms,
                remaining_ms: 0,
            },
            effects: vec![Effect::SetTrayAlert(false)],
        },
        (WorkAlerting, AlertRepeat) if now >= core.deadline_ms => Transition {
            core: TimerCore {
                state: WorkAlerting,
                deadline_ms: now + cfg.repeat_ms,
                remaining_ms: 0,
            },
            effects: vec![
                Effect::Notify {
                    title: REPEAT_ALERT_TITLE,
                    body: REPEAT_ALERT_BODY,
                },
                Effect::PlaySound,
                Effect::ShowPanel,
            ],
        },

        // ── 休息时段 ─────────────────────────────────────────────────────
        (Breaking, BreakElapsed) if now >= core.deadline_ms => Transition {
            core: TimerCore {
                state: Working,
                deadline_ms: now + cfg.work_ms,
                remaining_ms: 0,
            },
            effects: vec![
                Effect::Notify {
                    title: BREAK_END_TITLE,
                    body: BREAK_END_BODY,
                },
                Effect::PlaySound,
                Effect::SetTrayAlert(false),
            ],
        },

        // ── 其余组合一律忽略 ─────────────────────────────────────────────
        _ => Transition {
            core: *core,
            effects: vec![],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timer::test_support::{cfg, MIN};

    fn step(core: &TimerCore, event: Event, now: u64) -> Transition {
        reduce(core, event, now, &cfg())
    }

    fn has_effect(t: &Transition, e: Effect) -> bool {
        t.effects.contains(&e)
    }

    /// 取出一次转移里的通知文案（标题，正文）。
    fn notify_text(t: &Transition) -> Option<(&'static str, &'static str)> {
        t.effects.iter().find_map(|e| match e {
            Effect::Notify { title, body } => Some((*title, *body)),
            _ => None,
        })
    }

    /// 提醒的完整路径：工作时段结束 → 重复提醒 → 确认休息 → 休息时段结束。
    ///
    /// 走完整路径而不是直接摆状态：这样文案和触发它的时机不会走散。
    /// 「确认休息」也在里面：spec §5.5 说它既不通知也不出声——提醒已经响过了，
    /// 用户刚回应过，这时候再顶一下是吵。
    struct ReminderPath {
        /// 工作时段结束。
        work: Transition,
        /// 10 分钟未确认，重复提醒。
        repeat: Transition,
        /// 用户确认去休息。
        acknowledged: Transition,
        /// 休息时段结束。
        break_end: Transition,
    }

    fn the_reminder_path() -> ReminderPath {
        let work = step(&TimerCore::start(0, &cfg()), Event::WorkElapsed, 60 * MIN);
        let repeat = step(&work.core, Event::AlertRepeat, 70 * MIN);
        let acknowledged = step(&repeat.core, Event::Acknowledge, 70 * MIN);
        let break_end = step(&acknowledged.core, Event::BreakElapsed, 85 * MIN);
        ReminderPath {
            work,
            repeat,
            acknowledged,
            break_end,
        }
    }

    /// 三次提醒（不含「确认休息」）。
    fn the_reminders(path: &ReminderPath) -> [&Transition; 3] {
        [&path.work, &path.repeat, &path.break_end]
    }

    /// 是不是一个汉字。
    fn is_han(c: char) -> bool {
        ('\u{4e00}'..='\u{9fff}').contains(&c)
    }

    // ── 启动 ────────────────────────────────────────────────────────────

    #[test]
    fn start_enters_working_with_a_full_session() {
        let core = TimerCore::start(1_000, &cfg());
        assert_eq!(core.state, TimerState::Working);
        assert_eq!(core.deadline_ms, 1_000 + 60 * MIN);
        assert_eq!(core.remaining_ms(1_000), 60 * MIN);
    }

    // ── 暂停 / 恢复 ─────────────────────────────────────────────────────

    #[test]
    fn pause_freezes_the_remaining_time() {
        let core = TimerCore::start(0, &cfg());
        let t = step(&core, Event::Pause, 20 * MIN);
        assert_eq!(t.core.state, TimerState::Paused);
        assert_eq!(t.core.remaining_ms, 40 * MIN);
        // 暂停不再有任何截止时刻。
        assert_eq!(t.core.deadline_ms, 0);
        assert!(t.effects.is_empty());
    }

    #[test]
    fn paused_remaining_does_not_tick_down_with_time() {
        let core = TimerCore::start(0, &cfg());
        let t = step(&core, Event::Pause, 20 * MIN);
        // 过了 10 分钟，冻结值不变。
        assert_eq!(t.core.remaining_ms(30 * MIN), 40 * MIN);
        assert_eq!(t.core.remaining_ms(999 * MIN), 40 * MIN);
    }

    #[test]
    fn resume_continues_from_the_frozen_remaining() {
        let core = TimerCore::start(0, &cfg());
        let paused = step(&core, Event::Pause, 20 * MIN);
        let t = step(&paused.core, Event::Resume, 100 * MIN);
        assert_eq!(t.core.state, TimerState::Working);
        // 续走剩余的 40 分钟，而不是重新给满 60 分钟。
        assert_eq!(t.core.deadline_ms, 100 * MIN + 40 * MIN);
    }

    #[test]
    fn pause_is_ignored_while_alerting() {
        // 待确认提醒无法被暂停——否则就成了躲开休息的后门。
        let core = TimerCore {
            state: TimerState::WorkAlerting,
            deadline_ms: 10 * MIN,
            remaining_ms: 0,
        };
        let t = step(&core, Event::Pause, 10 * MIN);
        assert_eq!(t.core, core);
        assert!(t.effects.is_empty());
    }

    #[test]
    fn pause_is_ignored_while_breaking() {
        let core = TimerCore {
            state: TimerState::Breaking,
            deadline_ms: 15 * MIN,
            remaining_ms: 0,
        };
        let t = step(&core, Event::Pause, 15 * MIN);
        assert_eq!(t.core, core);
        assert!(t.effects.is_empty());
    }

    #[test]
    fn resume_is_ignored_while_working() {
        let core = TimerCore::start(0, &cfg());
        let t = step(&core, Event::Resume, 5 * MIN);
        assert_eq!(t.core, core);
        assert!(t.effects.is_empty());
    }

    // ── 工作时段到期 ────────────────────────────────────────────────────

    #[test]
    fn work_elapsed_moves_to_alerting_and_schedules_the_repeat() {
        let core = TimerCore::start(0, &cfg());
        let t = step(&core, Event::WorkElapsed, 60 * MIN);
        assert_eq!(t.core.state, TimerState::WorkAlerting);
        // 重复间隔从**本次提醒发出的时刻**起算。
        assert_eq!(t.core.deadline_ms, 60 * MIN + 10 * MIN);
    }

    #[test]
    fn work_elapsed_alerts_with_notify_sound_and_panel() {
        let core = TimerCore::start(0, &cfg());
        let t = step(&core, Event::WorkElapsed, 60 * MIN);
        assert!(has_effect(&t, Effect::PlaySound));
        assert!(has_effect(&t, Effect::ShowPanel));
        assert!(has_effect(&t, Effect::SetTrayAlert(true)));
        assert!(t
            .effects
            .iter()
            .any(|e| matches!(e, Effect::Notify { title, .. } if *title == WORK_ALERT_TITLE)));
    }

    #[test]
    fn work_elapsed_is_ignored_before_the_deadline() {
        let core = TimerCore::start(0, &cfg());
        let t = step(&core, Event::WorkElapsed, 59 * MIN);
        assert_eq!(t.core, core);
        assert!(t.effects.is_empty());
    }

    // ── 重复提醒 ────────────────────────────────────────────────────────

    #[test]
    fn alert_repeat_reschedules_and_re_alerts() {
        let core = TimerCore {
            state: TimerState::WorkAlerting,
            deadline_ms: 10 * MIN,
            remaining_ms: 0,
        };
        let t = step(&core, Event::AlertRepeat, 10 * MIN);
        assert_eq!(t.core.state, TimerState::WorkAlerting);
        assert_eq!(t.core.deadline_ms, 20 * MIN);
        assert!(has_effect(&t, Effect::PlaySound));
        assert!(has_effect(&t, Effect::ShowPanel));
    }

    #[test]
    fn alert_repeat_is_ignored_before_the_deadline() {
        let core = TimerCore {
            state: TimerState::WorkAlerting,
            deadline_ms: 10 * MIN,
            remaining_ms: 0,
        };
        let t = step(&core, Event::AlertRepeat, 9 * MIN);
        assert_eq!(t.core, core);
        assert!(t.effects.is_empty());
    }

    #[test]
    fn alert_repeat_never_stops_on_its_own() {
        // 无限重复，直到用户确认——这是产品明确要的行为。
        let mut core = TimerCore::start(0, &cfg());
        let mut now = 60 * MIN;
        core = step(&core, Event::WorkElapsed, now).core;
        for i in 1..=20 {
            now += 10 * MIN;
            let t = step(&core, Event::AlertRepeat, now);
            core = t.core;
            assert_eq!(core.state, TimerState::WorkAlerting, "第 {i} 次重复后不应离开提醒态");
            assert_eq!(core.deadline_ms, now + 10 * MIN);
        }
    }

    // ── 确认休息 ────────────────────────────────────────────────────────

    #[test]
    fn acknowledge_starts_the_break_and_stops_the_repeat() {
        let core = TimerCore {
            state: TimerState::WorkAlerting,
            deadline_ms: 10 * MIN,
            remaining_ms: 0,
        };
        let t = step(&core, Event::Acknowledge, 12 * MIN);
        assert_eq!(t.core.state, TimerState::Breaking);
        // 休息从**确认的那一刻**起算 15 分钟。
        assert_eq!(t.core.deadline_ms, 12 * MIN + 15 * MIN);
        assert!(has_effect(&t, Effect::SetTrayAlert(false)));
        // 确认后不该再有提醒类的副作用。
        assert!(!has_effect(&t, Effect::PlaySound));
        assert!(!has_effect(&t, Effect::ShowPanel));
    }

    #[test]
    fn acknowledge_is_ignored_outside_alerting() {
        let core = TimerCore::start(0, &cfg());
        let t = step(&core, Event::Acknowledge, 5 * MIN);
        assert_eq!(t.core, core);
        assert!(t.effects.is_empty());
    }

    // ── 休息时段 ────────────────────────────────────────────────────────

    #[test]
    fn break_elapsed_starts_the_next_work_session() {
        let core = TimerCore {
            state: TimerState::Breaking,
            deadline_ms: 15 * MIN,
            remaining_ms: 0,
        };
        let t = step(&core, Event::BreakElapsed, 15 * MIN);
        assert_eq!(t.core.state, TimerState::Working);
        assert_eq!(t.core.deadline_ms, 15 * MIN + 60 * MIN);
        assert!(has_effect(&t, Effect::PlaySound));
        assert!(has_effect(&t, Effect::SetTrayAlert(false)));
    }

    #[test]
    fn break_elapsed_is_ignored_before_the_deadline() {
        let core = TimerCore {
            state: TimerState::Breaking,
            deadline_ms: 15 * MIN,
            remaining_ms: 0,
        };
        let t = step(&core, Event::BreakElapsed, 14 * MIN);
        assert_eq!(t.core, core);
        assert!(t.effects.is_empty());
    }

    // ── 重新计时 ────────────────────────────────────────────────────────

    #[test]
    fn restart_resets_to_a_full_work_session() {
        let core = TimerCore::start(0, &cfg());
        let t = step(&core, Event::Restart, 33 * MIN);
        assert_eq!(t.core.state, TimerState::Working);
        assert_eq!(t.core.deadline_ms, 33 * MIN + 60 * MIN);
    }

    #[test]
    fn restart_is_ignored_while_alerting() {
        // 待确认提醒只能被"确认去休息"结束——不能靠重新计时躲开休息。
        let core = TimerCore {
            state: TimerState::WorkAlerting,
            deadline_ms: 10 * MIN,
            remaining_ms: 0,
        };
        let t = step(&core, Event::Restart, 10 * MIN);
        assert_eq!(t.core, core);
        assert!(t.effects.is_empty());
    }

    #[test]
    fn restart_is_ignored_while_breaking() {
        let core = TimerCore {
            state: TimerState::Breaking,
            deadline_ms: 15 * MIN,
            remaining_ms: 0,
        };
        let t = step(&core, Event::Restart, 15 * MIN);
        assert_eq!(t.core, core);
        assert!(t.effects.is_empty());
    }

    // ── 睡眠跨越截止时刻 ────────────────────────────────────────────────

    #[test]
    fn a_long_sleep_still_triggers_the_work_alert() {
        // 时间戳方案的核心价值：睡了 8 小时，醒来时差值直接算出 8 小时。
        let core = TimerCore::start(0, &cfg());
        let t = step(&core, Event::WorkElapsed, 8 * 60 * MIN);
        assert_eq!(t.core.state, TimerState::WorkAlerting);
        // 重复提醒从**醒来这一刻**起算，而不是从原定的 60 分钟起算。
        assert_eq!(t.core.deadline_ms, 8 * 60 * MIN + 10 * MIN);
    }

    // ── 配置 ────────────────────────────────────────────────────────────

    #[test]
    fn a_work_session_uses_the_configured_work_duration() {
        let mut c = cfg();
        c.work_ms = 42;
        let core = TimerCore::start(0, &c);
        assert_eq!(core.deadline_ms, 42);
    }

    // ── 提醒的文案与音效 ────────────────────────────────────────────────

    #[test]
    fn every_reminder_notifies_and_plays_a_sound() {
        // 三个时机（工作时段结束 / 10 分钟重复提醒 / 休息时段结束）都得又通知
        // 又出声——漏了哪一个，那个时机就只剩面板在提醒。
        let path = the_reminder_path();
        for (i, t) in the_reminders(&path).into_iter().enumerate() {
            assert!(notify_text(t).is_some(), "第 {i} 次提醒没有通知");
            assert!(has_effect(t, Effect::PlaySound), "第 {i} 次提醒没有音效");
        }
    }

    #[test]
    fn acknowledging_the_reminder_is_silent() {
        // spec §5.5 的提醒矩阵里，「确认休息」那一行的通知和音效都是「—」。
        // 提醒刚响过，用户已经回应过了，这时候再顶一下是吵。
        let path = the_reminder_path();
        assert!(
            notify_text(&path.acknowledged).is_none(),
            "确认休息不该再发通知"
        );
        assert!(
            !has_effect(&path.acknowledged, Effect::PlaySound),
            "确认休息不该再出声"
        );
    }

    #[test]
    fn the_notification_copy_is_chinese() {
        let path = the_reminder_path();
        for t in the_reminders(&path) {
            let (title, body) = notify_text(t).expect("提醒必须带文案");
            assert!(title.chars().any(is_han), "标题不是中文：{title}");
            assert!(body.chars().any(is_han), "正文不是中文：{body}");
        }
    }

    #[test]
    fn each_reminder_names_the_moment_it_fires_for() {
        // 三套话得能一眼分清是哪个时机：不然"休息结束"也写成"工作时间到"，
        // 用户根本不知道自己该干嘛。
        //
        // 这里写死字面量，而不是用 `WORK_ALERT_TITLE` 那几个常量：这条用例问的
        // 是"**用户看到的**是不是这句话"，常量自己跟自己比是问不出什么的。
        let path = the_reminder_path();
        assert_eq!(notify_text(&path.work).unwrap().0, "工作时间到");
        assert_eq!(notify_text(&path.repeat).unwrap().0, "还在工作？");
        assert_eq!(notify_text(&path.break_end).unwrap().0, "休息结束");
    }

    #[test]
    fn the_three_reminders_do_not_repeat_the_same_copy() {
        let path = the_reminder_path();
        let titles = [
            notify_text(&path.work).unwrap().0,
            notify_text(&path.repeat).unwrap().0,
            notify_text(&path.break_end).unwrap().0,
        ];
        let bodies = [
            notify_text(&path.work).unwrap().1,
            notify_text(&path.repeat).unwrap().1,
            notify_text(&path.break_end).unwrap().1,
        ];
        for i in 0..titles.len() {
            for j in (i + 1)..titles.len() {
                assert_ne!(titles[i], titles[j], "两个时机的标题撞了");
                assert_ne!(bodies[i], bodies[j], "两个时机的正文撞了");
            }
        }
    }
}
