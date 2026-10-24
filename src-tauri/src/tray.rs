//! 托盘：菜单栏一侧的全部表现。
//!
//! 分两层：
//!
//! - **纯逻辑**——菜单项 id ↔ 动作、状态下拉、点击怎么算。都在下面
//!   `TrayCommand` 那一段里，不碰任何 Tauri 句柄（用的 `MouseButton` 只是两个
//!   普通枚举），因此能被单元测试精确驱动。
//! - **Tauri 接线**——建图标、建菜单、点哪儿调哪儿。只照着纯逻辑的结论做，
//!   自己不再判断。
//!
//! 这么切是因为托盘的行为大半是"什么状态下、点什么、按哪个键"，这些不启动
//! app 就能验；真正只有真跑才看得见的只剩 macOS 的绘制效果。
//!
//! 面板窗口本身不在这一层，它住在 [`crate::panel`]——托盘只是**叫它**显示或
//! 收起。
//!
//! 标题文案不在这一层：它是引擎算出来的一行字符串，和 [`crate::timer::clock::format_remaining`]
//! 是同一条规则，住在引擎那边才不会被抄成第三份。

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

use crate::panel;
use crate::timer::clock::ClockThread;
use crate::timer::state::{Event, TimerState};
use crate::on_main;

/// 托盘图标 id。托盘句柄是按 id 从 Tauri 状态表里取的，所以它得是常量。
const TRAY_ID: &str = "focus-timer-tray";

/// 菜单项 id。菜单项本身由 muda 持有，事件回调只给得出 id。
const MENU_TOGGLE_PANEL: &str = "toggle-panel";
const MENU_PAUSE: &str = "pause";
const MENU_QUIT: &str = "quit";

/// 常态托盘图标（黑色剪影，按模板渲染，自动跟随菜单栏明暗）。
const ICON_NORMAL: &[u8] = include_bytes!("../icons/tray.png");
/// 提醒态托盘图标（红色，本身带颜色，不走模板）。
const ICON_ALERT: &[u8] = include_bytes!("../icons/tray-alert.png");

// ── 纯逻辑 ────────────────────────────────────────────────────────────

/// 托盘能触发的动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    /// 显示 / 隐藏面板。
    TogglePanel,
    /// 暂停工作时段。
    Pause,
    /// 退出应用。
    Quit,
}

/// 菜单项 id → 动作。
pub fn command_for_menu(id: &str) -> Option<TrayCommand> {
    match id {
        MENU_TOGGLE_PANEL => Some(TrayCommand::TogglePanel),
        MENU_PAUSE => Some(TrayCommand::Pause),
        MENU_QUIT => Some(TrayCommand::Quit),
        _ => None,
    }
}

/// 点击托盘图标 → 动作。
///
/// **只看抬起那一下。** macOS 的一次左键点击会先发 `Down` 再发 `Up`，两次都触发
/// 点击回调；两下都当成"切换"，就会先显示再隐藏，面板等于没点开。
pub fn command_for_click(
    button: MouseButton,
    button_state: MouseButtonState,
) -> Option<TrayCommand> {
    match (button, button_state) {
        (MouseButton::Left, MouseButtonState::Up) => Some(TrayCommand::TogglePanel),
        _ => None,
    }
}

/// `暂停` 菜单项是否可用：只在工作时段进行中可点。
pub fn pause_item_enabled(state: TimerState) -> bool {
    state == TimerState::Working
}

// ── Tauri 接线 ────────────────────────────────────────────────────────

/// 建托盘：图标、菜单、左键 / 右键两套行为。
pub fn create<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(ICON_NORMAL)?)
        // 常态图标是黑色剪影：走模板，macOS 才能自动跟随菜单栏的明 / 暗。
        .icon_as_template(true)
        // 左键留给"切换面板"，菜单只走右键。
        .show_menu_on_left_click(false)
        // 初始菜单按 `WORKING` 拼：托盘建于时钟之前，而应用启动即进入工作时段
        // （spec §4.1），所以这就是启动那一刻的真状态。时钟的第一帧还会用
        // `apply_state` 再拼一次，把菜单对齐到实际状态。
        .menu(&build_menu(app, TimerState::Working)?)
        .on_menu_event(|app, event| {
            if let Some(command) = command_for_menu(event.id().as_ref()) {
                run_command(app, command);
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button,
                button_state,
                ..
            } = event
            {
                if let Some(command) = command_for_click(button, button_state) {
                    run_command(tray.app_handle(), command);
                }
            }
        })
        .build(app)?;
    Ok(())
}

