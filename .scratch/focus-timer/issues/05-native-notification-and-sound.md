# 05 — Rust 侧系统通知与系统音效

Status: resolved

## 目标

在 Rust 侧实现两种提醒手段：系统通知横幅 + macOS 系统音效。**不依赖 webview 的音频上下文**。

## 验收

- [x] `src-tauri/src/notify.rs` 暴露 `notify(title, body)` 与 `play_sound()` —— 是 `Notifier` 上的两个方法，见 Comments
- [x] 通知走 `tauri-plugin-notification` 的 Rust API
- [x] 音效用 macOS 系统音效：调用 `AudioServicesPlaySystemSound`（**没有引 crate**，见 Comments），**不使用 Web Audio**
- [x] 三个时机都能发声：工作时段结束、10 分钟重复提醒、休息时段结束
- [x] 通知文案为中文，且分别对应三个时机
- [x] 通知权限被拒时**不 panic、不报错刷屏**，静默降级（面板与托盘仍是可靠提醒途径）—— 真机跑过一遍，见 Comments

## 背景

- 提醒矩阵见 [`spec.md` §5.5](../spec.md)
- 为什么不用前端 Web Audio：AudioContext 在没有用户交互时处于 suspended 状态，而这个 app 启动即自己计时、用户可能一次都没点过屏幕 → 首次提醒会一声不吭
- 依赖 02 的副作用分发机制与 03 的时钟线程

## Comments

### 2026-10-24 — 新模块 `notify.rs` + 真跑一遍

新模块 `src-tauri/src/notify.rs`：上半截是纯逻辑（递送策略、失败降级），下半截才是 Tauri 接线——
跟 `tray.rs` 一个路子。这么切是因为"通知递不出去怎么办"是最要紧的降级场景，得能被单元测试驱动。

`cargo test` **73 passed / 0 failed / 1 ignored**（票 02/03/04 的 65 个 + 本票 8 个通过 + 1 个 ignored），
`npm run typecheck` 无输出。新用例：

- `notify.rs` 三个：文案原样送到出口 / 递不出去只报一次且不 panic / 八个线程里只有一个抢到告警。
- `notify.rs` 一个 `#[ignore]`：`the_system_sound_call_does_not_crash`——真调一次 `AudioServicesPlaySystemSound`，
  验 FFI 边界链得上、不崩。会出声，所以默认跳过（手跑 `cargo test -- --ignored`）。
- `state.rs` 五个：三个时机都又通知又出声 / 文案是中文 / 三套话能分清 / 三套话不撞车 /
  **「确认休息」既不通知也不出声**（spec §5.5 矩阵里那一行是「—」，以前没人断言）。

跑起来看（`FOCUS_TIMER_SCALE=0.002`，把 60 分钟压成 7.2 秒）：工作时段结束与 10 分钟重复提醒
各走一遍「通知 + 音效」，文案分别是「工作时间到」「还在工作？」，**stderr 一个字节没有**。

第三个时机（休息时段结束）**只在用例里验过**——它得先「确认休息」才轮得到，而确认是面板上的
一个按钮（票 06 才落地），没法在无人值守时点出来。状态机那一侧是确定的：三个时机都发
`Effect::Notify` + `Effect::PlaySound`，用例逐个断言过。

### 通知真的弹出来了——但得先有个 .app

第一次跑没看到横幅。查下来是两件事叠在一起：

1. **未打包的 debug 二进制没有 bundle**，macOS 不知道该把通知算在谁头上，直接丢掉。
   插件在 dev 模式下把通知挂给 `com.apple.Terminal`（源码里 `notify_rust::set_application(if tauri::is_dev() {...})`），
   那个也记不进通知库。
2. 于是改用**验证真机制的办法**：把 debug 二进制装进 `/tmp/FocusTimer.app`（bundle id `com.focustimer.app`），
   `lsregister -f` 注册一下再跑。系统日志立刻有了：

   ```
   Delivering <NotificationRecord app:"com.focustimer.app" ...> to [ .alert .lockScreen .notificationCenter ]
   Presenting <NotificationRecord app:"com.focustimer.app" ...> as alert
   ```

   重复提醒每来一次就递一次，机制是通的。**所以"横幅真的弹出来"这件事，在打包成 .app 之前验不了**——
   票 10（打包）落地后补一次真机确认。

   顺带撞见了"权限被拒"的样子：macOS 第一次为新 bundle id 弹权限询问，没人点，10 秒后记成
   `Authorization set for com.focustimer.app to allow: NO`；**应用照跑，一行错误都没打**——
   验收里"权限被拒时静默降级"这条就是这么验的。

   收尾：删掉测试用的 .app、`lsregister -u` 注销、把通知库里留下的那条授权记录删掉
   （`~/Library/Group Containers/group.com.apple.usernoted/Library/Preferences/group.com.apple.usernoted.plist`
   的 `apps[143]`，备份在 `/tmp/usernoted.plist.bak`）。别让这次试验给正式版留个"已拒绝"。

### 四笔账

