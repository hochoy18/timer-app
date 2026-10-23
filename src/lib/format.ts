/**
 * 把毫秒格式化成倒计时文本。
 *
 * 规则必须与 Rust 侧 `timer::clock::format_remaining` 逐字一致——两边同属一个
 * 「这一轮还剩多少」的数字，却要同时出现在托盘标题和面板上，不一致会很难解释。
 *
 *   ≤ 60 分钟 → `MM:SS`（分钟位可以到 60，60 分钟写成 `60:00` 而不是 `1:00:00`）
 *   > 60 分钟 → `H:MM:SS`
 */
export function formatDuration(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000));
  const m = Math.floor(total / 60);
  const s = total % 60;
  const pad = (n: number) => String(n).padStart(2, "0");

  if (total > 3600) {
    return `${Math.floor(total / 3600)}:${pad(m % 60)}:${pad(s)}`;
  }
  return `${pad(m)}:${pad(s)}`;
}