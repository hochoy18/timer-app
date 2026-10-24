import { useCallback, useEffect, useState } from "react";
import { requestPermission } from "@tauri-apps/plugin-notification";

import { completeOnboarding, onboardingStatus } from "../lib/actions";

/** 引导的状态，以及"用户点了启用通知"这个动作。 */
export interface Onboarding {
  /**
   * 引导还没做完吗？
   *
   * `null` = 还没问过 Rust。问之前什么也不画——宁可先显示"正在启动…"，
   * 也不要先画一秒计时面板再换成引导面板。
   */
  pending: boolean | null;
  /** 正在申请权限（按钮该灰掉，免得连点两下弹两次框）。 */
  busy: boolean;
  /** 用户点了"启用通知"：申请权限，然后收摊。 */
  enable: () => Promise<void>;
}

/**
 * 引导：一次性面板 + 通知权限申请。
 *
 * 前端不自己判断"是不是第一次运行"——那件事 Rust 说了算（它看着那个标记
 * 文件）。前端只问一次，然后把答案照着画出来。
 */
export function useOnboarding(): Onboarding {
  const [pending, setPending] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;

    onboardingStatus()
      .then((status) => {
        if (!cancelled) {
          setPending(status.pending);
        }
      })
      .catch((err) => {
        console.error("问引导状态失败", err);
        // 问不着就当不用引导：宁可少一次说明，也不要把用户堵在面板里。
        if (!cancelled) {
          setPending(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, []);

  const enable = useCallback(async () => {
    setBusy(true);
    try {
      // 先申请权限。
      //
      // 这一步在 macOS 上**不会**自己弹出系统框：插件把 `window.Notification`
      // 换成了自己的壳（`init-iife.js`），`requestPermission()` 走
      // `invoke("plugin:notification|request_permission")`，而那条命令在桌面
      // 上恒回 `Granted`——不弹框，连"拒绝"都轮不到用户点。真正把 app 登记进
      // 通知中心、弹出 macOS 那句「想要发送通知」的，是 Rust 侧随后递出的那条
      // 探测通知（见 `src-tauri/src/onboarding.rs`）。
      //
      // 这里照样调它：票 07 点名要这一步；哪天插件在桌面上真做了授权，它就是
      // 对的。抛错了也得往下走——引导不能因为一个弹不出来的框把人堵在这儿。
      try {
        await requestPermission();
      } catch (err) {
        console.error("申请通知权限失败", err);
      }

      // 再告诉 Rust"做完了"：它记下引导标记，顺手递出那条探测通知。
      //
      // 标记写不下去时，Rust 那边仍然记着"没做完"（下次启动还会再引导一次）。
      // 但这次照样把引导关掉——不能因为一个标记文件把人堵在面板里。
      try {
        if (!(await completeOnboarding())) {
          console.warn("引导标记没写下去，下次启动还会再引导一次");
        }
      } catch (err) {
        console.error("记引导标记失败", err);
      }

      // 引导到此为止。允许也好、拒绝也好，都不再挡着用户用这个 app。
      setPending(false);
    } finally {
      setBusy(false);
    }
  }, []);

  return { pending, busy, enable };
}
