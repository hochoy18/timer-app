# 06 — 面板四态 UI 与提醒流程

Status: ready-for-agent

## 目标

实现面板（Panel）的四个状态视图与全部按钮，并把按钮接到 Rust 的动作接口上。

## 验收

- [ ] 面板宽度约 320pt，macOS 风格卡片，无边框无标题栏
- [ ] `WORKING`：大号剩余时间 `MM:SS` + "专注工作中" + `暂停` `重新计时` 两个按钮
- [ ] `PAUSED`：显示冻结的剩余时间 + "已暂停" + `继续` `重新计时` 两个按钮
- [ ] `WORK_ALERTING`：高亮提示块 "工作已满 1 小时" / "若不确认，每 10 分钟将重复提醒" + 唯一的按钮 `马上去休息 15 分钟`
- [ ] `BREAKING`：剩余时间 + "休息中"，**无任何按钮**
- [ ] 倒计时由前端本地插值渲染：`remaining = max(0, deadlineMs - Date.now())`，每 250ms 更新一次；`deadlineMs === null` 时改用快照里的 `remainingMs`
- [ ] 状态来源只有 Rust 的 `timer://state` 事件，前端不自己维护状态机
- [ ] 所有按钮调用 `invoke("timer_action", { action })`，`action ∈ { "pause" | "resume" | "restart" | "acknowledge" }`
- [ ] 按钮点击后立即反映新状态（不等下一个状态事件）
- [ ] 面板右上角 ✕ 只隐藏窗口，计时继续

## 背景

- 四态定义与按钮集见 [`spec.md` §4.1](../spec.md)
- 交互契约见 [`spec.md` §5.3](../spec.md)
- 前端是纯视图，节流无害——真值在 Rust，见 [ADR-0002](../../../../docs/adr/0002-authoritative-clock-in-rust.md)

## Comments

### 2026-10-23 — 四个面板的 UI 已经写完了（本票大部分已完成）

搭骨架（票 01）时顺手把四态面板和按钮接线一起做了，所以本票剩下的只有：

- [ ] 面板关闭（系统关闭按钮 / `WindowEvent::CloseRequested`）只隐藏、不退出 —— **Rust 侧**，前端做不了
- [ ] 圆角透明窗口（`macOSPrivateApi: true` + `transparent: true`）—— 从票 01 挪过来的，见该票 Comments
- [ ] 端到端跑一遍：点每个按钮都能看到状态正确切换

前端这半边（`src/components/*.tsx` × 4、`useNow` 插值、`useTimerState` 订阅、✕ 走 `hidePanel()`）已经在 `fd0cf62` 里，`npm run typecheck` + `npm run build` 通过。**不要重写**，等 03 的时钟线程上线后端到端验证即可。
