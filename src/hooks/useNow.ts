import { useEffect, useState } from "react";

/**
 * 一个本地走动的时钟，用来把 `deadlineMs` 插值成剩余时间。
 *
 * 面板不可见时 WebView 的定时器会被降频——这没关系：真值在 Rust 侧，
 * 这里只影响显示。
 */
export function useNow(intervalMs = 250): number {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(id);
  }, [intervalMs]);

  return now;
}
