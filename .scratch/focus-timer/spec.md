# Focus Timer — 实现方案

> 领域词汇：[`CONTEXT.md`](../../CONTEXT.md) · 架构决策：[`docs/adr/`](../../docs/adr/) · 实施票：[`issues/`](./issues/)

## 1. 目标

一个常驻 macOS 菜单栏的专注计时工具。启动即开始一个 **60 分钟工作时段**；到点提醒休息；**不确认则每 10 分钟重复提醒**；确认后进入 **15 分钟休息时段**；休息结束自动开启下一轮工作时段。

## 2. 范围

### 本期做

- 常驻菜单栏（托盘），面板默认隐藏、点托盘图标或菜单唤出
- 托盘实时显示剩余时间；提醒态下图标醒目 + 时间变红
- 工作时段 / 休息时段 / 暂停 / 待确认提醒 四态面板
- 系统通知 + 系统音效（Rust 侧发出）
- 首次运行一次性引导 + 通知权限申请
- 单实例
- 打包成可安装的 `.app`

### 本期不做

| 不做 | 原因 |
|---|---|
| 开机自启 | 决策：暂不做，日后手动加登录项 |
| 时长可配置 | 决策：先写死 60 / 15 / 10，改代码即可 |
| 计时状态持久化 | [ADR-0003](../../docs/adr/0003-no-state-persistence.md)（例外有三个，都不记计时进度：引导标记，见票 07；单实例的 socket 和名分锁，见票 08） |
| 跨平台 | 决策：仅 macOS |
| 历史统计 / 番茄钟风格计数 | 不在本工具职责内 |

## 3. 领域模型

见 [`CONTEXT.md`](../../CONTEXT.md)。核心词汇：**工作时段**、**休息时段**、**轮次**、**暂停**、**提醒**、**待确认提醒**、**重复提醒**、**确认**、**面板**、**引导**。

## 4. 状态机

### 4.1 状态

| 状态 | 含义 | 面板内容 | 面板按钮 |
|---|---|---|---|
| `WORKING` | 工作时段进行中 | 剩余倒计时 + "专注工作中" | 暂停、重新计时 |
| `PAUSED` | 工作时段被暂停 | 冻结的剩余时间 + "已暂停" | 继续、重新计时 |
| `WORK_ALERTING` | 待确认提醒 | "工作已满 1 小时" + "不确认则每 10 分钟重复提醒" | 马上去休息 15 分钟 |
| `BREAKING` | 休息时段进行中 | 剩余倒计时 + "休息中" | 无 |

应用启动即进入 `WORKING`，不存在可停留的空闲态。

### 4.2 转移表

| 当前状态 | 事件 | 下一状态 | 副作用 |
|---|---|---|---|
| —（启动） | `Start` | `WORKING` | `target = now + 60m`；托盘显示 `60:00`；首次运行显示引导 |
| `WORKING` | `Pause` | `PAUSED` | 冻结 `remaining = target - now` |
| `PAUSED` | `Resume` | `WORKING` | `target = now + remaining` |
| `WORKING` | `WorkElapsed` | `WORK_ALERTING` | 通知 + 音效；面板弹出置顶（**不抢焦点**）；托盘转红 + 换图标；`nextRepeat = now + 10m` |
| `WORK_ALERTING` | `Acknowledge` | `BREAKING` | 停止重复提醒；`target = now + 15m`；托盘恢复正常样式；面板显示休息倒计时 |
| `WORK_ALERTING` | `AlertRepeat` | `WORK_ALERTING` | 再通知 + 再音效；面板再次置顶；`nextRepeat = now + 10m` |
| `BREAKING` | `BreakElapsed` | `WORKING` | 通知 + 音效（"休息结束，开始下一轮"）；`target = now + 60m`；托盘恢复正常样式 |

### 4.3 时间推进规则

- 一切以**绝对时间戳**为准，不累加秒数（[ADR-0001](../../docs/adr/0001-timestamp-based-timing.md)）
- `WORK_ALERTING` 的重复间隔从**上一次提醒发出的时刻**起算 10 分钟，无次数上限
- 系统休眠期间计时照常推进；唤醒后按时间戳判定应处于哪个状态（跨过工作终点 → 立即提醒；跨过休息终点 → 直接进入新工作时段）
- `PAUSED` 期间不产生任何提醒

