import { invoke } from "@tauri-apps/api/core";
import type { TimerAction } from "../types";

/** 把用户动作交给 Rust 侧的状态机。 */
export function sendAction(action: TimerAction): Promise<void> {
  return invoke("timer_action", { action });
}

/** 隐藏面板（不退出应用，计时继续）。 */
export function hidePanel(): Promise<void> {
  return invoke("hide_panel");
}
