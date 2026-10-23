# 01 — 初始化 Tauri 2 + React + TS 工程骨架

Status: ready-for-human

## 目标

在仓库根目录搭出可运行的空壳：Tauri 2.x + Vite + React + TypeScript，窗口默认隐藏，能 `npm run tauri dev` 起起来。

## 验收

- [x] `package.json` 含 `dev` / `build` / `tauri` 脚本，依赖 `@tauri-apps/api`、`@tauri-apps/plugin-notification`
- [x] `src-tauri/Cargo.toml` 声明 `tauri`（features 含 `tray-icon`）、`tauri-plugin-notification`、`serde`、`serde_json`
- [x] `tauri.conf.json`：`identifier`、`productName: FocusTimer`、窗口 `visible: false` / `skipTaskbar: true` / `alwaysOnTop: true` / `decorations: false`
- [x] `capabilities/default.json` 含 `core:default` 与 `notification:default` 相关权限
- [x] 图标齐备（`scripts/generate-icons.py` 生成 `.icns` / `.ico` / 托盘模板图）
- [x] 前端侧通过校验：`npm run typecheck` 与 `npm run build` 均通过
- [ ] `npm run tauri dev` 成功，`cargo build` 无警告阻塞 ← **被工具链阻塞**
- [ ] 托盘图标能在菜单栏显示，左键点击可显示/隐藏窗口 ← 归 04 托盘
- [ ] 窗口关闭按钮只隐藏窗口，不退出应用 ← 归 06 面板
- [ ] 圆角透明窗口（`macOSPrivateApi` + `transparent`）← 归 06 面板

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