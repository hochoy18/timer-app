import { AlertPanel } from "./components/AlertPanel";
import { BreakPanel } from "./components/BreakPanel";
import { PausedPanel } from "./components/PausedPanel";
import { WorkPanel } from "./components/WorkPanel";
import { useNow } from "./hooks/useNow";
import { useTimerState } from "./hooks/useTimerState";
import { hidePanel } from "./lib/actions";

const STATE_LABEL: Record<string, string> = {
  working: "工作中",
  paused: "已暂停",
  workAlerting: "待确认",
  breaking: "休息中",
};

export default function App() {
  const snapshot = useTimerState();
  const now = useNow(250);

  if (!snapshot) {
    return <div className="boot">正在启动…</div>;
  }

  // 倒计时一律由绝对时刻现算，不做本地累减。
  const remaining =
    snapshot.deadlineMs === null
      ? snapshot.remainingMs
      : Math.max(0, snapshot.deadlineMs - now);

  return (
    <div className={`panel panel-${snapshot.state}`}>
      <header className="panel-header">
        <span className="badge">{STATE_LABEL[snapshot.state] ?? snapshot.state}</span>
        <button
          className="icon-btn"
          title="隐藏面板（计时继续）"
          onClick={() => void hidePanel()}
        >
          ✕
        </button>
      </header>

      {snapshot.state === "working" && <WorkPanel remaining={remaining} />}
      {snapshot.state === "paused" && <PausedPanel remaining={remaining} />}
      {snapshot.state === "workAlerting" && <AlertPanel />}
      {snapshot.state === "breaking" && <BreakPanel remaining={remaining} />}
    </div>
  );
}
