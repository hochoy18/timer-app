import { AlertPanel } from "./components/AlertPanel";
import { BreakPanel } from "./components/BreakPanel";
import { OnboardingPanel } from "./components/OnboardingPanel";
import { PanelHeader } from "./components/PanelHeader";
import { PausedPanel } from "./components/PausedPanel";
import { WorkPanel } from "./components/WorkPanel";
import { useNow } from "./hooks/useNow";
import { useOnboarding } from "./hooks/useOnboarding";
import { useTimerState } from "./hooks/useTimerState";

export default function App() {
  const snapshot = useTimerState();
  const now = useNow(250);
  const onboarding = useOnboarding();

  // 引导状态还没问回来之前什么也不画：先亮一帧"正在启动…"，比先画计时面板
  // 再换成引导面板干净。
  if (onboarding.pending === null) {
    return <div className="boot">正在启动…</div>;
  }

  // 首次运行：引导占住这个窗口。计时照跑，只是先让用户看一眼这个 app 是
  // 干什么的、把通知权限要到手。
  if (onboarding.pending) {
    return (
      <div className="panel panel-onboarding">
        <PanelHeader />
        <OnboardingPanel
          onEnable={() => void onboarding.enable()}
          busy={onboarding.busy}
        />
      </div>
    );
  }

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
      <PanelHeader />

      {snapshot.state === "working" && <WorkPanel remaining={remaining} />}
      {snapshot.state === "paused" && <PausedPanel remaining={remaining} />}
      {snapshot.state === "workAlerting" && <AlertPanel />}
      {snapshot.state === "breaking" && <BreakPanel remaining={remaining} />}
    </div>
  );
}
