import { hidePanel } from "../lib/actions";

/**
 * 面板右上角那个 ✕。
 *
 * 它只是把窗口藏起来：计时在 Rust 那边照走，托盘还在（CONTEXT.md 的"面板"）。
 * 引导那屏也是同一个面板窗口，所以这个头两处都用得上。
 */
export function PanelHeader() {
  return (
    <header className="panel-header">
      <button
        className="icon-btn"
        title="隐藏面板（计时继续）"
        aria-label="隐藏面板（计时继续）"
        onClick={() => void hidePanel()}
      >
        ✕
      </button>
    </header>
  );
}
