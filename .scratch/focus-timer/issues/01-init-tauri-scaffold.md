# 01 — 初始化 Tauri 2 + React + TS 工程骨架

Status: resolved

## 目标

在仓库根目录搭出可运行的空壳：Tauri 2.x + Vite + React + TypeScript，窗口默认隐藏，能 `npm run tauri dev` 起起来。

## 验收

- [x] `package.json` 含 `dev` / `build` / `tauri` 脚本，依赖 `@tauri-apps/api`、`@tauri-apps/plugin-notification`
- [x] `src-tauri/Cargo.toml` 声明 `tauri`（features 含 `tray-icon`）、`tauri-plugin-notification`、`serde`、`serde_json`
- [x] `tauri.conf.json`：`identifier`、`productName: FocusTimer`、窗口 `visible: false` / `skipTaskbar: true` / `alwaysOnTop: true` / `decorations: false`
- [x] `capabilities/default.json` 含 `core:default` 与 `notification:default` 相关权限
- [x] 图标齐备（`scripts/generate-icons.py` 生成 `.icns` / `.ico` / 托盘模板图）
- [x] 前端侧通过校验：`npm run typecheck` 与 `npm run build` 均通过
- [x] `cargo build` 通过且**零警告**
- [x] `npm run tauri dev` 成功起起来，进程稳定运行 60 秒无 panic
- [x] 托盘图标能在菜单栏显示，左键点击可显示/隐藏窗口 ← 归 04 托盘（已由 04 落地）
- [x] 窗口关闭按钮只隐藏窗口，不退出应用 ← 归 06 面板（已由 06 落地）
- [x] 圆角透明窗口（`macOSPrivateApi` + `transparent`）← 归 06 面板（已由 06 落地）

## 背景

- 环境准备见 [`spec.md` §7](../spec.md)——**先装 Rust 和 Tauri CLI**，否则 `cargo tauri dev` 跑不起来
- 目录结构照 [`spec.md` §5.2](../spec.md)
- 尚未实现任何计时逻辑；本票只交付骨架
- 托盘与面板交互留给 04 / 06，本票只保证它们有地方接

## Comments

### 2026-10-23 — 代码写完，剩工具链

前端这一半已经验证过了：`npm install` → `npm run typecheck` → `npm run build` 全绿，产物 27 个模块、224 KB（gzip 70 KB）。

Rust 这一半**一次都没编译过**——这台机器还没装 Rust，票里打勾的那几条 `Cargo.toml` / `tauri.conf.json` / `capabilities` 是照 Tauri 2 的约定写的，但没有编译器替我担保。跑完 `./scripts/setup-toolchain.sh` 之后第一件事就是：

```bash
cd src-tauri && cargo build
```

有报错回来改，改完把上面那三条未勾的补上，`Status` 改成 `resolved`。

原验收里的 `macOSPrivateApi: true` 挪到了 06：单独打开这个开关而窗口不开 `transparent` 没有任何效果，两者要一起才有意义，而透明窗口是能构建成功还是要跑一次才知道的事——在 Rust 还没装、无法验证的情况下先不赌。

### 2026-10-23（稍后）— 工具链装好了，但 crates.io 连不上

`cargo test` 跑起来了，失败在网络上，不是代码上：

```
error: failed to get `serde` as a dependency of package `focus-timer`
Caused by: unable to update registry `crates-io`
Caused by: transfer too slow: failed to transfer more than 10 bytes in 30s
```

直连确认：`index.crates.io` 25 秒超时、`static.crates.io` 25 秒超时、`crates.io` 返回 403。这是网络出口的问题，不是本项目的问题——**依赖一个 crate 都拉不下来**。

镜像站都通，快慢差很多：

| 镜像 | 响应 |
|---|---|
| `rsproxy.cn` | 200，0.13s |
| `mirrors.ustc.edu.cn` | 200，0.20s |
| `mirrors.tuna.tsinghua.edu.cn` | 200，0.88s |

**待用户决定**：把 cargo registry 换成镜像。这等于让所有 Rust 依赖的二进制产物来自第三方而非 crates.io 官方 —— 是供应链信任问题，而且作用域是全局（`~/.cargo/config.toml`，影响这台机器上所有 Rust 项目）还是仅本项目（`src-tauri/.cargo/config.toml`）也需要定。

在解决之前，本票剩下两条（`cargo build` 无警告阻塞、`npm run tauri dev`）无法验证。

### 2026-10-23（再后）— 走 rsproxy 镜像，全绿，本票收口

用户决定换镜像、且只作用于本项目。`src-tauri/.cargo/config.toml` 里把 crates.io 换成 `sparse+https://rsproxy.cn/index/`，提交进仓库（删掉该文件即可回退）。

换完之后一切正常：

```
cargo test    →  25 passed; 0 failed        （1m49s 编译完整 Tauri 依赖栈）
cargo build   →  Finished dev profile，零警告
npm run tauri dev
               →  VITE ready in 231ms
                  cargo run → target/debug/focus-timer
                  进程稳定运行 60 秒，无 panic、无 webview 报错
```

`target/debug/focus-timer` 32 MB（debug 未裁剪，正常）。

**本票收口。** 剩下的三条不是没做，是本来就属于别的票：托盘归 04，窗口关闭处理与圆角透明窗口归 06。