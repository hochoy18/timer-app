import { act, render } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { vi } from "vitest";

import App from "../App";
import type { TimerSnapshot } from "../types";

/** Rust 推过来的那一帧。 */
type StateEvent = { payload: TimerSnapshot };

/**
 * 抓住 `timer://state` 的订阅，好往里灌快照。
 *
 * 前端不自己维护状态机：它只认 Rust 推过来的快照（spec §5.3）。所以测试也得
 * 走同一条路——把快照从事件里灌进去，而不是直接给组件塞 props。
 */
export function captureStateEvent(): (snapshot: TimerSnapshot) => void {
  let handler: ((event: StateEvent) => void) | undefined;
  vi.mocked(listen).mockImplementation(((
    _event: string,
    cb: (event: StateEvent) => void,
  ) => {
    handler = cb;
    return Promise.resolve(() => {});
  }) as never);
  return (snapshot) => {
    if (!handler) {
      throw new Error("前端还没订阅 timer://state");
    }
    handler({ payload: snapshot });
  };
}

/**
 * 让 `invoke` 有一副能用的默认回答。
 *
 * 前端跟 Rust 的对话只有 `invoke` 这一个出口：面板上的按钮、引导的状态。
 * 默认回答让用例只关心自己关心的那条命令。
 *
 * `answers` 可以覆盖某条命令的回答（比如让引导处于"还没做完"）。
 */
export function stubInvoke(answers: Record<string, unknown> = {}) {
  vi.mocked(invoke).mockImplementation(((cmd: string) => {
    if (cmd in answers) {
      return Promise.resolve(answers[cmd]);
    }
    if (cmd === "onboarding_status") {
      return Promise.resolve({ pending: false });
    }
    if (cmd === "onboarding_complete") {
      return Promise.resolve(true);
    }
    return Promise.resolve(undefined);
  }) as never);
}

/**
 * 渲染应用，等引导状态问回来。
 *
 * 引导状态是问 Rust 得到的，所以得等一回合。等完之后用例可以往事件里灌快照。
 */
export async function renderApp() {
  const push = captureStateEvent();
  render(<App />);
  await act(async () => {
    await Promise.resolve();
  });
  return push;
}

/**
 * 渲染面板，并灌进一帧快照。
 *
 * 先等引导状态问回来：Rust 一开口就有答案，这一步不该让用例变脆。
 */
export async function renderPanel(snapshot: TimerSnapshot) {
  const push = await renderApp();
  await act(async () => {
    push(snapshot);
  });
}
