import { sendAction } from "../lib/actions";
import { formatDuration } from "../lib/format";

export function WorkPanel({ remaining }: { remaining: number }) {
  return (
    <div className="stage">
      <div className="time">{formatDuration(remaining)}</div>
      <div className="status-label">专注工作中…</div>
      <div className="actions">
        <button className="btn btn-secondary" onClick={() => void sendAction("pause")}>
          暂停
        </button>
        <button
          className="btn btn-ghost"
          onClick={() => void sendAction("restart")}
        >
          重新计时
        </button>
      </div>
    </div>
  );
}
