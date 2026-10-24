import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "../App";
import { captureStateEvent, renderPanel, stubInvoke } from "../test/harness";

// 前端只跟 Rust 说话，两头都得掐住：
//
// - `invoke` 是前端 → Rust 的唯一出口（面板上的按钮、引导的状态）；
// - `listen` 是 Rust → 前端 的唯一入口（状态快照）。
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

beforeEach(() => {
  stubInvoke();
});

afterEach(() => {
  cleanup();
  vi.mocked(invoke).mockClear();
});

describe("the four panel states", () => {
  it("working: the countdown, 专注工作中, and 暂停 / 重新计时", async () => {
    await renderPanel({
      state: "working",
      deadlineMs: Date.now() + 61_000,
      remainingMs: 0,
    });

    expect(screen.getByText("01:01")).toBeInTheDocument();
    expect(screen.getByText("专注工作中")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "暂停" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "重新计时" })).toBeInTheDocument();
  });

  it("paused: the frozen remaining time, 已暂停, and 继续 / 重新计时", async () => {
    await renderPanel({ state: "paused", deadlineMs: null, remainingMs: 40 * 60_000 });

    expect(screen.getByText("40:00")).toBeInTheDocument();
    expect(screen.getByText("已暂停")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "继续" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "重新计时" })).toBeInTheDocument();
  });

  it("pending reminder: the alert box and the single 马上去休息 15 分钟 button", async () => {
    await renderPanel({ state: "workAlerting", deadlineMs: null, remainingMs: 0 });

    expect(screen.getByText("工作已满 1 小时")).toBeInTheDocument();
    expect(screen.getByText("若不确认，每 10 分钟将重复提醒")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "马上去休息 15 分钟" }),
    ).toBeInTheDocument();
    // 除了右上角的 ✕，没有别的按钮——待确认提醒只能被"确认"这一种方式结束。
    expect(screen.getAllByRole("button")).toHaveLength(2);
  });

  it("breaking: the countdown and 休息中, with no action button at all", async () => {
    await renderPanel({
      state: "breaking",
      deadlineMs: Date.now() + 15 * 60_000,
      remainingMs: 0,
    });

    expect(screen.getByText("15:00")).toBeInTheDocument();
    expect(screen.getByText("休息中")).toBeInTheDocument();
    // 休息时段里唯一能按的就是右上角那个 ✕。
    expect(screen.getAllByRole("button")).toHaveLength(1);
    expect(
      screen.getByRole("button", { name: "隐藏面板（计时继续）" }),
    ).toBeInTheDocument();
  });
});

describe("the countdown is interpolated locally", () => {
  it("the countdown is computed from deadlineMs, never accumulated", async () => {
    await renderPanel({
      state: "working",
      deadlineMs: Date.now() + 61_000,
      remainingMs: 0,
    });

    expect(screen.getByText("01:01")).toBeInTheDocument();
  });

  it("past the deadline the countdown stops at 00:00 instead of going negative", async () => {
    await renderPanel({
      state: "working",
      deadlineMs: Date.now() - 5_000,
      remainingMs: 0,
    });

    expect(screen.getByText("00:00")).toBeInTheDocument();
  });

  it("without a deadline the remaining time comes from the snapshot", async () => {
    await renderPanel({ state: "paused", deadlineMs: null, remainingMs: 40 * 60_000 });

    expect(screen.getByText("40:00")).toBeInTheDocument();
  });
});

describe("the state comes only from the snapshots Rust pushes", () => {
  it("no state is invented before the first snapshot arrives", async () => {
    const push = captureStateEvent();
    render(<App />);

    expect(screen.getByText("正在启动…")).toBeInTheDocument();

    await act(async () => {
      push({ state: "working", deadlineMs: Date.now() + 61_000, remainingMs: 0 });
    });

    expect(screen.queryByText("正在启动…")).not.toBeInTheDocument();
    expect(screen.getByText("专注工作中")).toBeInTheDocument();
  });
});

describe("the buttons hand their actions to Rust", () => {
  it("pause", async () => {
    await renderPanel({
      state: "working",
      deadlineMs: Date.now() + 61_000,
      remainingMs: 0,
    });

    fireEvent.click(screen.getByRole("button", { name: "暂停" }));

    expect(invoke).toHaveBeenCalledWith("timer_action", { action: "pause" });
  });

  it("resume", async () => {
    await renderPanel({ state: "paused", deadlineMs: null, remainingMs: 40 * 60_000 });

    fireEvent.click(screen.getByRole("button", { name: "继续" }));

    expect(invoke).toHaveBeenCalledWith("timer_action", { action: "resume" });
  });

  it("restart", async () => {
    await renderPanel({
      state: "working",
      deadlineMs: Date.now() + 61_000,
      remainingMs: 0,
    });

    fireEvent.click(screen.getByRole("button", { name: "重新计时" }));

    expect(invoke).toHaveBeenCalledWith("timer_action", { action: "restart" });
  });

  it("acknowledge", async () => {
    await renderPanel({ state: "workAlerting", deadlineMs: null, remainingMs: 0 });

    fireEvent.click(screen.getByRole("button", { name: "马上去休息 15 分钟" }));

    expect(invoke).toHaveBeenCalledWith("timer_action", { action: "acknowledge" });
  });

  it("the ✕ only hides the panel and never touches the timer", async () => {
    await renderPanel({
      state: "working",
      deadlineMs: Date.now() + 61_000,
      remainingMs: 0,
    });

    fireEvent.click(screen.getByRole("button", { name: "隐藏面板（计时继续）" }));

    expect(invoke).toHaveBeenCalledWith("hide_panel");
    // 关掉面板只是收起，计时继续——所以不该顺手给状态机递一个动作。
    expect(invoke).not.toHaveBeenCalledWith("timer_action", expect.anything());
  });
});
