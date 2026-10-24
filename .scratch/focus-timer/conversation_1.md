这个需求的核心是一个**带状态流转、用户交互确认（Snooze/确认机制）与无限循环的有限状态机（FSM）**。

---

### 一、 核心状态机设计

定时逻辑不能单纯依赖 `sleep` 或单一延时计时，必须基于时间戳和状态转移来保证精准度及异常恢复（如休眠唤醒、进程重启）。

整个流转定义为 4 个状态：

```
[IDLE (未启动)]
       │
       ▼ (用户点击启动)
[WORKING (工作计时: 60分钟)]
       │
       ▼ (60分钟到期)
[WORK_ALERTING (休息提醒触发)] ──(无操作/未关闭)──> 延迟 10 分钟再次提醒 (循环)
       │
       ▼ (用户点击确认/开始休息)
[BREAKING (休息计时: 15分钟)]
       │
       ▼ (15分钟到期)
[BREAK_ALERTING (休息结束提醒)] ──> 自动流转 / 用户确认流转
       │
       ▼
回流至 [WORKING] (自动重置并开启下一轮 60分钟)

```

#### 状态转移明细表

| 当前状态 | 触发条件 | 目标状态 | 附带动作 |
| --- | --- | --- | --- |
| **WORKING** | 倒计时归零 (60m) | **WORK_ALERTING** | 触发系统弹窗/音频报警，启动 10 分钟重试计时器 |
| **WORK_ALERTING** | 超过 10 分钟未确认 | **WORK_ALERTING** | 再次发出强提醒（响铃/置顶弹窗），重置 10 分钟定时 |
| **WORK_ALERTING** | 用户点击“去休息” | **BREAKING** | 关闭报警与重试定时器，计算 `break_end = now + 15m` |
| **BREAKING** | 倒计时归零 (15m) | **WORKING** | 发出“休息结束，开始工作”通知，自动重置下一轮 60m 计时 |

---

### 二、 技术选型与平台建议

根据你的使用习惯和目标平台，有三种主流实现方式：

1. **轻量桌面客户端（推荐）：Tauri + React / Vue 或 Electron**
* **优势**：跨平台（macOS / Windows / Linux）、系统原生托盘常驻、本地系统通知（System Notification）与全局快捷键支持。
* **包体积**：Tauri（Rust 底层，打包后通常 < 15MB，内存占用低）。


2. **极简跨平台 Python 脚本 / 托盘：PyQt6 / pystray + plyer**
* **适合**：个人自用、无需复杂 UI、快速验证逻辑。


3. **Web 端（PWA）：HTML5 Notification API + Web Worker**
* **缺点**：浏览器标签页在后台休眠时定时器（`setTimeout`）可能会被降频节流，且无法像原生应用那样稳定锁屏提醒。



---

### 三、 核心逻辑与数据结构（TypeScript / 状态类参考）

为了避免系统休眠或前台被挂起导致计时漂移，**切勿使用累加计数器**（如 `secondsLeft--`），而应**记录目标截止时间戳（`targetTimestamp`）并在 Tick 中与系统当前时间做差值计算**。

```typescript
type AppState = 'IDLE' | 'WORKING' | 'WORK_ALERTING' | 'BREAKING';

interface TimerConfig {
  workDurationMs: number;    // 默认 60 * 60 * 1000 (1小时)
  breakDurationMs: number;   // 默认 15 * 60 * 1000 (15分钟)
  snoozeIntervalMs: number;  // 默认 10 * 60 * 1000 (10分钟)
}

class PomodoroFSM {
  private state: AppState = 'IDLE';
  private targetTime: number = 0;
  private snoozeTimer: NodeJS.Timeout | null = null;
  private ticker: NodeJS.Timeout | null = null;

  constructor(private config: TimerConfig) {}

  // 1. 启动工作计时
  public startWork() {
    this.clearAllTimers();
    this.state = 'WORKING';
    this.targetTime = Date.now() + this.config.workDurationMs;
    this.runTicker();
  }

  // 2. 状态主循环检查
  private tick() {
    const now = Date.now();
    const remaining = this.targetTime - now;

    if (this.state === 'WORKING' && remaining <= 0) {
      this.triggerWorkAlert();
    } else if (this.state === 'BREAKING' && remaining <= 0) {
      this.triggerBreakEnd();
    }
  }

  // 3. 触发工作结束提醒
  private triggerWorkAlert() {
    this.state = 'WORK_ALERTING';
    this.sendNotification("工作时间到", "已连续工作1小时，请休息15分钟！");
    this.playAlertSound();

    // 开启 10 分钟未确认重复提醒
    this.scheduleSnooze();
  }

  // 4. 未关闭提醒的重试逻辑 (10分钟)
  private scheduleSnooze() {
    if (this.snoozeTimer) clearTimeout(this.snoozeTimer);
    
    this.snoozeTimer = setTimeout(() => {
      if (this.state === 'WORK_ALERTING') {
        this.sendNotification("提醒：你还在工作！", "距离上次提醒已过10分钟，请立即休息。");
        this.playAlertSound();
        this.scheduleSnooze(); // 持续递归，直到用户确认
      }
    }, this.config.snoozeIntervalMs);
  }

  // 5. 用户响应提醒：确认休息
  public confirmBreak() {
    if (this.state !== 'WORK_ALERTING') return;

    if (this.snoozeTimer) {
      clearTimeout(this.snoozeTimer);
      this.snoozeTimer = null;
    }

    this.state = 'BREAKING';
    this.targetTime = Date.now() + this.config.breakDurationMs;
  }

  // 6. 休息结束，自动流转进下一轮工作
  private triggerBreakEnd() {
    this.sendNotification("休息结束", "15分钟休息已结束，开始新的1小时工作！");
    this.playAlertSound();
    
    // 自动重置并开启下一轮 1 小时
    this.startWork();
  }

  private runTicker() {
    if (this.ticker) clearInterval(this.ticker);
    this.ticker = setInterval(() => this.tick(), 1000);
  }

  private clearAllTimers() {
    if (this.ticker) clearInterval(this.ticker);
    if (this.snoozeTimer) clearTimeout(this.snoozeTimer);
  }

  // 平台相关实现：系统通知与音效
  private sendNotification(title: string, body: string) { /* Native API */ }
  private playAlertSound() { /* Audio API */ }
}

```

---

### 四、 关键边界问题与解决方案

1. **操作系统休眠与息屏（System Sleep / Nap）：**
* **问题**：如果电脑合盖或锁屏休眠，定时器线程会被挂起。
* **对策**：唤醒时比较 `Date.now()` 和 `targetTime`。如果发现唤醒时时间已经超过了目标时间，立即触发过期事件。


2. **强提醒机制（避免忽略系统通知）：**
* 很多系统自带通知很容易被通知中心静音或折叠。对于“10分钟未确认继续提醒”的需求，建议弹窗采用顶层模态窗口（Always on Top）或配合简短的高频提示音。


3. **状态持久化（Crash/重启安全）：**
* 将 `{ state, targetTime, lastAlertTime }` 实时写入本地（如 SQLite、LevelDB 或简单的 `localStorage`/JSON 文件）。应用意外闪退或重启后读取状态，若未超时则继续倒计时，若超时则直接进入 `WORK_ALERTING`。
