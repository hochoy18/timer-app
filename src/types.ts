/** 与 Rust 侧 `timer::state::TimerSnapshot` 一一对应。 */
export type TimerState = "working" | "paused" | "workAlerting" | "breaking";

export interface TimerSnapshot {
  /** 当前状态。 */
  state: TimerState;
  /** 当前阶段到期的绝对时刻（epoch 毫秒）。无倒计时时为 null。 */
  deadlineMs: number | null;
  /** 暂停时冻结的剩余毫秒数。其余状态为 0。 */
  remainingMs: number;
}

/** 用户动作。Rust 侧 `timer_action` 命令接受的取值。 */
export type TimerAction = "pause" | "resume" | "restart" | "acknowledge";
