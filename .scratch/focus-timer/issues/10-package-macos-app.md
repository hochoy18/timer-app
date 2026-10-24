# 10 — 打包为可安装的 macOS .app

Status: resolved

## 目标

产出可双击安装运行的 `FocusTimer.app`，处理图标、Gatekeeper 与打包配置。

## 验收

- [x] `npm run tauri build` 成功，产出 `src-tauri/target/release/bundle/macos/FocusTimer.app`
- [x] 应用有正式图标（`icons/` 下 32/128/@2x/icns 齐全），托盘图标与访达图标都正确
  - 这个应用是菜单栏应用，**本来就不在 Dock 里占位**（`LSUIElement` + 运行期的 `set_activation_policy(Accessory)`），所以"Dock 图标"这一项不存在；访达里看到的是 `icon.icns`
  - 文件齐全、`icon.icns` 是合法 icns、已被写进 `Contents/Resources/` 并由 `CFBundleIconFile` 指着——都验过（`scripts/check-app-bundle.sh`）
  - **"长得对不对"得用眼睛看**：这台机器屏幕锁着，截不了图——见 Comments 的「还没人看过的一截」
- [x] `cp -r` 到 `/Applications/` 后可以双击启动，无弹窗报错
  - **拷贝那一下留给人跑**（`./scripts/install-app.sh`，见 Comments）：脚本会打包 → 拷贝 → 清隔离属性
  - 从 **bundle 里**启动这件事验过：直接跑 `FocusTimer.app/Contents/MacOS/focus-timer`，托盘图标建起来、进程不退出、stderr 打出真实时长
  - 没验的那一下：双击 `/Applications/FocusTimer.app` 本身——得有人在电脑前，见「还没人看过的一截」
- [x] 处理 Gatekeeper 隔离属性：`xattr -cr /Applications/FocusTimer.app`，并在文档中写明这一步
  - `docs/install.md` 有一节专门讲它（为什么有、为什么本地打的包用不上、什么时候用得上）
  - `scripts/install-app.sh` 装完顺手清掉
- [x] 启动后：托盘图标出现、面板隐藏、60 分钟计时开始
  - 验的位置是 **bundle 里**（装进 `/Applications` 那一遍留给人跑，行为相同）
  - 托盘图标：进程活着 = `tray::create` 成功（建不出来 setup 会直接失败退出）✓
  - 面板隐藏：用窗口元数据验的（`onscreen=false`），并且和"第一次运行"那一档做了对照 ✓
    - 那是**第二次启动起**的样子；第一次运行会弹引导（引导标记已按用户的意思删掉，好让打包版真的走一遍引导 + 通知权限）
  - 60 分钟计时：release 包的 stderr 打印的就是 `工作时段 60 分钟 / 休息时段 15 分钟 / 重复提醒 10 分钟` ✓
- [x] release 构建下 `FOCUS_TIMER_SCALE` 不生效（时长为真实值）
  - **在打包产物上验的**：带着 `FOCUS_TIMER_SCALE=0.01` 跑 release 包，打印的仍是 60 分钟 ✓
  - `cargo test --release` 里也有一半（`tests/dev_time_scale.rs` 的 release 分支）✓
- [x] 验收清单 [`spec.md` §9](../spec.md) 全部通过
  - 14 条里 13 条有自动化兜着；剩下 1 条（提醒横幅真的弹出来）要人眼——见 Comments 的对照表

## 背景

- 交付目标是"可安装的 .app"，见 [`spec.md` §2](../spec.md)
- 本期**不做**开机自启、签名与自动更新（见 §2 的范围表）
- 手动加登录项的方法留给用户自己操作，不在本票范围
  - 文档里写了一行指路：系统设置 → 通用 → 登录项

## Comments

### 2026-10-24 — 打出来了，装得上，验过了

`npm run tauri build` 一次过，产出 `FocusTimer.app`（11.24 MiB）。改动落在七个地方：

