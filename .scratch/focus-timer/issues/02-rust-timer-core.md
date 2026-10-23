# 02 — Rust 纯函数状态机 + 单元测试

Status: ready-for-human

## 目标

在 Rust 侧实现计时状态机，写成**不依赖真实时间、不做 I/O 的纯函数**，用 `cargo test` 覆盖每一状态转移。

## 验收

- [x] `src-tauri/src/timer/state.rs` 定义 `TimerState { Working, Paused, WorkAlerting, Breaking }`
- [x] 定义 `TimerCore`（持有 `state`、`deadline_ms`、`remaining_ms`）与 `Event { Restart, Pause, Resume, WorkElapsed, AlertRepeat, Acknowledge, BreakElapsed }`
- [x] 核心函数 `fn reduce(core: &TimerCore, event: Event, now: u64, cfg: &TimerConfig) -> Transition`，`Transition` 携带新状态与副作用列表（`Notify` / `PlaySound` / `ShowPanel` / `SetTrayAlert`）
- [x] `now` 作为参数传入，函数体内不调用 `SystemTime::now()`
- [x] 时长来自 `config.rs` 的 `TimerConfig`（60 / 15 / 10 分钟）
- [x] 单元测试覆盖 [`spec.md` §4.2](../spec.md) 转移表的**每一行**，以及：
  - [x] `WorkElapsed` 在 `now < deadline` 时不触发
  - [x] `Acknowledge` 只在 `WorkAlerting` 下有效
  - [x] `Pause` 只在 `Working` 下有效，在 `WorkAlerting` / `Breaking` 下是 no-op
  - [x] `AlertRepeat` 后下一次重复重新计算为 `now + 10m`
  - [x] 暂停冻结剩余时间，`Resume` 后 `deadline = now + remaining`
- [ ] `cargo test` 全绿（22 个状态机用例 + 3 个配置用例已写好，**但一次都没跑过**——见 Comments）

## 背景

- 转移表见 [`spec.md` §4.2](../spec.md)
- 一切以绝对时间戳为准，见 [ADR-0001](../../../../docs/adr/0001-timestamp-based-timing.md)
- 状态机为什么在 Rust 而不是前端，见 [ADR-0002](../../../../docs/adr/0002-authoritative-clock-in-rust.md)

## Comments

### 2026-10-23 — 代码写完，22 个状态机用例 + 3 个配置用例，一个都没跑

`cargo test` 这条验收**没有验证过**：这台机器没装 Rust，Rust 侧一行都没编译过。装完工具链第一件事：

```bash
cd src-tauri && cargo test
```

#### 与原验收的两处偏离

**一、`TimerCore` 的字段不是 `target_ms` + `next_repeat_ms`，而是单个 `deadline_ms`。**

原设计有两个绝对时刻字段（工作终点、重复提醒时刻）。但四态里只有"暂停"没有倒计时，其余三态都只有一个"下一件事发生的时刻"，所以一个字段够了：暂停时 `deadline_ms` 归零、含义换成 `remaining_ms`（冻结值）。少一个字段就少一处能写错的不变量。

**二、`reduce` 多收一个 `cfg: &TimerConfig`。**

原签名是 `(core, event, now)`。加上 `cfg` 之后，状态机对**时间**纯（`now` 是参数、不读系统时钟），但对**时长**不纯——这正是想要的：`TimerConfig::from_env_or_default()` 在 debug 构建里会读 `FOCUS_TIMER_SCALE`，让测试能喂任意时长，也让 `config.rs` 成为时长的唯一出处，而不是散落的 `const`。放 `&` 是因为 `reduce` 不修改配置。

#### 设计上值得记一笔的两条

- **非法事件组合一律是空转移。** `(Paused, WorkElapsed)`、`(Working, Acknowledge)` 之类既不改状态也不发副作用。这让 03 的时钟线程可以放心地"随时问一次"，不需要在调用前判断当前是什么状态。
- **重复提醒没有次数上限，也没有"跨过多个间隔就补发几次"的逻辑。** 休眠 8 小时后唤醒，只重发一次，10 分钟后再来下一次，而不是一口气把睡过去的时间补成 48 次。这是对的行为：`a_long_sleep_still_triggers_the_work_alert` 锁住了它。