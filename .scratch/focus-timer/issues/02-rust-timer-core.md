# 02 — Rust 纯函数状态机 + 单元测试

Status: ready-for-agent

## 目标

在 Rust 侧实现计时状态机，写成**不依赖真实时间、不做 I/O 的纯函数**，用 `cargo test` 覆盖每一状态转移。

## 验收

- [ ] `src-tauri/src/timer/state.rs` 定义 `TimerState { WORKING, PAUSED, WORK_ALERTING, BREAKING }`
- [ ] 定义 `TimerCore`（持有 `state`、`target_ms`、`next_repeat_ms`）与 `Event { Start, Pause, Resume, WorkElapsed, AlertRepeat, Acknowledge, BreakElapsed }`
- [ ] 核心函数形如 `fn reduce(core: &TimerCore, event: Event, now: u64) -> Transition`，`Transition` 携带新状态与副作用列表（`Notify` / `PlaySound` / `ShowPanel` / `HidePanel` / `SetTrayAlert` 等）
- [ ] `now` 作为参数传入，函数体内不调用 `SystemTime::now()`
- [ ] 时长来自 `config.rs` 的常量（60 / 15 / 10 分钟）
- [ ] 单元测试覆盖 [`spec.md` §4.2](../spec.md) 转移表的**每一行**，以及：
  - [ ] `WorkElapsed` 在 `now < target` 时不触发
  - [ ] `Acknowledge` 只在 `WORK_ALERTING` 下有效
  - [ ] `Pause` 只在 `WORKING` 下有效，在 `WORK_ALERTING` / `BREAKING` 下是 no-op
  - [ ] `AlertRepeat` 后 `next_repeat` 重新计算为 `now + 10m`
  - [ ] 暂停冻结剩余时间，`Resume` 后 `target = now + remaining`
- [ ] `cargo test` 全绿

## 背景

- 转移表见 [`spec.md` §4.2](../spec.md)
- 一切以绝对时间戳为准，见 [ADR-0001](../../../../docs/adr/0001-timestamp-based-timing.md)
- 状态机为什么在 Rust 而不是前端，见 [ADR-0002](../../../../docs/adr/0002-authoritative-clock-in-rust.md)