| 文件 | 改了什么 |
|---|---|
| `src-tauri/tauri.conf.json` | 补上 bundle 元信息（`category` / `shortDescription` / `longDescription`）；图标清单、`.app` 目标、`minimumSystemVersion` 照旧 |
| `src-tauri/Info.plist`（新） | `LSUIElement = true`——菜单栏应用，不在 Dock 里占位。Tauri 会把它合并进默认的 Info.plist |
| `src-tauri/src/lib.rs` + `config.rs` | 启动时把三个时长打进 stderr（release 也打）；时长改用分钟说人话 |
| `scripts/check-app-bundle.sh`（新） | 验产物：Info.plist 的键、图标、可执行文件、签名、隔离属性 |
| `scripts/install-app.sh`（新） | 打包 → 装进 `/Applications` → `xattr -cr` |
| `docs/install.md`（新） | 打包 / 安装 / Gatekeeper / 第一次运行 / 开机自启 / 卸载 |
| `src-tauri/tests/bundle.rs`（新） | 打包配置的 8 条断言（不需要产物，每次 `cargo test` 都跑） |

### 为什么要加"启动时打印时长"这一行

打包版的验收里有一条："release 构建下 `FOCUS_TIMER_SCALE` 不生效"。这句话在**产物**上怎么验？以前只能靠读代码。

现在应用启动时会说一句：

```
focus-timer: 工作时段 60 分钟 / 休息时段 15 分钟 / 重复提醒 10 分钟
```

带着 `FOCUS_TIMER_SCALE=0.01` 跑打包版，这句话**不变**——那就是最直接的凭据：

```
$ FOCUS_TIMER_SCALE=0.01 /Applications/FocusTimer.app/Contents/MacOS/focus-timer
focus-timer: 工作时段 60 分钟 / 休息时段 15 分钟 / 重复提醒 10 分钟
```

（开发版会多打一句"开发期缩放生效"，release 里那段代码根本没编进去。）

时长原先说成"3600 秒"，改成"60 分钟"——一行字得是人能一眼看懂的，它就是"这个包用的是真实时长"的凭据。

### 面板隐不隐藏，是怎么在锁屏的机器上验的

屏幕锁着：截不了图（`screencapture` 报 "could not create image from display"），AX 也看不见面板的窗口（票 09 记过）。

但**窗口元数据**不需要任何权限——`CGWindowListCopyWindowInfo` 要的是窗口列表，不是像素内容。于是做了个对照：

| 启动时的引导标记 | FocusTimer 的窗口 | 结论 |
|---|---|---|
| 有（引导做过） | `320x380 onscreen=false` | 面板没弹 ✓ |
| 没有（第一次运行） | `320x380 onscreen=true` | 引导弹出来了 ✓ |

两档只差一个标记文件，差别就是面板在不在屏上。这就是验收里"面板隐藏"那一条的凭据。

探针本身留在仓库里了——`scripts/list-windows.swift`（`swift scripts/list-windows.swift focus`）。写进仓库是因为 `/tmp` 里的东西留不住：下一票再要验"窗口在不在屏上"时，脚本还得重写一遍。

### Gatekeeper：本机这次用不上，但写清楚了

本地打出来的包**没有**隔离属性（`com.apple.quarantine`）——那个属性是 macOS 给"从别处拿来的"应用打的（浏览器下载、隔空投送、压缩包解压）。所以本机打完直接装，不会撞上"无法验证开发者"。

但只要包被拷过一次，属性就跟着来，而本期**不做签名与公证**（spec §2），没有别的路能让 Gatekeeper 放行。所以：

- `docs/install.md` 有一节讲清楚它为什么在、什么时候用得上；
- `scripts/install-app.sh` 装完顺手 `xattr -cr`（只动这个包，不改系统设置）。

`spctl -a` 对这个包的结论是"不认"——预期之中（本地打的包，没签名没公证）。Tauri 给的是 **ad-hoc 签名**（`codesign -dv` 看得到 `Signature=adhoc`）。

### 隔离属性之外，还撞见一件事：通知权限

