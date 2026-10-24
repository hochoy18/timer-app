# 安装 FocusTimer

把 FocusTimer 打成可安装的 `.app`，装进 `/Applications`，双击就能用。

> 领域词汇见 [`CONTEXT.md`](../CONTEXT.md) · 实现方案见 [`spec.md`](../.scratch/focus-timer/spec.md)

## 产物在哪

```
npm run tauri build
```

打出来的是：

```
src-tauri/target/release/bundle/macos/FocusTimer.app
```

一个 11 MiB 左右的目录，里面是标准的 macOS 包结构：

```
FocusTimer.app/Contents/
├── Info.plist          # bundle id、CFBundleIconFile、LSUIElement…
├── MacOS/focus-timer   # 可执行文件
└── Resources/icon.icns # 访达里的图标
```

前置条件（Rust、Tauri CLI）见 [`spec.md` §7](../.scratch/focus-timer/spec.md#7-环境准备)，
或者跑仓库里的向导脚本 `./scripts/setup-toolchain.sh`。

## 装进 /Applications

**一条命令**（会先问一句）：

```bash
./scripts/install-app.sh
```

它做三件事：打包（产物不在时）→ 拷到 `/Applications/FocusTimer.app` → 清掉隔离属性。

**手动也一样**：

```bash
cp -R src-tauri/target/release/bundle/macos/FocusTimer.app /Applications/
xattr -cr /Applications/FocusTimer.app
```

## Gatekeeper：为什么要跑 `xattr -cr`

macOS 给"从别处拿来的"应用打一个 `com.apple.quarantine` 属性，Gatekeeper 靠它决定
要不要拦。本地打出来的包**本来没有**这个属性，所以本机打完直接装，通常不会撞上。

但只要这个包被拷过一次——从另一台机器拷来、从压缩包里解出来、或者隔着网盘传过
——属性就跟着来了。这时双击会弹：

> 无法打开"FocusTimer"，因为无法验证开发者。

本期**不做签名与公证**（见 [`spec.md` §2](../.scratch/focus-timer/spec.md#2-范围)），
所以没有别的路能让 Gatekeeper 认这个包。`xattr -cr` 就是那一步：

```bash
xattr -cr /Applications/FocusTimer.app
```

- `-c` 清除属性，`-r` 递归（包里的每一层都要清）；
- 只动这个应用包，不改任何系统设置；
- 代价：Gatekeeper 对这个包的检查被跳过了。自己的应用、自己签的构建，安全上没问题。

想确认某个包到底带没带这个属性：

```bash
xattr -l /Applications/FocusTimer.app
```

## 第一次运行

1. **引导**：第一次运行会弹一次引导面板，讲清楚 60 / 15 / 10 三件事。做完就不再出现。
2. **通知权限**：引导里点「启用通知」，macOS 会问一句"FocusTimer 想要发送通知"——
   **要点「允许」**。

   这句问话只在 macOS 第一次为这个 bundle 投递通知时出现，而且**没人点的话，
   十来秒后系统会自己记成"不允许"**，之后所有通知静默失败（应用自己看不见这个失败）。

   所以：装好之后**第一次启动要在电脑前**，别装完就走。

   已经错过（引导做完了但通知没允许）？删掉引导标记，重来一次：

   ```bash
   rm ~/Library/Application\ Support/com.focustimer.app/.onboarded
   ```

   下次启动引导会再来一遍，通知权限也会重新问。引导标记是磁盘上唯一的文件
   （[ADR-0003](./adr/0003-no-state-persistence.md)），删掉它不影响别的。

## 开机自启

本期**不做**（见 [`spec.md` §2](../.scratch/focus-timer/spec.md#2-范围) 的范围表）。
要开机自启就手动加登录项：

系统设置 → 通用 → 登录项 → ＋ → 选 `/Applications/FocusTimer.app`。

## 卸载

```bash
rm -rf /Applications/FocusTimer.app
rm -rf ~/Library/Application\ Support/com.focustimer.app
```

第二条是数据目录，里面只有引导标记（`.onboarded`）。删掉它，下次装回来会重新引导。

## 验一个包

```bash
./scripts/check-app-bundle.sh
```

查的是**产物**：Info.plist 里该有的键（bundle id、图标、`LSUIElement`）、图标文件、
可执行文件是不是 Mach-O、有没有签名、有没有隔离属性。

配置层面的验在 `cargo test` 里（`src-tauri/tests/bundle.rs`）——那个不需要产物，
每次 `cargo test` 都会跑一遍。

## 手工验一遍

1. `open /Applications/FocusTimer.app`
2. 菜单栏出现一个表盘图标，标题是 `60:00`；面板不弹出来（首次运行会弹引导）
3. 点托盘图标 → 面板出现；面板右上角的 ✕ → 面板收起，计时继续
4. 托盘图标右键 → 退出

release 包的时长是真实值。想快进验证整个轮次，用开发版：

```bash
FOCUS_TIMER_SCALE=0.01 npm run tauri dev
```

打包版无视这个变量（那段代码在 release 里根本没编进去，见
[`spec.md` §5.6](../.scratch/focus-timer/spec.md#56-配置)）。
