import { formatDuration } from "../lib/format";

export function BreakPanel({ remaining }: { remaining: number }) {
  return (
    <div className="stage">
      <div className="time time-break">{formatDuration(remaining)}</div>
      <div className="status-label">休息中，放松一下…</div>
    </div>
  );
}
