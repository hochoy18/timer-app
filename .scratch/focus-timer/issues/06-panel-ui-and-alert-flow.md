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
- [ ] 倒计时由前端本地插值渲染：`remaining = max(0, targetMs - Date.now())`，每 250ms 更新一次
- [ ] 状态来源只有 Rust 的 `timer://state` 事件，前端不自己维护状态机
- [ ] 所有按钮调用 `invoke("timer_action", { action })`，`action ∈ { "pause" | "resume" | "restart" | "acknowledge" }`
- [ ] 按钮点击后立即反映新状态（不等下一个状态事件）
- [ ] 面板右上角 ✕ 只隐藏窗口，计时继续

## 背景

- 四态定义与按钮集见 [`spec.md` §4.1](../spec.md)
- 交互契约见 [`spec.md` §5.3](../spec.md)
- 前端是纯视图，节流无害——真值在 Rust，见 [ADR-0002](../../../../docs/adr/0002-authoritative-clock-in-rust.md)
