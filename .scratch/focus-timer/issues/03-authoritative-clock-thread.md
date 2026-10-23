# 03 — Rust 权威时钟线程

Status: ready-for-agent

## 目标

启动一个 Rust 内部线程作为唯一时钟源：持有 `TimerCore`，周期性校验时间戳，驱动状态机推进，并向托盘推送剩余时间。

## 验收

- [ ] `src-tauri/src/timer/clock.rs` 提供 `ClockThread`，启动后独立运行
- [ ] 每 250ms 读取一次 `SystemTime::now()`，与 `target_ms` / `next_repeat_ms` 比较，构造相应 `Event` 送进状态机
- [ ] 状态机返回的副作用被分发执行（通知、音效、面板显示等）
- [ ] 每次 tick 把最新 `TimerSnapshot`（见 [`spec.md` §5.3](../spec.md)）emit 给前端，**仅在状态或剩余秒数发生变化时**
- [ ] 托盘标题每秒刷新一次（`MM:SS`），非秒级变化时不重复设置
- [ ] 线程在应用退出时干净停止（`Drop` 里置标志位 + `join`，不留下游离线程）
- [ ] 系统休眠跨越目标时刻后，唤醒时状态机立即推进到正确状态（不等到下一个 250ms tick 之后的额外延迟）

## 背景

- 这是整个 app 的心跳来源；前端只做插值渲染，见 [ADR-0002](../../../../docs/adr/0002-authoritative-clock-in-rust.md)
- 依赖 02 提供的 `TimerCore` 与 `reduce`
- 睡眠语义见 [`spec.md` §4.3](../spec.md)
