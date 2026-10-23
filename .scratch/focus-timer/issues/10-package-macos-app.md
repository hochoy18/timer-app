# 10 — 打包为可安装的 macOS .app

Status: ready-for-agent

## 目标

产出可双击安装运行的 `FocusTimer.app`，处理图标、Gatekeeper 与打包配置。

## 验收

- [ ] `npm run tauri build` 成功，产出 `src-tauri/target/release/bundle/macos/FocusTimer.app`
- [ ] 应用有正式图标（`icons/` 下 32/128/@2x/icns 齐全），托盘图标与 Dock/访达图标都正确
- [ ] `cp -r` 到 `/Applications/` 后可以双击启动，无弹窗报错
- [ ] 处理 Gatekeeper 隔离属性：`xattr -cr /Applications/FocusTimer.app`，并在文档中写明这一步
- [ ] 从 `/Applications` 启动后：托盘图标出现、面板隐藏、60 分钟计时开始
- [ ] release 构建下 `FOCUS_TIMER_SCALE` 不生效（时长为真实值）
- [ ] 验收清单 [`spec.md` §9](../spec.md) 全部通过

## 背景

- 交付目标是"可安装的 .app"，见 [`spec.md` §2](../spec.md)
- 本期**不做**开机自启、签名与自动更新（见 §2 的范围表）
- 手动加登录项的方法留给用户自己操作，不在本票范围
