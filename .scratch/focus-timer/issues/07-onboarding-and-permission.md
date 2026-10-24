# 07 — 首次运行引导与通知权限申请

Status: resolved

## 目标

首次运行时展示一次性引导面板，用它完成 macOS 通知权限申请；之后不再出现。

## 验收

- [x] 首次启动展示引导面板：用两三句话说明这个 app 做什么（60 分钟工作 / 15 分钟休息 / 每 10 分钟重复提醒），并有一个 `启用通知` 按钮
- [x] 点 `启用通知` 才触发系统通知权限弹窗（`requestPermission`）—— **桌面上插件的 `requestPermission()` 恒回 `granted`、自己不弹框；真正逼出 macOS 那句「想要发送通知」的是 Rust 侧那条探测通知**，见 Comments
- [x] 用户允许或拒绝后引导面板都关闭，不再阻塞使用
- [x] 引导**只在首次运行出现**。用一个持久化标记（如 `~/Library/Application Support/<identifier>/.onboarded`）记录已引导过
  - 注意：此标记只记录"引导与权限申请已完成"，**不记录任何计时进度**（见 [ADR-0003](../../../../docs/adr/0003-no-state-persistence.md)）
- [x] 非首次启动时不展示引导，直接进入正常的工作时段
- [x] 权限被拒时不反复弹窗骚扰

## 背景

- 为什么要引导而不是直接弹系统权限框：macOS 通知权限只弹一次，用户当时没留意就永久变成"拒绝"，之后所有 `sendNotification` 静默失败。如果等到 1 小时后第一次提醒才申请，而人恰好不在，这个 app 就永远静默了
- 引导与权限申请必须在**应用启动时**完成，而不是等到第一次提醒
- 相关词汇见 [`CONTEXT.md`](../../../../CONTEXT.md) 的**引导**

## Comments

### 2026-10-24 — 落地了：Rust 一个模块 + 前端三件小事

**Rust 侧新增 `src-tauri/src/onboarding.rs`**（跟 `tray.rs` / `notify.rs` 一个路子：上半截纯逻辑，下半截 Tauri 接线）：

- `Onboarding` 抱着标记文件的位置和一个"还没做完吗"的答案；
- `onboarding_status` 回前端一个 `pending`；
- `onboarding_complete` 记下标记，**并递出一条探测通知**（下面说为什么这是关键的一步）；
- `setup()` 里建好它，`first_run` 为真时把面板弹出来。

**标记文件**：`app_data_dir()` 在 macOS 上就是 `~/Library/Application Support/com.focustimer.app`，所以落在 `.../com.focustimer.app/.onboarded`——跟票里写的一模一样。文件是**空的**：它的存在本身就是全部内容。用例 `the_marker_records_nothing_but_its_own_existence` 把它锁住了，哪天有人往里塞计时进度，这条就会红（ADR-0003）。

**前端**：`useOnboarding` 问一次 Rust 要 `pending`，`OnboardingPanel` 画那三句话 + 一个按钮，`App.tsx` 按 `pending` 在"引导"和"计时面板"之间选。引导占的就是面板那个窗口——面板是应用里唯一的窗口（CONTEXT.md），没有第二个窗口好开。

### 关键的一条：插件的 `requestPermission()` 在 macOS 上**不会**弹框

这是本票最该记下来的一件事，查了三层：

1. `tauri-plugin-notification` 的 `init-iife.js`（插件在 `js_init_script` 里注入，桌面也注入）把 `window.Notification` **整个换掉**了，换成一个壳；
2. 壳的 `requestPermission()` 走 `invoke("plugin:notification|request_permission")`；
3. 那条命令在桌面上的实现是 `Ok(PermissionState::Granted)`——插件的 `desktop::request_permission` 就这么写的，注释是"Desktop applications do not need to ask for this permission"。

也就是说：**前端那句 `requestPermission()` 在 macOS 上恒回 `granted`，既不弹框，连"拒绝"都轮不到用户点。**（顺带查过：WKWebView 裸的 `window.Notification.requestPermission()` 也不弹框、当场回 `denied`——但插件把 `window.Notification` 换掉了，所以真机上走不到那条路。）

真正把 macOS 那句「FocusTimer 想要发送通知」逼出来的是**投递通知**本身：插件在 macOS 上走 `notify-rust` → `mac-notification-sys` → `NSUserNotificationCenter deliverNotification:`，未授权的 app 一投递就被系统拦下来问一句。

所以点 `启用通知` 时做了两件事：

- 前端调插件的 `requestPermission()`（票 07 点名要这一步；哪天插件在桌面上真做了授权，它就是对的。抛错了也不挡路——引导不能因为一个弹不出来的框把人堵在这儿）；
- Rust 侧 `onboarding_complete` 顺手递一条**探测通知**。没授权 → 系统弹窗；已授权 → 当成"提醒已开启"的确认，正好让用户看见提醒长什么样。

探测通知走的是提醒本身那条路（同一个 `Notifier`），所以提醒能响，探测就能响。

### 引导的 ✕ 与标记的语义

引导面板右上角那个 ✕ 只藏窗口，**不写标记**。下次启动引导会再来一次。

