import { invoke } from "@tauri-apps/api/core";
import type { OnboardingStatus, TimerAction } from "../types";

/** 把用户动作交给 Rust 侧的状态机。 */
export function sendAction(action: TimerAction): Promise<void> {
  return invoke("timer_action", { action });
}

/** 隐藏面板（不退出应用，计时继续）。 */
export function hidePanel(): Promise<void> {
  return invoke("hide_panel");
}

/**
 * 问 Rust：引导还没做完吗？
 *
 * 引导是 Rust 说了算的（它看着那个标记文件），前端只负责照着画。
 */
export function onboardingStatus(): Promise<OnboardingStatus> {
  return invoke("onboarding_status");
}

/**
 * 告诉 Rust：引导做完了（权限申请也做了）。
 *
 * Rust 侧会记下引导标记，顺手递一条探测通知——macOS 那句「想要发送通知」
 * 就是被它逼出来的（见 `src-tauri/src/onboarding.rs`）。
 *
 * 返回标记有没有真的写下去。
 */
export function completeOnboarding(): Promise<boolean> {
  return invoke("onboarding_complete");
}
