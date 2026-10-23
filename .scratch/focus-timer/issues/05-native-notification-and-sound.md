# 05 — Rust 侧系统通知与系统音效

Status: ready-for-agent

## 目标

在 Rust 侧实现两种提醒手段：系统通知横幅 + macOS 系统音效。**不依赖 webview 的音频上下文**。

## 验收

- [ ] `src-tauri/src/notify.rs` 暴露 `notify(title, body)` 与 `play_sound()`
- [ ] 通知走 `tauri-plugin-notification` 的 Rust API
- [ ] 音效用 macOS 系统音效：调用 `AudioServicesPlaySystemSound`（需要 `objc2` / `core_audio` 之类的 crate），**不使用 Web Audio**
- [ ] 三个时机都能发声：工作时段结束、10 分钟重复提醒、休息时段结束
- [ ] 通知文案为中文，且分别对应三个时机
- [ ] 通知权限被拒时**不 panic、不报错刷屏**，静默降级（面板与托盘仍是可靠提醒途径）

## 背景

- 提醒矩阵见 [`spec.md` §5.5](../spec.md)
- 为什么不用前端 Web Audio：AudioContext 在没有用户交互时处于 suspended 状态，而这个 app 启动即自己计时、用户可能一次都没点过屏幕 → 首次提醒会一声不吭
- 依赖 02 的副作用分发机制与 03 的时钟线程