这是故意的：标记记的是"引导**与权限申请**已完成"（ADR-0003 的措辞）。用 ✕ 划走了，权限申请就还没做——macOS 那句提示是一锤子买卖，错过了不会再来第二次。要是 ✕ 也写标记，用户就此永久静默，正是票里"背景"要防的那个结局。

点击 `启用通知` 之后（无论允许还是拒绝）标记一定写下，所以**权限被拒不反复弹窗**成立：拒绝之后引导不再出现。

另一处边界：标记写不下去时（磁盘满、目录没权限），Rust 会如实说"没做完"，前端照旧把引导关掉——不能因为一个标记文件把人堵在面板里——但会在控制台留一句警告，下次启动还会再引导一次。

### 标记之外，磁盘上什么都没有

ADR-0003 说"项目里不存在状态持久化代码"，这票给它划了唯一的例外。落盘的只有那一个**空文件**，没有工作时段、没有剩余时间、没有暂停状态、没有待确认提醒——全仓搜下来，前端也没用 localStorage。ADR-0003 自己补了"唯一的例外：引导标记"这一节，顺手把 spec §2 那一行也改了（"状态持久化" → "计时状态持久化"）。

### 验证分三层

1. **真机（屏幕没锁的时候）**：这一层**没跑成**——这台机器的屏幕当时锁着，合成点击 / AX / 截图全失效（应用本身是好的：进程在、托盘在、面板窗口在）。解锁后补一遍：首次启动应该弹引导 → 点"启用通知" → macOS 问"想要发送通知" → 允许 → 托盘开始走 60:00；再退出重开，引导不再出现。
2. **无头 IPC（`src-tauri/src/lib.rs`）**：6 个用例走真的 IPC 通道——第一次运行要引导、`onboarding_complete` 把标记写下去、标记是空的、做完之后不再引导、第二次启动不再引导（模拟下一次启动）、做完引导不打扰计时（点 pause 真的能暂停）。用 Tauri 的假运行时，不需要屏幕。
3. **前端（`src/__tests__/onboarding.test.tsx`，8 个用例）**：三句话 + 按钮在、第二次启动不出现、✕ 只调 `hide_panel`、点了按钮会去申请权限并告诉 Rust 做完了、允许 / 拒绝 / 申请炸了三种结果引导都关掉、申请期间按钮灰着。

第 2、3 层是**同一个接缝的两半**：前端那半验"按钮发的动作对不对"，Rust 那半验"这个动作到得了状态机、并且新状态回到了前端"。

### 双轴评审之后改了什么

`/code-review` 跑了两条轴（standards / spec），各挑出几处，都已经改掉：

- **注释跟事实相反**（最要紧的一处）：原来写的是"插件的 `requestPermission()` 会当场回 `denied`"——那是 WKWebView 裸 API 的行为，插件把 `window.Notification` 换掉之后真机走不到那条路。已按上面查到的三层改成"恒回 `granted`、自己不弹框"。
- **词汇漂移**：探测通知的正文原来写"会**通知**你休息"，CONTEXT.md 明说「提醒」_Avoid_: 通知。已改成"会提醒你休息"。
- **前端越过 Rust 宣布真相**：`completeOnboarding()` 返回的布尔原来被丢掉，前端一律 `setPending(false)`。现在用它说话——标记没写下去时前端会 warn 一声，而不是假装做完（Rust 的契约是"写不下去就算没做完"）。
- **面板头抄了两遍**：引导和计时面板各一份 ✕，抽成了 `src/components/PanelHeader.tsx`。
- **用例脚手架抄了两遍**：`renderApp()` 挪进了 `src/test/harness.tsx`。
- **`is_onboarded()` / `marker()` 只有测试在用**：`is_onboarded` 接回了 `Onboarding::new`；`marker()` 删了。

评审还提了两条，**没有**改，理由记在这里：

- "`notify.rs` 的 `try_state` 守卫是票 05 的改动，属于越界"——不是。没有它，票 07 的 IPC 用例会 panic：假运行时没装通知插件，而 `app.notification()` 在插件状态不在时是 panic。守卫是这条用例跑得起来的前提。
- "引导占住整个面板，用户看不到倒计时、点不到暂停"——是刻意的。引导就是要占住面板说那三句话；计时在 Rust 那边照跑，点 `启用通知` 之后引导就散了，计时面板立刻接上（用例 `completing_the_onboarding_does_not_disturb_the_timer` 盯着这件事）。

### 顺带

- `src/test/harness.tsx` 是新的公共脚手架：`captureStateEvent()`（往 `timer://state` 事件里灌快照）、`stubInvoke()`（给 `invoke` 一副能用的默认回答）、`renderPanel()` / `renderApp()`。前端的用例从现在起都走它——"状态只认 Rust 推的快照"这条规矩，用例也该守。
- `notify.rs` 的 `TauriSink::show` 加了插件状态的守卫：插件没装上时降级成"递不出去"，不 panic。
- spec 跟着更新了：§5.2 的目录树补了本票新增的文件、§5.3 的"两条命令"改成四条（多 `onboarding_status` / `onboarding_complete`）、§5.5 补了那条**探测通知**（它不是提醒，所以单起一段说明，没混进提醒表）、§9 加了一条引导的验收。
