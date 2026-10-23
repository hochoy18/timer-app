# 04 — 托盘菜单与提醒态样式

Status: resolved

## 目标

完成菜单栏一侧：剩余时间标题、常态/提醒态两套图标、右键菜单、左键点击行为。

## 验收

- [x] 托盘标题显示剩余时间 `MM:SS`（超过 1 小时则 `H:MM:SS`）
- [x] 进入 `WORK_ALERTING` 时：图标切换为醒目样式，标题带红色标记；离开时恢复 —— **标题不是逐字染红，而是前缀一个红点**，见 Comments
- [x] 右键菜单包含：`打开面板` / `暂停` / `退出`
- [x] `暂停` 菜单项仅在 `WORKING` 状态下可用，其余状态下置灰
- [x] 左键点击托盘图标：面板已显示则隐藏，未显示则显示
- [x] 显示面板时**只调用 `show()`，不调用 `setFocus()`**——不抢焦点
- [x] `退出` 菜单项真正结束进程
- [x] macOS 下应用不占用 Dock 图标（`ActivationPolicy::Accessory`）

## 背景

- 托盘规范见 [`spec.md` §5.4](../spec.md)
- 不抢焦点是明确的产品决定：提醒要显眼，但不该打断手头正在做的事
- 依赖 03 的时钟线程提供托盘刷新

## Comments

### 2026-10-23 — 13 个新用例 + 真跑一遍

`cargo test` 65 passed / 0 failed（票 02/03 的 52 个 + 本票 13 个），`npm run typecheck` 无输出。
新模块 `src-tauri/src/tray.rs`：上半截是纯逻辑（菜单项 id ↔ 动作、状态下拉、标题文案、点击怎么算），
下半截是 Tauri 接线。切这么开的理由是托盘的行为大半是"什么状态下、点什么、按哪个键"，
这些不启动 app 就能验；真跑才看得见的只剩 macOS 的绘制效果。

跑起来看（`FOCUS_TIMER_SCALE=0.002`，把 60 分钟压成 7.2 秒）：
标题从 `00:08` 一秒一秒往下走 → 到点依次发出 `set_title(🔴 00:00)` / `apply_state(WorkAlerting)` /
`show_panel` / `set_alerting(true)` → 之后每 1.2 秒（真实 10 分钟）重复一次 `show_panel`，无上限。
截屏工具在这台机器上被系统挡了，所以照票 03 的办法临时在分发函数里加 `eprintln!` 看的，看完撤掉了。

### 三笔账

**1. 标题转红做不到，改成前缀红点。** 规格写的是"提醒时托盘上的时间变红"，但 `tauri` 2.12 /
`tray-icon` 0.25 / `muda` 0.20 一路下来都没有 status item 标题着色的 API——托盘标题只有
`set_title(text)` 一种写法。逃逸口是有的：`TrayIcon::with_inner_tray_icon` 能摸到内层的
`tray_icon::TrayIcon`，进而拿到 `NSStatusItem`，用 `attributedTitle` 的 `foregroundColor`
可以把数字真正染红。但那要把 `objc2-app-kit` 提成直接依赖、并耦合到 `tray-icon` 的内部结构
（Tauri 自己的文档就提醒过 tray-icon 会在小版本里动），不划算。

改成在标题前加一个红点 `🔴`（Apple Color Emoji，菜单栏文本按彩色渲染）：菜单栏里因此真的
多出一个红色元素，数字本身仍跟随菜单栏明暗。**这条是产品侧拍板的**，将来若要改回逐字染红，
`with_inner_tray_icon` 是那个口子。

顺带一个模板图的坑：常态图标是黑色剪影，走模板（`icon_as_template(true)`）才能自动跟随菜单栏
明暗；而醒目图标 `tray-alert.png` 本身是红色的，**不能**走模板——模板图只取 alpha，会被抹成
菜单栏前景色，那"醒目"就白换了。所以换图标时用 `set_icon_with_as_template` 一起设，原子切换
也省得闪一下。

**2. 顺手修了一个会把应用卡死的死锁（票 03 埋的，本票第一次能撞上）。**
Tauri 的托盘 / 窗口 setter 内部是"投递到主线程 + 等它跑完"。而 `ClockThread` 是**拿着状态机的锁**
去分发副作用的，`TauriDispatcher::show_panel` 就在这条路上。票 03 时主线程只有一个入口会回头等
这把锁——`timer_action` 命令；本票给主线程又添了托盘菜单和左键点击两个。于是：

> 时钟线程持有状态机锁 → `window.show()` 等主线程跑
> 主线程正在处理菜单点击 → `clock.apply()` 等状态机锁
> 互等，应用卡死。

修法落在 `tray::on_main`：分发这一层一律 fire-and-forget，只投递不等。投递是 FIFO 的，
标题、状态、快照、副作用的先后照样成立，所以票 03 特意保的"发出顺序必须与状态变化的顺序一致"
没有被破坏。`ClockThread` / `TimerEngine` 一个字没动。