## 5. 架构

### 5.1 分层

```
┌─────────────────────────────────────────┐
│  Rust 侧（权威）                         │
│  · TimerCore：纯函数状态机（无 I/O、无时间依赖，now 作为参数传入） │
│  · ClockThread：每 250ms 校验时间戳      │
│  · Tray：标题每秒刷新、菜单、图标状态     │
│  · Notify / Sound：系统通知 + 系统音效    │
└──────────────┬──────────────────────────┘
               │ Tauri events（仅状态变化时）
┌──────────────▼──────────────────────────┐
│  前端（纯视图）                           │
│  · 收状态事件，用 targetTime 本地插值渲染  │
│  · 按钮点击 → invoke 动作给 Rust          │
└─────────────────────────────────────────┘
```

**为什么这样分**：面板隐藏时 WKWebView 会节流前端定时器，心跳必须活在 Rust 里（[ADR-0002](../../docs/adr/0002-authoritative-clock-in-rust.md)）。前端只负责画，且节流无害。

### 5.2 目录

```
timer-app/
├── package.json / vite.config.ts / tsconfig.json / index.html
├── vitest.config.ts              # 前端测试（jsdom + testing-library）
├── src/                          # 前端：纯视图
│   ├── main.tsx
│   ├── App.tsx
│   ├── types.ts
│   ├── components/
│   │   ├── WorkPanel.tsx
│   │   ├── PausedPanel.tsx
│   │   ├── AlertPanel.tsx
│   │   ├── BreakPanel.tsx
│   │   ├── PanelHeader.tsx       # 面板右上角的 ✕（票 07 抽出来共用）
│   │   └── OnboardingPanel.tsx   # 票 07 落地
│   ├── hooks/
│   │   ├── useTimerState.ts      # 订阅 timer://state
│   │   ├── useNow.ts             # 250ms 的本地时钟
│   │   └── useOnboarding.ts      # 引导状态 + 申请权限（票 07）
│   ├── lib/
│   │   ├── actions.ts            # 四个动作 → invoke("timer_action")
│   │   └── format.ts             # MM:SS / H:MM:SS
│   ├── __tests__/panel.test.tsx  # 面板四态 + 按钮（票 06）
│   ├── __tests__/onboarding.test.tsx  # 引导 + 权限申请（票 07）
│   ├── test/setup.ts
│   ├── test/harness.tsx          # 前端用例的公共脚手架
│   └── styles.css
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── capabilities/default.json
│   ├── icons/
│   ├── tests/
│   │   └── single_instance.rs    # 进程级的单实例用例（票 08）
│   └── src/
│       ├── main.rs
│       ├── lib.rs                # 命令 + Tauri 接线 + IPC 层的端到端用例
│       ├── timer/
│       │   ├── mod.rs            # 公共脚手架（测试用）
│       │   ├── state.rs          # 纯函数状态机 + cargo test
│       │   └── clock.rs          # 权威时钟线程
│       ├── tray.rs
│       ├── panel.rs              # 面板窗口：显示 / 隐藏 / 关掉不退出
│       ├── onboarding.rs         # 引导标记 + 通知权限申请（票 07）
│       ├── single_instance.rs    # 单实例：后来的实例自己退出（票 08）
│       ├── notify.rs             # 系统通知 + 系统音效
│       └── config.rs             # 时长常量 + dev 加速开关
├── CONTEXT.md
├── docs/
│   ├── adr/
│   └── agents/
└── .scratch/focus-timer/
```

### 5.3 进程间通信

**Rust → 前端**（event `timer://state`，**状态或显示秒数变化时**触发；倒计时中即每秒一帧）：

```ts
type TimerState = 'working' | 'paused' | 'workAlerting' | 'breaking'
interface TimerSnapshot {
  state: TimerState
  deadlineMs: number | null  // 当前阶段到期的绝对时刻（epoch ms）；无倒计时时为 null
  remainingMs: number        // 仅 paused 有意义：冻结的剩余毫秒数
}
```