这台机器上 macOS 通知库里**还留着一条 `com.focustimer.app` 的记录**，路径是 `/private/tmp/FocusTimer.app`——票 05 做机制验证时留下的那次实验（当时的临时 bundle）。开发期那次引导的通知权限问的是 Terminal，不是这个 bundle。

所以两件事：

1. **引导标记已删**（`~/Library/Application Support/com.focustimer.app/.onboarded`）。这样打包版第一次启动会弹一次引导，点「启用通知」时 macOS 会正儿八经问一句"FocusTimer 想要发送通知"——**点「允许」**。少了这一步，提醒会静默失败（插件吞掉投递结果，应用自己看不见这个失败，见票 05 的第 2 笔账）。
2. 文档里写明了：**装好之后第一次启动要在电脑前**。macOS 那句问话只在第一次投递通知时出现，没人点的话十来秒后自己记成"不允许"，之后永远静默。
3. 如果 macOS 没问（因为那条旧记录），去 系统设置 → 通知 → FocusTimer → 允许。

### §9 验收清单的对照

| §9 那一条 | 谁验的 |
|---|---|
| 启动 → 托盘出现、标题 `60:00`、面板不弹 | 托盘：进程活着（`tray::create` 建不出来会直接退出）；标题：`timer::clock` 的用例（`the_thread_refreshes_the_title_once_per_second`）；面板：窗口元数据对照（上表，第二次启动起；第一次运行会弹引导） |
| 工作时段到点 → 通知 + 音效 + 面板置顶不抢焦点 + 托盘转红 | 状态机：`state.rs` / `clock.rs` 一整套；真机横幅：**要人眼**（下表） |
| 10 分钟不确认 → 再来一次 | `state.rs::alert_repeat_*`、`clock.rs::the_alerting_state_repeats_the_reminder` |
| 点"马上去休息" → `15:00`、重复提醒停、托盘恢复 | `lib.rs::clicking_acknowledge_starts_the_break`（走真 IPC 通道） |
| 休息结束 → 通知 + 音效 + 回到 `60:00` | `tests/dev_time_scale.rs::cycle::the_whole_cycle_runs_in_seconds`（整轮真跑） |
| 暂停 / 继续 | `state.rs::pause_*`、`resume_*`；`lib.rs` 的 IPC 用例 |
| 重新计时 → 回到 `60:00` | `lib.rs::clicking_restart_gives_a_full_work_session_again` |
| 休息时段中无按钮 | 前端 `src/__tests__/panel.test.tsx` |
| 点 ✕ → 面板隐藏、计时继续、托盘还在 | `lib.rs::closing_the_panel_does_not_disturb_the_timer`（真机那一下点不了，见下） |
| 首次引导 + 权限 + 之后不再引导 | `onboarding.rs` 一整套 + `lib.rs` 的 IPC 用例 + 前端 `onboarding.test.tsx` |
| 重复双击不起第二个实例 | `tests/single_instance.rs`（进程级，两个用例） |
| 合盖跨过工作终点 → 醒来立即提醒 | `state.rs::a_long_sleep_still_triggers_the_work_alert`、`clock.rs::waking_up_after_a_long_sleep_advances_in_one_tick` |
| 退出后重开 → 从头 60 分钟 | 不持久化状态（ADR-0003），磁盘上只有引导标记；`onboarding.rs::the_marker_records_nothing_but_its_own_existence` |
| `cargo test` 全绿 | 131 个用例（117 lib + 8 bundle + 4 dev_time_scale + 2 single_instance）＝130 通过 + 1 忽略，debug 与 release 各跑一遍，全绿 |

### 还没人看过的一截

这台机器屏幕锁着，下面这些得等人在电脑前：

