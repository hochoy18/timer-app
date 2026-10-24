基于 **Tauri 2.x + TypeScript + React** 的完整可运行工程方案。整个应用常驻 macOS 菜单栏（系统托盘），支持开机运行，窗口可关闭隐藏，后台精确倒计时与系统通知。

---

### 一、 架构与目录结构

```text
timer-app/
├── package.json
├── src/                      # 前端 UI 与状态机（React + TS）
│   ├── App.tsx
│   ├── main.tsx
│   ├── types.ts
│   └── styles.css
└── src-tauri/                # Rust 后端（系统托盘、原生通知、生命周期）
    ├── Cargo.toml
    ├── tauri.conf.json
    ├── capabilities/
    │   └── default.json
    └── src/
        ├── lib.rs
        └── main.rs

```

---

### 二、 Rust 后端配置与实现

#### 1. `src-tauri/Cargo.toml`

引入 Tauri 2.x 核心插件（通知与系统托盘支持）：

```toml
[package]
name = "timer-app"
version = "0.1.0"
description = "A Pomodoro timer for macOS"
edition = "2021"

[dependencies]
tauri = { version = "2", features = ["tray-icon"] }
tauri-plugin-notification = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[build-dependencies]
tauri-build = { version = "2", features = [] }

```

#### 2. `src-tauri/tauri.conf.json`

配置 macOS 专属属性：隐藏 Dock 图标（纯菜单栏应用），关闭默认窗口显示，注册托盘：

```json
{
  "$schema": "https://raw.githubusercontent.com/tauri-apps/tauri/dev/tooling/cli/schema.json",
  "productName": "FocusTimer",
  "version": "0.1.0",
  "identifier": "com.focustimer.app",
  "build": {
    "beforeDevCommand": "npm run dev",
    "devUrl": "http://localhost:5173",
    "beforeBuildCommand": "npm run build",
    "frontendDist": "../dist"
  },
  "app": {
    "macOSPrivateApi": true,
    "windows": [
      {
        "title": "FocusTimer",
        "width": 320,
        "height": 420,
        "resizable": false,
        "alwaysOnTop": true,
        "visible": false,
        "decorations": false,
        "skipTaskbar": true
      }
    ],
    "security": {
      "csp": null
    }
  },
  "bundle": {
    "active": true,
    "targets": ["app"],
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ],
    "macOS": {
      "frameworks": [],
      "minimumSystemVersion": "10.15"
    }
  }
}

```

#### 3. `src-tauri/capabilities/default.json`

Tauri 2 权限声明，启用通知与窗口管理权限：

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Default permissions",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "notification:default",
    "notification:allow-notify",
    "notification:allow-is-permission-granted",
    "notification:allow-request-permission"
  ]
}

