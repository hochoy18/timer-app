/** 引导面板：第一次运行时的一次性说明 + 通知权限申请。 */
export function OnboardingPanel({
  onEnable,
  busy,
}: {
  /** 用户点了"启用通知"。 */
  onEnable: () => void;
  /** 正在申请权限（按钮该灰掉，免得连点两下弹两次框）。 */
  busy: boolean;
}) {
  return (
    <div className="onboarding">
      <p className="onboarding-title">专注计时</p>
      <p className="onboarding-copy">一次工作 60 分钟，然后休息 15 分钟。</p>
      <p className="onboarding-copy">
        工作满一小时会提醒你休息；不确认的话，每 10 分钟重复提醒一次。
      </p>
      <p className="onboarding-copy">提醒走系统通知，授权一次就好。</p>
      <button
        className="btn btn-primary btn-block"
        disabled={busy}
        onClick={onEnable}
      >
        {busy ? "正在申请…" : "启用通知"}
      </button>
    </div>
  );
}