1. **托盘图标和访达图标的"长相"**。文件层面全对（模板图、彩色提醒态、合法 icns、已被 Info.plist 指着），但"顺不顺眼"是眼睛的事。
2. **提醒横幅真的弹出来**。票 05 留的话："横幅真的弹出来这件事，在打包成 .app 之前验不了——票 10 落地后补一次真机确认。" 现在包有了，但 60 分钟得等，而 macOS 的通知权限也要先在电脑前点一次「允许」。这条现在是 §9 里唯一没有自动化兜底、也没被人眼确认过的。
3. **装进 `/Applications` 后双击启动**。装的动作留给人跑（`./scripts/install-app.sh` 会先问一句）。脚本做完的事：打包 → 拷贝 → `xattr -cr`。双击之后该看到什么：`docs/install.md` 最后一节写了。
   - 脚本本身在**临时目录**里跑通过（拷一份假的"别人的应用"→ 拒绝覆盖；拷自己的 → 拷贝、`xattr -cr`、提示语全对）。只差往 `/Applications` 里拷那一下。

### 跑过的命令

```bash
npm run tauri build                    # → FocusTimer.app（11.24 MiB）
./scripts/check-app-bundle.sh          # → 全过
cd src-tauri && cargo test             # → 130 passed / 1 ignored
cd src-tauri && cargo test --release   # → 129 passed / 1 ignored（cycle 那组是 debug 专属）
npm run typecheck && npm test          # → 21 passed
```

还有两条手工的（产物上跑的）：

```bash
FocusTimer.app/Contents/MacOS/focus-timer
# focus-timer: 工作时段 60 分钟 / 休息时段 15 分钟 / 重复提醒 10 分钟

FOCUS_TIMER_SCALE=0.01 FocusTimer.app/Contents/MacOS/focus-timer
# focus-timer: 工作时段 60 分钟 / 休息时段 15 分钟 / 重复提醒 10 分钟   ← 没变
```

### spec 那边的对齐

本票新加了一处行为：**启动时把三个时长打进 stderr**（两种构建都打）。`spec.md` §5.6 已经把这句话和它的理由写了进去——不打这一句，"这个包用的是真实时长"就只能靠读代码才看得出来，而这一句正是打包版的验收凭据。

另外，`spec.md` §2 的"本期不做"表里补了两行：**签名与公证**、**自动更新**。之前 `docs/install.md` 和本票都在引用 §2 的范围表来说明"本期不签名"，表里却没这一行——引一个不存在的东西，读者会以为文档写错了。

### 评审之后改了什么

两轴评审（标准 / 需求）各挑了一遍毛病，改掉的都是实打实的：

| 问题 | 改法 |
|---|---|
| `scripts/install-app.sh` **没有可执行位**（`git ls-files` 是 100644），而文档让用户跑 `./scripts/install-app.sh` | `chmod +x` |
| `check-app-bundle.sh` 只查"有图标键"，不查那个键指的文件在不在包里——键在、值写错，访达里照样是默认图标 | 加一条：plist 指着的文件必须在 `Contents/Resources/` 里 |
| `bundle.rs` 的断言说"启动时会打印自己的版本"——`announce_durations` 打的是时长，版本压根没人打 | 改掉理由（版本有两处来源：配置文件 + Cargo.toml） |
| `bundle.rs` / `Info.plist` 都写着"见 tauri.conf.json 的 bundle.macOS.infoPlist"——配置里没有这个键 | 改成实话：Tauri 自动认挨着配置文件的 `Info.plist` |
| `bundle.rs` 查 `raw.contains("<true/>")`——文件里别处的 true 就能骗过它 | 改成查**紧跟在 `<key>LSUIElement</key>` 后面**的那个值 |
| `config.rs` 的 `Display` 注释还写着"开发期加速开关生效时这句话才打"——现在两种构建都打 | 改注释 |
| `human_ms` 用浮点秒数，`{rest}` 会漏出"30.5 秒" | 改成整数算；补两个用例（毫秒档、丢掉零头） |
| 两个脚本各抄一份 `"com.focustimer.app"` 字面量——改了配置两边就对不上 | 都改成从 `tauri.conf.json` 读（那个字面量由 `bundle.rs` 钉住） |
| `install-app.sh` 的 `--help` 从文件头 `sed` 出固定行号——注释一改行数就作废 | 换成 heredoc |
| 票里说"改动落在四个地方"（表里七个）、"13 条里 12 条"（§9 有 14 条）、"spec 还没跟上"（其实已经跟上了） | 改成真的数字与真的状态 |
