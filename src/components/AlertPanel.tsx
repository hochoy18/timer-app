import { sendAction } from "../lib/actions";

export function AlertPanel() {
  return (
    <div className="stage">
      <div className="alert-box">
        <p className="alert-title">⚠️ 工作已满 1 小时</p>
        <p className="alert-subtitle">若不确认，每 10 分钟将重复提醒</p>
      </div>
      <button
        className="btn btn-primary btn-block"
        onClick={() => void sendAction("acknowledge")}
      >
        马上去休息 15 分钟
      </button>
    </div>
  );
}