字段拆开而不是合成一个 `targetMs`，是因为暂停态的"目标"和计时态的"目标"含义不同——一个是绝对时刻，一个是剩余时长，混在一个数字里前端每次都要猜。用 `null` 而不是 `-1` 表示"没有倒计时"，是为了让"无倒计时"和"倒计时是 0"在类型上就分得开。`alerting` 不传：它是 `state === 'workAlerting'` 的推论，传冗余字段等于给两个真相。

**每秒一帧而不是仅状态变化**：前端用自己的 `Date.now()` 算剩余，Rust 用 `SystemTime::now()`，两个时钟可能差几秒。每秒推一帧等于持续对时，前端每帧都能把显示拉回 Rust 的真值；改成仅状态变化就没有任何纠偏机制了，那点偏差会一直留在面板上。代价只是每秒一条 IPC 消息。

**前端 → Rust**（四条命令）：

- `invoke("timer_action", { action })`，`action ∈ { "pause" | "resume" | "restart" | "acknowledge" }`——面板上的四个动作按钮。
- `invoke("hide_panel")`——面板右上角的 ✕。**只隐藏窗口，不退出应用**：计时在时钟线程里继续走，托盘还在（CONTEXT.md 的"面板"）。这条跟 `timer_action` 分开，是因为它根本不碰状态机。
- `invoke("onboarding_status")`——问 Rust：引导还没做完吗？引导是 Rust 说了算的（它看着那个标记文件），前端只负责照着画。
- `invoke("onboarding_complete")`——告诉 Rust：引导做完了（权限申请也做了）。Rust 记下引导标记，顺手递一条**探测通知**（见 §5.5）。

窗口右上角的 ✕ 和系统级的关闭（⌘W、窗口的关闭按钮）走同一条路：拦住关闭、改为隐藏。面板是应用里唯一的窗口，放任它被关掉的话，Tauri 会因为"一个窗口都不剩"而退出应用，计时就断了。

前端每 250ms 用本地 `setInterval` 做插值渲染：`remaining = max(0, deadlineMs - Date.now())`。`deadlineMs` 为 `null`（提醒态）或暂停态时改用快照里的 `remainingMs`。

### 5.4 托盘

- **标题**：剩余时间 `MM:SS`，超过 1 小时则 `H:MM:SS`（`WORK_ALERTING` 时前缀一个红点 `🔴`）
- **图标**：常态图标（模板图，跟随菜单栏明暗）；`WORK_ALERTING` 时换醒目图标（彩色，不走模板）
- **菜单**：`打开面板` / `暂停`（仅 `WORKING` 可用）/ `退出`。菜单里的"打开面板"与左键点击是同一个动作——面板已显示就收起来
- **左键点击**：面板已显示则隐藏，未显示则显示（`show()` 但**不** `setFocus()`，不抢焦点）

托盘标题与面板显示的剩余时间是同一个数字，因此 `format_remaining`（Rust）和 `formatDuration`（前端）必须遵守同一条规则：`≤ 60 分钟 → MM:SS`（60 分钟写作 `60:00`），`> 60 分钟 → H:MM:SS`。两份格式化器，改一处就要改另一处。

提醒态的"红"用前缀红点而不是把数字染红：Tauri 2 这条路走不通，`tauri` / `tray-icon` / `muda` 一路下来都没有 status item 标题着色的 API。逐字染红要经 `TrayIcon::with_inner_tray_icon` 摸进 `NSStatusItem`，代价是把 `objc2-app-kit` 提成直接依赖并耦合 `tray-icon` 的内部结构。见 [`issues/04`](./issues/04-tray-menu-and-alert-style.md) 的 Comments。

### 5.5 提醒

| 时机 | 通知 | 音效 | 面板 |
|---|---|---|---|
| 工作时段结束 | ✅ | ✅ | 弹出置顶，不抢焦点 |
| 10 分钟未确认 | ✅ | ✅ | 再次置顶 |
| 确认休息 | — | — | 切到休息倒计时 |
| 休息时段结束 | ✅ | ✅ | 切回工作倒计时 |

