# 06 — 面板四态 UI 与提醒流程

Status: resolved

## 目标

实现面板（Panel）的四个状态视图与全部按钮，并把按钮接到 Rust 的动作接口上。

## 验收

- [x] 面板宽度约 320pt，macOS 风格卡片，无边框无标题栏
- [x] `WORKING`：大号剩余时间 `MM:SS` + "专注工作中" + `暂停` `重新计时` 两个按钮
- [x] `PAUSED`：显示冻结的剩余时间 + "已暂停" + `继续` `重新计时` 两个按钮
- [x] `WORK_ALERTING`：高亮提示块 "工作已满 1 小时" / "若不确认，每 10 分钟将重复提醒" + 唯一的按钮 `马上去休息 15 分钟`
- [x] `BREAKING`：剩余时间 + "休息中"，**无任何按钮**
- [x] 倒计时由前端本地插值渲染：`remaining = max(0, deadlineMs - Date.now())`，每 250ms 更新一次；`deadlineMs === null` 时改用快照里的 `remainingMs`
- [x] 状态来源只有 Rust 的 `timer://state` 事件，前端不自己维护状态机
- [x] 所有按钮调用 `invoke("timer_action", { action })`，`action ∈ { "pause" | "resume" | "restart" | "acknowledge" }`
- [x] 按钮点击后立即反映新状态（不等下一个状态事件）
- [x] 面板右上角 ✕ 只隐藏窗口，计时继续

## 背景

- 四态定义与按钮集见 [`spec.md` §4.1](../spec.md)
- 交互契约见 [`spec.md` §5.3](../spec.md)
- 前端是纯视图，节流无害——真值在 Rust，见 [ADR-0002](../../../../docs/adr/0002-authoritative-clock-in-rust.md)

## Comments

### 2026-10-23 — 四个面板的 UI 已经写完了（本票大部分已完成）

搭骨架（票 01）时顺手把四态面板和按钮接线一起做了，所以本票剩下的只有：

- [x] 面板关闭（系统关闭按钮 / `WindowEvent::CloseRequested`）只隐藏、不退出 —— **Rust 侧**，前端做不了
- [x] 圆角透明窗口（`macOSPrivateApi: true` + `transparent: true`）—— 从票 01 挪过来的，见该票 Comments
- [x] 端到端跑一遍：点每个按钮都能看到状态正确切换

前端这半边（`src/components/*.tsx` × 4、`useNow` 插值、`useTimerState` 订阅、✕ 走 `hidePanel()`）已经在 `fd0cf62` 里，`npm run typecheck` + `npm run build` 通过。**不要重写**，等 03 的时钟线程上线后端到端验证即可。

### 2026-10-24 — 本票收口

**新增 Rust 模块 `src-tauri/src/panel.rs`**：上半截是纯逻辑（关闭请求该怎么处理、显示 / 藏起之后可见性是什么），下半截才是 Tauri 接线——跟 `tray.rs` 一个路子。关键的一条：面板被要求关闭时**先 `api.prevent_close()` 再藏起来**。不拦的话窗口会被销毁，而面板是应用里唯一的窗口，Tauri 会因为"一个窗口都不剩"而退出应用，计时就断了。

`hide_panel` 也注册成了一条命令（`invoke("hide_panel")`），前端 ✕ 走的就是它。它跟 `timer_action` 分开，是因为它根本不碰状态机——只藏窗口。

**窗口配置**（`tauri.conf.json`）：`macOSPrivateApi: true` + `transparent: true` + `decorations: false` + `shadow: false`。原生投影是方的（macOS 按窗口矩形算），跟卡片的圆角对不上，所以关掉它，改由 CSS 画圆角投影。

#### 验证分三层，别当成"全都在真机上点过了"

1. **真机（屏幕没锁的时候）**：上一版构建上逐个点过四个按钮，状态切换都对；本版真机上确认了面板窗口是 320×380、默认藏着（`visible: false`）、工作时段到点自己弹出来。
2. **无头 IPC（`src-tauri/src/lib.rs` 的用例）**：`invoke("timer_action", { action })` 走真的 IPC 通道 → 真的命令 → 真的时钟线程 → 真的状态机 → 真的快照推给前端。用 Tauri 自带的假运行时（`tauri::test`），不需要屏幕，CI 里也能跑。四个动作各一个用例，外加"看不懂的动作什么也不做"和"藏面板不推快照"。
3. **前端（`src/__tests__/panel.test.tsx`，13 个用例）**：四个状态各自的渲染、倒计时的插值、按钮调的动作、✕ 调 `hide_panel`。

第 2、3 层是**同一个接缝的两半**：前端那半验"按钮发的动作对不对"，Rust 那半验"这个动作到得了状态机、并且新状态回到了前端"。中间没有断点。

#### 一处没做到的

最后一轮真机点击没能重跑：这台机器的屏幕当时锁着（`kCGSSessionScreenIsLocked = Yes`）。锁屏状态下 macOS 不派发合成点击事件，AX 也读不到面板窗口（`screencapture -l` 同样失败）——应用本身是好的（进程在、托盘在、面板窗口在、状态在推）。解锁后把第 1 层重跑一遍就算补齐。

#### 读法记录

票里"唯一的按钮""无任何按钮"说的是**动作按钮**；面板右上角的 ✕ 是窗口控件，不是动作按钮——它在四个状态里都在，点了只是藏窗口，不碰状态机。

#### 顺带

- 假分发器（记录副作用的那套）从 `timer/clock.rs` 的测试模块搬到了 `timer/mod.rs` 的 `test_support`——`lib.rs` 的 IPC 用例也要用它，住一份就够。
- `Cargo.toml` 加了 `[dev-dependencies] tauri = { version = "2", features = ["test"] }`：Cargo 的 resolver v2 不让 dev-dependency 的特性漏进正式构建（`cargo tree -e features -i tauri --edges normal,build` 里没有 `test`，release 二进制里也没有 `tauri::test` 的符号）。