/// 按当前状态拼出托盘菜单。
///
/// 状态一变就整个重建，而不是留着菜单项句柄去改可用性：状态切换一次要按小时，
/// 重建一个四项菜单的代价可以忽略，换来的是不必把 `MenuItem` 塞进 Tauri 的状态表
/// （也就绕开了"这东西到底 Send + Sync 吗"的纠缠）。
fn build_menu<R: Runtime>(app: &AppHandle<R>, state: TimerState) -> tauri::Result<Menu<R>> {
    // 菜单里的"打开面板"和左键是同一个动作：面板已显示就收起来。
    let toggle = MenuItem::with_id(app, MENU_TOGGLE_PANEL, "打开面板", true, None::<&str>)?;
    let pause = MenuItem::with_id(
        app,
        MENU_PAUSE,
        "暂停",
        pause_item_enabled(state),
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "退出", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    Menu::with_items(app, &[&toggle, &pause, &separator, &quit])
}

/// 设托盘标题（剩余时间）。
pub fn set_title<R: Runtime>(app: &AppHandle<R>, title: &str) {
    let title = title.to_string();
    on_main(app, move |app| {
        if let Some(tray) = app.tray_by_id(TRAY_ID) {
            let _ = tray.set_title(Some(title));
        }
    });
}

/// 切提醒态样式：常态图标 ↔ 醒目图标。
///
/// 标题的红点不在这儿管——标题整条由引擎算好送来（`clock::title_for`）。分开是为了
/// 让"托盘上现在长什么样"只有一个地方说了算。
pub fn set_alerting<R: Runtime>(app: &AppHandle<R>, alerting: bool) {
    on_main(app, move |app| {
        let Some(tray) = app.tray_by_id(TRAY_ID) else {
            return;
        };
        let bytes = if alerting { ICON_ALERT } else { ICON_NORMAL };
        let Ok(icon) = Image::from_bytes(bytes) else {
            return;
        };
        // 醒目图标是红色的，不能走模板——模板图只取 alpha，会被抹成菜单栏的前景色，
        // 那"醒目"就白换了。
        let _ = tray.set_icon_with_as_template(Some(icon), !alerting);
    });
}

/// 状态变了：重建菜单，让 `暂停` 该灰的时候灰下去。
pub fn apply_state<R: Runtime>(app: &AppHandle<R>, state: TimerState) {
    on_main(app, move |app| {
        let Some(tray) = app.tray_by_id(TRAY_ID) else {
            return;
        };
        if let Ok(menu) = build_menu(&app, state) {
            let _ = tray.set_menu(Some(menu));
        }
    });
}

/// 执行一个托盘动作。
fn run_command<R: Runtime>(app: &AppHandle<R>, command: TrayCommand) {
    match command {
        TrayCommand::TogglePanel => panel::toggle(app),
        TrayCommand::Pause => {
            if let Some(clock) = app.try_state::<ClockThread>() {
                clock.apply(Event::Pause);
            }
        }
        // 走 Tauri 的退出流程，`RunEvent::Exit` 里会停掉时钟线程。
        TrayCommand::Quit => app.exit(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 标题的用例在 `timer::clock` 那边——标题是引擎算的，测试也跟着住过去。

    // ── 菜单 ───────────────────────────────────────────────────────────

    #[test]
    fn pause_is_only_enabled_while_working() {
        assert!(pause_item_enabled(TimerState::Working));
        assert!(!pause_item_enabled(TimerState::Paused));
        assert!(!pause_item_enabled(TimerState::WorkAlerting));
        assert!(!pause_item_enabled(TimerState::Breaking));
    }

    #[test]
    fn menu_items_map_to_commands() {
        assert_eq!(
            command_for_menu(MENU_TOGGLE_PANEL),
            Some(TrayCommand::TogglePanel)
        );
        assert_eq!(command_for_menu(MENU_PAUSE), Some(TrayCommand::Pause));
        assert_eq!(command_for_menu(MENU_QUIT), Some(TrayCommand::Quit));
        assert_eq!(command_for_menu("nonsense"), None);
    }

    // ── 点击 ───────────────────────────────────────────────────────────

    #[test]
    fn a_left_click_release_toggles_the_panel() {
        assert_eq!(
            command_for_click(MouseButton::Left, MouseButtonState::Up),
            Some(TrayCommand::TogglePanel)
        );
    }

    #[test]
    fn a_left_click_press_does_not_toggle_the_panel() {
        // macOS 一次点击先 Down 再 Up。两下都切换的话，面板会先显示再隐藏，
        // 等于点了个寂寞——所以只有抬起那一下算数。
        assert_eq!(
            command_for_click(MouseButton::Left, MouseButtonState::Down),
            None
        );
    }

    #[test]
    fn right_clicks_do_not_toggle_the_panel() {
        // 右键是菜单，不该顺手把面板也切一下。
        assert_eq!(
            command_for_click(MouseButton::Right, MouseButtonState::Up),
            None
        );
        assert_eq!(
            command_for_click(MouseButton::Middle, MouseButtonState::Up),
            None
        );
    }
}