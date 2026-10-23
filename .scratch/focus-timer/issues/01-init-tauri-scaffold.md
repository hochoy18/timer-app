# 01 — 初始化 Tauri 2 + React + TS 工程骨架

Status: ready-for-agent

## 目标

在仓库根目录搭出可运行的空壳：Tauri 2.x + Vite + React + TypeScript，能 `npm run tauri dev` 起起来，窗口默认隐藏，托盘图标能显示。

## 验收

- [ ] `package.json` 含 `dev` / `build` / `tauri` 脚本，依赖 `@tauri-apps/api`、`@tauri-apps/plugin-notification`
- [ ] `src-tauri/Cargo.toml` 声明 `tauri`（features 含 `tray-icon`）、`tauri-plugin-notification`、`serde`、`serde_json`
- [ ] `tauri.conf.json`：`identifier`、`productName: FocusTimer`、`macOSPrivateApi: true`、窗口 `visible: false` / `skipTaskbar: true` / `alwaysOnTop: true` / `decorations: false`
- [ ] `capabilities/default.json` 含 `core:default` 与 `notification:default` 相关权限
- [ ] 托盘图标能在菜单栏显示，左键点击可显示/隐藏窗口
- [ ] 窗口关闭按钮只隐藏窗口，不退出应用
- [ ] `npm run tauri dev` 成功，`cargo build` 无警告阻塞

## 背景

- 环境准备见 [`spec.md` §7](../spec.md)——**先装 Rust 和 Tauri CLI**，否则 `cargo tauri dev` 跑不起来
- 目录结构照 [`spec.md` §5.2](../spec.md)
- 尚未实现任何计时逻辑；本票只交付骨架
