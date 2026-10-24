import { act, cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { requestPermission } from "@tauri-apps/plugin-notification";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { renderApp, stubInvoke } from "../test/harness";

// 前端只跟 Rust 说话，两头都得掐住；`requestPermission` 是唯一一处直接碰
// 浏览器 API 的地方（票 07 点的就是它），所以也得掐住。
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/plugin-notification", () => ({
  requestPermission: vi.fn(),
}));

/**
 * 引导收摊了没有？
 *
 * 不能盯着按钮上的字看：按钮一按就变成"正在申请…"，那会儿引导还杵在屏幕上。
 * 真正的信号是引导那段话整个没了。
 */
async function waitForOnboardingGone() {
  await waitFor(() => {
    expect(screen.queryByText(/一次工作 60 分钟/)).not.toBeInTheDocument();
  });
}

beforeEach(() => {
  stubInvoke();
  vi.mocked(requestPermission).mockResolvedValue("granted");
});

afterEach(() => {
  cleanup();
  vi.mocked(invoke).mockClear();
  vi.mocked(requestPermission).mockClear();
});

describe("the onboarding panel", () => {
  it("explains what the app does and offers 启用通知", async () => {
    stubInvoke({ onboarding_status: { pending: true } });
    await renderApp();

    // 三句话说清：60 分钟工作、15 分钟休息、每 10 分钟重复提醒。
    expect(screen.getByText(/一次工作 60 分钟/)).toBeInTheDocument();
    expect(screen.getByText(/休息 15 分钟/)).toBeInTheDocument();
    expect(screen.getByText(/每 10 分钟重复提醒/)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "启用通知" }),
    ).toBeInTheDocument();
  });

  it("does not show up on a later launch", async () => {
    // Rust 说不用引导（标记在了）→ 直接是计时面板。
    stubInvoke({ onboarding_status: { pending: false } });
    const push = await renderApp();
    await act(async () => {
      push({ state: "working", deadlineMs: Date.now() + 61_000, remainingMs: 0 });
    });

    expect(screen.queryByText("启用通知")).not.toBeInTheDocument();
    expect(screen.getByText("专注工作中")).toBeInTheDocument();
  });

  it("the ✕ only hides the panel and never touches the timer", async () => {
    stubInvoke({ onboarding_status: { pending: true } });
    await renderApp();

    fireEvent.click(screen.getByRole("button", { name: "隐藏面板（计时继续）" }));

    expect(invoke).toHaveBeenCalledWith("hide_panel");
    expect(invoke).not.toHaveBeenCalledWith("timer_action", expect.anything());
  });
});

describe("启用通知", () => {
  it("asks for the permission and then tells Rust the onboarding is done", async () => {
    stubInvoke({ onboarding_status: { pending: true } });
    await renderApp();

    fireEvent.click(screen.getByRole("button", { name: "启用通知" }));

    await waitFor(() => {
      expect(requestPermission).toHaveBeenCalled();
    });
    expect(requestPermission).toHaveBeenCalledTimes(1);
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("onboarding_complete");
    });
  });

  it("closes the onboarding and lets the timer be used again", async () => {
    stubInvoke({ onboarding_status: { pending: true } });
    const push = await renderApp();
    await act(async () => {
      push({ state: "working", deadlineMs: Date.now() + 61_000, remainingMs: 0 });
    });

    fireEvent.click(screen.getByRole("button", { name: "启用通知" }));

    await waitForOnboardingGone();
    // 引导关掉之后，计时面板就在那儿——不阻塞使用。
    expect(screen.getByText("专注工作中")).toBeInTheDocument();
  });

  it("closes the onboarding even when the permission was denied", async () => {
    // 用户点了"不允许"：引导照样关掉，不再唠叨。
    //
    // 插件在桌面上恒回 "granted"（见 `useOnboarding` 里的注释），所以这一支在
    // 真机上走不到。但前端不能把返回值当"一定成功"的保证：换个平台、换个插件
    // 版本，都可能是 "denied"。
    vi.mocked(requestPermission).mockResolvedValue("denied");
    stubInvoke({ onboarding_status: { pending: true } });
    const push = await renderApp();
    await act(async () => {
      push({ state: "working", deadlineMs: Date.now() + 61_000, remainingMs: 0 });
    });

    fireEvent.click(screen.getByRole("button", { name: "启用通知" }));

    await waitForOnboardingGone();
    expect(screen.getByText("专注工作中")).toBeInTheDocument();
  });

  it("closes the onboarding even when the permission request blew up", async () => {
    // 申请权限这一下炸了（比如 webview 里根本没有这个 API）：引导照样关掉，
    // 不能把人堵在这儿。
    vi.mocked(requestPermission).mockRejectedValue(new Error("没有这个 API"));
    stubInvoke({ onboarding_status: { pending: true } });
    const push = await renderApp();
    await act(async () => {
      push({ state: "working", deadlineMs: Date.now() + 61_000, remainingMs: 0 });
    });

    fireEvent.click(screen.getByRole("button", { name: "启用通知" }));

    await waitForOnboardingGone();
    expect(screen.getByText("专注工作中")).toBeInTheDocument();
  });

  it("the button greys out while the permission is being asked for", async () => {
    // 连点两下就弹两次框：申请期间按钮该是灰的。
    let release: (() => void) | undefined;
    vi.mocked(requestPermission).mockImplementation(
      () =>
        new Promise((resolve) => {
          release = () => resolve("granted");
        }),
    );
    stubInvoke({ onboarding_status: { pending: true } });
    await renderApp();

    fireEvent.click(screen.getByRole("button", { name: "启用通知" }));

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "正在申请…" })).toBeDisabled();
    });

    release?.();
    await waitForOnboardingGone();
  });
});