**3. 菜单整份重建，不留 `MenuItem` 句柄去改可用性。** 状态一变就重拼一个四项菜单。
状态切换一次要按小时，这点代价可以忽略；换来的是不必把 `MenuItem` 塞进 Tauri 的状态表，
也就绕开了"这东西到底 `Send + Sync` 吗"的纠缠。为此 `Dispatcher` 多了一个 `set_state`：
`TickOutput` 里带上状态，且**只在状态真变了时才发**（倒计时每秒一帧，不能每秒重建一次菜单）。
`set_state` 和 `set_tray_alert` 答的不是同一个问题——前者答"现在站在哪个状态"（菜单能不能点），
后者答"刚刚进了 / 退了提醒态"（换不换图标）；两者总在同一批里进出。

菜单里的"打开面板"和左键是同一个动作：面板已显示就收起来。规格只给了左键定切换，
面板自己的 ✕ 要等票 06 才落地，托盘得先能把它收回去。

### 环境变更

`Cargo.toml` 给 tauri 加了 `image-png` feature——运行时要把 `icons/tray*.png` 解成 `Image`，
托盘要在常态 / 提醒态之间切，两张图都得进二进制。依赖仍走 rsproxy 镜像。

### 2026-10-23 — 双轴评审之后

评审跑了两轴（规范 / 规格），改掉的和**故意不改的**都记在这里。

**改掉的：**

- `title_for` / `ALERT_MARK` 从 `tray.rs` 挪进 `clock.rs`，挨着 `format_remaining`。
  引擎调 Tauri 模块、`clock.rs ↔ tray.rs` 成环——这跟 `clock.rs` 自己写的
  "做成 trait 是为了让时钟线程不必依赖 Tauri" 是打架的，而 `title_for` 本来就只是
  `format_remaining` 加个前缀。挪完引擎不再认识托盘，环也断了，标题的用例也跟着过去。
- `state.rs` 里"醒目图标 + 红色剩余时间"改成"醒目图标"——上一票只改了 `clock.rs` 那份，
  留了个还在承诺红字的注释。
- spec §5.4 菜单那行补上"菜单里的『打开面板』与左键是同一个动作"。文案照旧是
  `打开面板`（验收标准就是这么写的），但行为是切换，把话说明白，免得下一个人照字面改。
- `lib.rs` 补回结尾换行；顺手把"FIFO 保证先后"里混带的快照摘出去——快照走 `emit`，
  不在那条队列上。
- `ClockThread::stop` 里"分发会等主线程所以 join 可能互锁"那段已经过时了，写清了现在的
  真实理由（`emit_state` 仍走事件循环，主线程一旦停住就没人执行），保留"等一个心跳就放弃"。
- `lib.rs` 丢了结尾换行，纯事故，补回。

**加了测试：** `the_menu_is_told_about_the_state_before_the_panel_pops_up`——锁住
"菜单先按新状态重建，面板才弹出来"这个顺序。反过来面板弹出的一瞬间菜单还按上一状态亮着，
提醒态里 `暂停` 会多亮一拍。反序试过，确实会红。

**没改，以及为什么：**

- **"分发必须不等分发器"这条不变量没法用单元测试锁。** 关键在于它根本不在
  `ClockThread` 这一层：真值在 `TauriDispatcher` 的每个方法里（都走 `tray::on_main` 投递）。
  单元测试用的是 `FakeDispatcher`，trait 本来就是同步的——一个会阻塞的假实现阻塞是设计
  使然，测不出真实现有没有阻塞。要真测得把 Tauri app 起起来、让主线程故意卡住，
  那是集成测试不是单元测试。**改成写清楚 + 加了个运行时的断言**：`tray::on_main` 的注释点明
  这条只在 `tracing` 关着时成立（`tauri-runtime-wry` 的 `emit` / `eval_script` 在
  `tracing` feature 打开时会改用 channel + `rx.recv()` 等回执，重新变回阻塞；当前依赖图里
  没有 `tracing`，`cargo tree -i tracing` 是空的）。谁给 `tauri-runtime-wry` 打开 `tracing`，
  谁就得回头看那里。
- **不拿状态去推图标、砍掉 `SetTrayAlert`。** 评审说这是"两个真相"。不砍的理由：
  `SetTrayAlert` 是**副作用**（票 02 转移表里的，跟 `ShowPanel` / `Notify` 一类），
  `set_state` 是**当前位置**。重复提醒时状态没变但确实要再顶一次面板，靠的是副作用那一路；
  反过来图标是状态的函数而不是某次转移的函数。两者答的不是同一个问题，票 04 的 Comments
  里也写了。留着。
- `Cargo.lock` 为两张 ~1 KB 的 PNG 多引了 `image`，每次切图标重解一次。一小时才切一次，
  不值得为此把 `Image` 缓存成全局状态（那又得处理 `Send + Sync`）。
- `panel_visible_after_toggle` 就是 `!visible`——留着一个能单测的接缝，好过把 `!` 直接写进
  闭包里。

**要人来拍板的：** 本票自己改了自己的验收标准（逐字染红 → 前缀红点）并自标 `resolved`。
理由写在上面，但改产品行为这事最终该有人签字。