```

#### 4. `src-tauri/src/lib.rs`

创建 macOS 菜单栏托盘（Tray Icon），绑定点击显示/隐藏窗口与退出逻辑：

```rust
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            // macOS 专有：隐藏 Dock 栏图标，做成纯托盘应用
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let quit_i = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&quit_i])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::ButtonPress {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

```

---

### 三、 前端核心实现（React + TypeScript）

#### 1. `src/types.ts`

```typescript
export type TimerState = 'IDLE' | 'WORKING' | 'WORK_ALERTING' | 'BREAKING';

export interface TimerConfig {
  workDurationMs: number;    // 60 * 60 * 1000
  breakDurationMs: number;   // 15 * 60 * 1000
  snoozeIntervalMs: number;  // 10 * 60 * 1000
}

// 生产默认值与本地测试缩减值（如需快速测试，把分钟改成秒即可）
export const DEFAULT_CONFIG: TimerConfig = {
  workDurationMs: 60 * 60 * 1000,
  breakDurationMs: 15 * 60 * 1000,
  snoozeIntervalMs: 10 * 60 * 1000,
};

```

#### 2. `src/App.tsx`

集成状态流转、系统原生通知、以及到期提示音：

```tsx
import React, { useState, useEffect, useRef } from 'react';
import { 
  isPermissionGranted, 
  requestPermission, 
  sendNotification 
} from '@tauri-apps/plugin-notification';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { TimerState, DEFAULT_CONFIG } from './types';

export default function App() {
  const [state, setState] = useState<TimerState>('IDLE');
  const [targetTime, setTargetTime] = useState<number>(0);
  const [remainingSec, setRemainingSec] = useState<number>(0);

  const stateRef = useRef<TimerState>(state);
  const targetTimeRef = useRef<number>(targetTime);
  const snoozeTimerRef = useRef<NodeJS.Timeout | null>(null);

  stateRef.current = state;
  targetTimeRef.current = targetTime;

  // 播放系统提示音（Web Audio API 生成双音提示）
  const playBeep = () => {
    try {
      const ctx = new (window.AudioContext || (window as any).webkitAudioContext)();
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = 'sine';
      osc.frequency.setValueAtTime(880, ctx.currentTime);
      gain.gain.setValueAtTime(0.3, ctx.currentTime);
      gain.gain.exponentialRampToValueAtTime(0.01, ctx.currentTime + 0.6);
      osc.connect(gain);
      gain.connect(ctx.destination);
      osc.start();
      osc.stop(ctx.currentTime + 0.6);
    } catch (e) {
      console.warn("Audio Context Error", e);
    }
  };

  const notify = async (title: string, body: string) => {
    let hasPerm = await isPermissionGranted();
    if (!hasPerm) {
      const permission = await requestPermission();
      hasPerm = permission === 'granted';
    }
    if (hasPerm) {
      sendNotification({ title, body });
      playBeep();
    }
  };

  // 1. 启动工作计时（启动时自动进入）
  const startWorkSession = () => {
    if (snoozeTimerRef.current) clearInterval(snoozeTimerRef.current);
    const target = Date.now() + DEFAULT_CONFIG.workDurationMs;
    setTargetTime(target);
    setState('WORKING');
  };

  // 2. 触发休息提醒（1小时结束）
  const triggerWorkAlert = () => {
    setState('WORK_ALERTING');
    notify("工作时间到！", "你已连续工作1小时，该休息15分钟了。");
    
    // 自动唤醒并置顶窗口
    const win = getCurrentWindow();
    win.show();
    win.setFocus();

    // 10 分钟 Snooze 重试循环
    if (snoozeTimerRef.current) clearInterval(snoozeTimerRef.current);
    snoozeTimerRef.current = setInterval(() => {
      if (stateRef.current === 'WORK_ALERTING') {
        notify("强烈提醒：还在工作！", "距离上次提醒已过去10分钟，请务必休息！");
      }
    }, DEFAULT_CONFIG.snoozeIntervalMs);
  };

  // 3. 用户确认响应：开始 15 分钟休息
  const handleConfirmBreak = () => {
    if (snoozeTimerRef.current) {
      clearInterval(snoozeTimerRef.current);
      snoozeTimerRef.current = null;
    }
    const target = Date.now() + DEFAULT_CONFIG.breakDurationMs;
    setTargetTime(target);
    setState('BREAKING');
  };

  // 4. 休息结束：自动进入下一轮 1 小时工作
  const triggerBreakEnd = () => {
    notify("休息结束", "15分钟已结束，开启下一轮1小时专注！");
    startWorkSession();
  };

  // 核心 Tick 循环：每秒校准剩余时间（抵抗休眠漂移）
  useEffect(() => {
    const timer = setInterval(() => {
      const currentState = stateRef.current;
      if (currentState === 'WORKING' || currentState === 'BREAKING') {
        const diff = Math.max(0, targetTimeRef.current - Date.now());
        setRemainingSec(Math.ceil(diff / 1000));

        if (diff <= 0) {
          if (currentState === 'WORKING') {
            triggerWorkAlert();
          } else if (currentState === 'BREAKING') {
            triggerBreakEnd();
          }
        }
      }
    }, 1000);

    return () => clearInterval(timer);
  }, []);

  // 默认启动时立即自动开始
  useEffect(() => {
    startWorkSession();
  }, []);

  const formatTime = (secs: number) => {
    const m = Math.floor(secs / 60);
    const s = secs % 60;
    return `${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`;
  };

  return (
    <div className="container">
      <div className="header">
        <span className="badge">{state}</span>
        <button className="close-btn" onClick={() => getCurrentWindow().hide()}>✕</button>
      </div>

      <div className="display">
        {state === 'WORK_ALERTING' ? (
          <div className="alert-box">
            <p className="alert-title">⚠️ 工作已满 1 小时</p>
            <p className="alert-subtitle">若不确认，每 10 分钟将重复报警</p>
            <button className="btn-primary" onClick={handleConfirmBreak}>
              马上休息 15 分钟
            </button>
          </div>
        ) : (
          <div>
            <div className="time">{formatTime(remainingSec)}</div>
            <div className="status-label">
              {state === 'WORKING' && '专注工作中...'}
              {state === 'BREAKING' && '休息中，放松一下...'}
            </div>
          </div>
        )}
      </div>

      <div className="footer">
        {state !== 'WORK_ALERTING' && (
          <button className="btn-secondary" onClick={startWorkSession}>
            重新计时
          </button>
        )}
      </div>
    </div>
  );
}

```

#### 3. `src/styles.css`

针对 macOS 原生窗口卡片风格适配：

```css
:root {
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
  color: #212529;
  background-color: #f8f9fa;
  user-select: none;
}

body, html {
  margin: 0;
  padding: 0;
  width: 100%;
  height: 100%;
  overflow: hidden;
}

.container {
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  height: 100vh;
  padding: 16px;
  box-sizing: border-box;
}

.header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.badge {
  background: #e9ecef;
  padding: 4px 8px;
  border-radius: 6px;
  font-size: 11px;
  font-weight: 600;
  color: #495057;
}

.close-btn {
  background: transparent;
  border: none;
  cursor: pointer;
  font-size: 14px;
  color: #868e96;
}

.display {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  text-align: center;
  flex: 1;
}

.time {
  font-size: 48px;
  font-weight: 700;
  letter-spacing: -1px;
  color: #111;
}

.status-label {
  margin-top: 8px;
  font-size: 14px;
  color: #6c757d;
}

.alert-box {
  background: #fff3bf;
  border: 1px solid #ffe066;
  padding: 16px;
  border-radius: 8px;
  text-align: center;
}

.alert-title {
  margin: 0 0 4px 0;
  font-weight: bold;
  color: #d9480f;
}

.alert-subtitle {
  margin: 0 0 16px 0;
  font-size: 12px;
  color: #868e96;
}

.btn-primary {
  background: #228be6;
  color: white;
  border: none;
  padding: 10px 16px;
  border-radius: 6px;
  cursor: pointer;
  font-weight: 600;
  width: 100%;
}

.btn-secondary {
  background: #dee2e6;
  color: #495057;
  border: none;
  padding: 8px 12px;
  border-radius: 6px;
  cursor: pointer;
  font-size: 12px;
  width: 100%;
}

```

---

### 四、 本地运行与打包部署

1. **安装依赖与本地调试**：
```bash
npm install @tauri-apps/api @tauri-apps/plugin-notification
npm run tauri dev

```


*启动后菜单栏右上角会出现应用图标，终端中若提示通知权限，点击允许。*
2. **生成独立 `.app**`：
```bash
npm run tauri build

```


*构建物位于 `src-tauri/target/release/bundle/macos/FocusTimer.app`。*
3. **安装至系统并解除 Gatekeeper**：
```bash
# 复制到系统应用目录
cp -r src-tauri/target/release/bundle/macos/FocusTimer.app /Applications/

# 移除 macOS 隔离属性（防止打不开提示损坏）
xattr -cr /Applications/FocusTimer.app

```


4. **开机自启（可选）**：
直接打开 **系统设置 -> 通用 -> 登录项**，在“作为登录项打开”列表中点击 `+`，选入 `/Applications/FocusTimer.app`，即可实现开机自动后台倒计时。
