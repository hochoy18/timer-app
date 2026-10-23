import { sendAction } from "../lib/actions";
import { formatDuration } from "../lib/format";

export function PausedPanel({ remaining }: { remaining: number }) {
  return (
    <div className="stage">
      <div className="time time-paused">{formatDuration(remaining)}</div>
      <div className="status-label">已暂停</div>
      <div className="actions">
        <button className="btn btn-primary" onClick={() => void sendAction("resume")}>
          继续
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
