import { useEffect, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { TimerSnapshot } from "../types";

/** Rust 侧推送状态的事件名。 */
export const STATE_EVENT = "timer://state";

/**
 * 订阅 Rust 侧的状态推送。
 *
 * 前端不维护自己的状态机：收到一帧快照就照着渲染，倒计时靠 `useNow` 本地插值。
 * Rust 侧在状态变化、或显示的剩余秒数跨过一秒时才推。
 */
export function useTimerState(): TimerSnapshot | null {
  const [snapshot, setSnapshot] = useState<TimerSnapshot | null>(null);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;

    listen<TimerSnapshot>(STATE_EVENT, (event) => {
      setSnapshot(event.payload);
    })
      .then((fn) => {
        if (cancelled) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch((err) => {
        console.error("订阅 timer://state 失败", err);
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  return snapshot;
}