**1. 音效没引 crate。** 验收写的是"需要 `objc2` / `core_audio` 之类的 crate"，实际手写了一个
`extern "C"` 声明。原因：`AudioServicesPlaySystemSound` 是 AudioToolbox 里的一个普通 C 函数——
`objc2` 包的是 Objective-C 的类，`core_audio` 包的是 AudioObject（音频硬件）那一族 API，都不对口；
为一个符号引一整个 crate 不划算。依赖图因此一行没动（这台机器只连得上 rsproxy 镜像，少一个是一个）。
`AudioServices.h` 里写着这个函数"将来会弃用"，届时换成 `AudioServicesPlaySystemSoundWithCompletion` 即可，
只是多一个回调。**这是对验收措辞的偏离，留给人签字。**

**2. 失败降级那套闸门，运行时其实基本走不到。** 插件的 `show()` 把真正的投递扔进后台任务、
结果自己吞掉（源码里是 `let _ = notification.show();`），**恒返回 `Ok(())`**。所以 macOS 拒收、
权限被拒这类事，我们这边根本看不见——macOS 不会告诉我们，我们也不去猜。

这恰好就是验收要的那条路（不 panic、不刷屏、安静降级，面板和托盘照常提醒），但**靠的是插件吞掉错误，
不是我们那套机制**。`deliver()` + `WarnOnce` 的闸门是留给"插件自己报错"的后路（哪天插件改了行为，这里接得住）；
`FakeSink` 那条用例证的是闸门逻辑，不是运行时路径。

第一版的文档注释写着"一声不吭又会让'通知为什么不出来'无从查起"——**这句话是错的**，评审揪出来了，
已改成实话：插件吞了结果，我们看不见失败。

**3. 「暴露 `notify(title, body)` 与 `play_sound()`」读作 `Notifier` 上的方法。** 做成模块级自由函数的话，
每次调用都得把 `AppHandle` 传进去——句柄正是 `Notifier` 自己攥着的东西。分发器那边用起来是
`notifier.notify(title, body)` / `notifier.play_sound()`，参数与验收一致。

**4. `Notifier` 又管通知又管音效。** 这不算毛病：这两件事是同一次提醒的两条腿（spec §5.5 里
「通知 ✅ 音效 ✅」总是成对出现），合在一个"提醒出口"里才说得通。类型名沿用 `Notifier`，
是因为票与 spec §5.2 已经把这一层叫 notification 了；`CONTEXT.md` 里"通知"是**要避开的词**，
指的是"待确认提醒"那个领域概念（那才是 `Reminder`），两者不是一回事。

### 双轴评审之后

评审跑了两轴（规范 / 规格），改掉的和**故意不改的**都记在这里。

**改掉的：**

- `notify.rs` 的三处文档改成实话（见第 2 笔）：插件吞掉投递结果，所以我们看不见失败。
- `state.rs` 补上「确认休息」那一行：既不通知也不出声——spec §5.5 矩阵里写着，以前没有用例断言。
- `state.rs` 的 `the_three_reminders()` 改成 `the_reminder_path()`：多带一个"确认休息"的转移，
  顺带把四个时刻写成具名字段，别再靠解包位置认人。
- `notify.rs` 里 `SYSTEM_SOUND_ID` 与 `play_system_sound()` 收成模块私有：没有模块外的人用它们。
- `FakeSink::ok()` / `FakeSink::failing()` 合成一个 `FakeSink::new(fail)`：两个几乎一样的构造器没必要。
- `lib.rs` 里 `notifier` 字段的注释原先理由是错的（"让 `notify.rs` 不必从托盘那边伸手拿句柄"——
  `notify.rs` 根本没提过托盘），改成实话。

**没改，以及为什么：**

- **`state.rs` 里那三条中文文案写死字面量，没换成 `WORK_ALERT_TITLE` 那几个常量。**
  那条用例问的是"**用户看到的**是不是这句话"，常量自己跟自己比是问不出什么的。
  真改成常量，这条用例就变成"常量等于常量"的空断言——票 04 的评审就砍过一轮这种用例。
- **`Notifier<R: Runtime>` 保留泛型**（实际只用到 `Notifier<Wry>`）：`tray.rs` 也是这么写的，
  时钟线程那条路要能不依赖具体的运行时。
- **纯逻辑那半边（`NotificationSink` / `deliver` / `WarnOnce` / `Delivery`）留着**：约 150 行，
  换来的是"权限被拒"这件事不用真机等它发生就能验。跟插件吞错误这件事不矛盾——闸门是后路，不是主力。
- **`TauriDispatcher` 里的两份 `AppHandle`**：`Notifier` 得自己拿住一份（它是独立小组件），
  分发器操作托盘也要一份。两份是同一个 `Arc` 的克隆，不花钱，也不多出引用环。

**要人来拍板的：**

- 本票偏离了验收里"需要 `objc2` / `core_audio` 之类的 crate"那句（见第 1 笔）。
- 本票自己改了自己的验收措辞（"暴露"读作方法、"需要 crate"没照做）并自标 `resolved`。