音效走 **Rust 侧 macOS 系统音效**（`AudioServicesPlaySystemSound`），不依赖 webview 音频上下文——避免"从未交互过 → AudioContext suspended → 一声不吭"。

**还有一条不是提醒的**：首次运行完成引导时（票 07），Rust 会递一条**探测通知**。它不重复、不承担任何提醒职责——它是把 macOS 那句「想要发送通知」逼出来的手段：插件在桌面上不会自己弹权限框，macOS 只在 app **第一次投递通知**时问一句。已授权的话，这条探测通知就当"提醒已开启"的确认。

### 5.6 配置

| 常量 | 值 | 说明 |
|---|---|---|
| `WORK_DURATION` | 60 分钟 | 工作时段 |
| `BREAK_DURATION` | 15 分钟 | 休息时段 |
| `REPEAT_INTERVAL` | 10 分钟 | 重复提醒间隔 |

时长写死。开发期加速开关：环境变量 `FOCUS_TIMER_SCALE`（倍数），**仅 debug 构建读取**，release 构建忽略。

## 6. 关键决策

| ADR | 决策 |
|---|---|
| [0001](../../docs/adr/0001-timestamp-based-timing.md) | 用绝对时间戳推进计时 |
| [0002](../../docs/adr/0002-authoritative-clock-in-rust.md) | 状态机与权威时钟放在 Rust 侧 |
| [0003](../../docs/adr/0003-no-state-persistence.md) | 不持久化任何状态 |

## 7. 环境准备

当前机器**缺 Rust 和 Tauri CLI**，是唯一的硬阻塞。

**推荐做法**：跑仓库里自带的向导脚本，它会分阶段检查、确认、安装并验证：

```bash
./scripts/setup-toolchain.sh
```

脚本做两件会改动系统的事：安装 rustup 与全局 npm 包；向 `~/.zshrc` 追加一行 cargo 的 PATH（仅当尚未存在）。可重复运行。

**手动等价命令**：

```bash
# 1. Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal
# 重开终端使 ~/.cargo/bin 进入 PATH

# 2. Tauri CLI
npm i -g @tauri-apps/cli
```

已就绪：Node v26.10.0、npm 11.7.0、macOS 26.3.2 (arm64 / Apple M1)、Xcode CLT 26.3（clang 17 + MacOSX26.2.sdk，macOS 桌面构建**不需要**完整 Xcode）。

## 8. 里程碑

| M | 内容 | 票 |
|---|---|---|
| M1 | 工程骨架 | 01 |
| M2 | Rust 计时核心（状态机 + 时钟） | 02, 03 |
| M3 | 托盘与提醒 | 04, 05 |
| M4 | 面板 UI 与引导 | 06, 07 |
| M5 | 收尾 | 08, 09, 10 |

## 9. 验收标准

- [ ] 启动应用 → 托盘出现，标题显示约 `60:00`，面板不弹出
- [ ] 工作时段到点 → 通知 + 音效 + 面板弹出置顶且**不抢焦点**；托盘转红换图标
- [ ] 10 分钟不确认 → 再来一次通知 + 音效 + 面板再次置顶
- [ ] 点"马上去休息 15 分钟" → 面板显示 `15:00` 倒计时；重复提醒停止；托盘恢复正常样式
- [ ] 休息结束 → 通知 + 音效 + 自动回到工作时段 `60:00`
- [ ] 工作时段按暂停 → 时间冻结；按继续 → 接着走剩余
- [ ] 按"重新计时" → 丢弃剩余，重开 `60:00`
- [ ] 休息时段中无任何按钮
- [ ] 点面板 ✕ → 面板隐藏，计时继续，托盘仍在
- [ ] 首次启动 → 面板里出现一次性引导，说明 60/15/10 三件事；点"启用通知" → 申请通知权限、记下引导标记；之后启动不再出现引导
- [ ] 重复双击 `.app` → 不起第二个实例，只激活面板
- [ ] 合盖跨过工作终点 → 醒来立即收到提醒
- [ ] 退出后重新启动 → 从头开始新的 60 分钟（无残留）
- [ ] `cargo test` 全绿，覆盖 §4.2 每一行转移
