# FocusTimer

[![CI](https://github.com/hochoy18/timer-app/actions/workflows/ci.yml/badge.svg)](https://github.com/hochoy18/timer-app/actions/workflows/ci.yml)

常驻 macOS 菜单栏的专注计时器。

工作时段满 1 小时就提醒你休息；**不确认则每 10 分钟重复提醒一次**，直到你真的
离开工位。确认之后进 15 分钟休息时段，休息结束自动开始下一个工作时段。

只做 macOS。

## 它长什么样

- **菜单栏常驻**：图标 + 剩余时间（`60:00` 这样）。提醒态时图标换成醒目的那一张，时间前面缀一个红点。
- **点图标弹面板**：面板里是倒计时和按钮——暂停 / 继续 / 重新计时。工作时段、暂停、待确认提醒、休息时段四态各有各的样子。
- **面板不抢焦点**：到点提醒时它会弹出来置顶，但不会把你正在敲的窗口顶走。
- **到点有通知和系统音效**：提醒由 Rust 侧发出（不走 webview 的音频上下文），所以即使从来没跟它交互过也听得见。
- **首次运行有一次引导**：说清 60 / 15 / 10 三件事，顺手申请通知权限。之后不再出现。
- **重复双击只起一个实例**：后来的那个把面板叫出来，自己退出。

## 快速上手

### 1. 装工具链

这台机器的硬阻塞只有 Rust 和 Tauri CLI。仓库里带了个向导脚本，它会分阶段检查、
确认、安装并验证：

```bash
./scripts/setup-toolchain.sh
```

脚本做两件会改动系统的事：装 rustup 和全局 npm 包；往 `~/.zshrc` 追加一行
cargo 的 PATH（仅当尚未存在）。可重复运行。

手动等价命令见 [`spec.md` §7](.scratch/focus-timer/spec.md#7-环境准备)。

### 2. 开发跑

```bash
npm install
npm run tauri dev
```

想快进验证一整轮（60 分钟压成 36 秒），用开发期的加速开关：

```bash
FOCUS_TIMER_SCALE=0.01 npm run tauri dev
```

**这个开关只有 debug 构建读**——release 里那段代码根本没编进去，不是"看了但不用"。
所以打包版不可能因为谁启动时手滑带了个环境变量，就跑出 36 秒一轮的怪事。

### 3. 打包并装上

```bash
npm run tauri build      # → src-tauri/target/release/bundle/macos/FocusTimer.app
./scripts/install-app.sh # → 打包 → 拷到 /Applications → 清掉隔离属性
```

装好之后**第一次启动要在电脑前**：引导面板里点「启用通知」，macOS 会问一句
"FocusTimer 想要发送通知"——**点「允许」**。没人点的话十来秒后系统自己记成
"不允许"，之后所有提醒静默失败。

装 / Gatekeeper / 卸载 / 手工验一遍，全写在 [`docs/install.md`](docs/install.md)。

## 开发

| 命令 | 干什么 |
|---|---|
| `npm run tauri dev` | 起应用（前端 + Rust） |
| `npm run typecheck` | `tsc --noEmit` |
| `npm test` | 前端用例（21 个） |
| `cd src-tauri && cargo test` | Rust 用例（131 个） |
| `cd src-tauri && cargo test --release` | release 那遍——只有它跑得到 `#[cfg(not(debug_assertions))]` 里的用例 |
| `npm run tauri build` | 打包成 `.app` |
| `./scripts/check-app-bundle.sh` | 验打出来的东西（Info.plist、图标、可执行文件、签名、隔离属性） |
| `./scripts/install-app.sh` | 装进 `/Applications` |
| `swift scripts/list-windows.swift focus` | 列窗口。屏幕锁着的时候，截不了图也看不见 AX，但窗口**元数据**不需要权限——面板弹没弹，看 `onscreen` 就知道 |

### 目录

```
src/                  前端：纯视图。收状态事件、用 targetTime 本地插值渲染
  components/         面板四态 + 引导面板 + 面板头
  hooks/              useTimerState / useNow / useOnboarding
  lib/                actions（四个动作 → invoke）、format（倒计时格式化）
  __tests__/          前端用例
src-tauri/
  src/
    timer/state.rs    纯函数状态机（无 I/O、无时间依赖，now 作为参数传进去）
    timer/clock.rs    权威时钟线程：每 250ms 校验时间戳
    tray.rs           托盘：标题、菜单、图标状态
    panel.rs          面板窗口：显示 / 隐藏 / 关掉不退出
    notify.rs         系统通知 + 系统音效
    onboarding.rs     引导标记 + 通知权限申请
    single_instance.rs 单实例：后来的实例自己退出
    config.rs         时长常量 + 开发期加速开关
  tests/              进程级用例（单实例、加速开关、打包配置）
scripts/              工具链向导、打包校验、安装脚本、窗口探针
docs/adr/             架构决策
docs/install.md       打包与安装
```

## 这套东西是怎么搭的

- **一切以绝对时间戳为准**，不累加秒数（[ADR-0001](docs/adr/0001-timestamp-based-timing.md)）。
  合盖跨过工作终点，醒来立刻判定该在哪一态。
- **状态机与权威时钟都在 Rust 侧**（[ADR-0002](docs/adr/0002-authoritative-clock-in-rust.md)）。
  面板隐藏时 webview 会节流前端定时器，心跳必须活在 Rust 里。前端只负责画。
- **不持久化任何状态**（[ADR-0003](docs/adr/0003-no-state-persistence.md)）。磁盘上只有一个
  文件：引导标记——它只记"引导做过了"，不记任何计时进度。退出后重开，从头 60 分钟。
- **领域词汇**在 [`CONTEXT.md`](CONTEXT.md)：工作时段、休息时段、轮次、暂停、提醒、
  待确认提醒、重复提醒、确认、面板、引导。每个词都有"别这么叫"的清单。

## 实施票

实现方案在 [`spec.md`](.scratch/focus-timer/spec.md)，票在
[`.scratch/focus-timer/issues/`](.scratch/focus-timer/issues/)——10 张，全 `resolved`。
§9 的验收清单有 14 条，13 条有自动化兜着，哪条由谁验的写在票 10 里。

同目录下的 `conversation_1.md` / `conversation_2.md` 是这个项目开头那两轮设计
对话的原始记录——领域词汇和实现方案就是从那儿来的。

## CI

每次推到 `main` 都跑一遍：前端在 Ubuntu 上跑类型检查 + 用例，应用在
macOS 上跑 Rust 用例（debug 与 release 各一遍）、真机打包、验产物。

验的不只是"测试过没过"——打包那几步验的是**打出来的东西**：Info.plist 的键、
plist 指着的图标在不在包里、可执行文件是不是 Mach-O、有没有签名、有没有
隔离属性。单元用例验不到这些。

[`.github/workflows/ci.yml`](.github/workflows/ci.yml)。

## 本期不做

开机自启、时长可配置、计时状态持久化、跨平台、签名与公证、自动更新、历史统计。
原因逐条写在 [`spec.md` §2](.scratch/focus-timer/spec.md#2-范围)。

## 许可证

[MIT](LICENSE)。
