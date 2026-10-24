//! 打包配置的验收。
//!
//! 票 10：把应用打成可安装的 `.app`。这一层验的是**配置本身**——打包前就该
//! 知道的东西，而不是等 `tauri build` 跑完几分钟才发现写错了。
//!
//! 真正打出来的包由 `scripts/check-app-bundle.sh` 验（那个得先有产物才能验）；
//! 这一层不依赖任何产物，所以每次 `cargo test` 都会跑一遍。
//!
//! ## 为什么图标要在这里再验一次
//!
//! `bundle.icon` 里少一个图标，Tauri 在打包时会报错——但那要等到 build 的
//! 最后一步才炸，前面的编译时间全白搭。把它挪到测试里，几秒钟就能发现。

use std::path::{Path, PathBuf};

/// 打包配置文件的路径。
///
/// cargo 跑集成测试时，工作目录是 crate 的根目录（`src-tauri/`）。
const CONFIG_PATH: &str = "tauri.conf.json";

/// 读一份 JSON 配置。
fn config() -> serde_json::Value {
    let raw = std::fs::read_to_string(CONFIG_PATH)
        .unwrap_or_else(|e| panic!("读不出 {CONFIG_PATH}：{e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{CONFIG_PATH} 不是合法 JSON：{e}"))
}

/// 配置文件里的某个值。
fn field<'a>(value: &'a serde_json::Value, path: &[&str]) -> &'a serde_json::Value {
    let mut current = value;
    for key in path {
        current = current
            .get(*key)
            .unwrap_or_else(|| panic!("配置里没有 {}", path.join(".")));
    }
    current
}

// ── 应用叫什么 ────────────────────────────────────────────────────────

#[test]
fn the_bundle_is_called_focus_timer() {
    // 验收里写死了产物路径：`.../bundle/macos/FocusTimer.app`。
    // 产品名一改，那个路径就变了——所以这条把名字钉住。
    let cfg = config();
    assert_eq!(field(&cfg, &["productName"]), "FocusTimer");
}

#[test]
fn the_identifier_is_stable() {
    // identifier 决定两件事：
    //
    // - 应用数据目录：`~/Library/Application Support/<identifier>/`（引导标记
    //   住在那里，见 `onboarding.rs`）；
    // - 单实例的 socket 与锁：`/tmp/<identifier 把 . - 换成 _>_si.*`。
    //
    // 改它 = 用户"重装"了一次应用（引导重来、单实例名分也换了）。
    let cfg = config();
    assert_eq!(field(&cfg, &["identifier"]), "com.focustimer.app");
}

#[test]
fn the_bundle_version_matches_the_crate_version() {
    // 版本这件事有两处来源：打包出来的 `CFBundleShortVersionString` 取自
    // `tauri.conf.json` 的 `version`，而 `Cargo.toml` 里另有一份（`CARGO_PKG_VERSION`）。
    // 两份对不上，"我装的是哪一版"就有两个答案了。
    let cfg = config();
    let bundle_version = field(&cfg, &["version"]).as_str().expect("版本该是字符串");
    let cargo_version = env!("CARGO_PKG_VERSION");
    assert_eq!(
        bundle_version, cargo_version,
        "tauri.conf.json 的 version 与 Cargo.toml 的 version 对不上"
    );
}

// ── 打什么 ────────────────────────────────────────────────────────────

#[test]
fn bundling_is_on_and_produces_an_app() {
    let cfg = config();
    assert_eq!(field(&cfg, &["bundle", "active"]), true);
    let targets = field(&cfg, &["bundle", "targets"])
        .as_array()
        .expect("targets 该是数组");
    let names: Vec<&str> = targets
        .iter()
        .map(|t| t.as_str().expect("target 该是字符串"))
        .collect();
    assert!(
        names.contains(&"app"),
        "打包目标里没有 app：{names:?}"
    );
}

// ── 图标 ──────────────────────────────────────────────────────────────

#[test]
fn every_icon_the_bundle_names_is_on_disk() {
    // `bundle.icon` 里的每个路径都得指着磁盘上真实存在的文件。
    let cfg = config();
    let icons = field(&cfg, &["bundle", "icon"])
        .as_array()
        .expect("icon 该是数组");
    assert!(!icons.is_empty(), "打包配置里一个图标都没列");

    for icon in icons {
        let rel = icon.as_str().expect("图标路径该是字符串");
        let path = Path::new(rel);
        assert!(path.is_file(), "打包配置里点名的图标不存在：{rel}");
        let size = std::fs::metadata(path).expect("量不出图标大小").len();
        assert!(size > 0, "图标是空文件：{rel}");
    }
}

#[test]
fn the_macos_icon_set_is_complete() {
    // 验收写的是"32 / 128 / @2x / icns 齐全"。这四样是 macOS 打包真正要的：
    // 访达里的图标来自 `icon.icns`，图标集里的其余尺寸是 Tauri's 默认要求。
    let cfg = config();
    let icons: Vec<String> = field(&cfg, &["bundle", "icon"])
        .as_array()
        .expect("icon 该是数组")
        .iter()
        .map(|i| i.as_str().expect("图标路径该是字符串").to_string())
        .collect();

    for required in [
        "icons/32x32.png",
        "icons/128x128.png",
        "icons/128x128@2x.png",
        "icons/icon.icns",
    ] {
        assert!(
            icons.iter().any(|i| i == required),
            "打包配置里少了 {required}：{icons:?}"
        );
        let path = Path::new(required);
        assert!(path.is_file(), "{required} 不在磁盘上");
    }
}

#[test]
fn the_tray_icons_are_on_disk() {
    // 托盘图标是**编进二进制**的（`tray.rs` 里的 `include_bytes!`），
    // 所以它们不在 `bundle.icon` 里。但它们同样得在磁盘上——而且这张清单
    // 写在这儿，是为了让"图标齐不齐"这件事有一处能一眼看完的地方。
    for name in ["icons/tray.png", "icons/tray-alert.png"] {
        let path = Path::new(name);
        assert!(path.is_file(), "托盘图标不存在：{name}");
    }
}

// ── 菜单栏应用 ────────────────────────────────────────────────────────

#[test]
fn the_app_declares_itself_a_menu_bar_app() {
    // `Info.plist` 里的 `LSUIElement` 是"不出现在 Dock 里"的开关。
    //
    // 这份文件跟 `tauri.conf.json` 放在一起，Tauri 打包时会自动找到它并
    // 合并进默认的 Info.plist（`bundle.macOS.infoPlist` 没配，靠的就是
    // 这个"挨着配置文件就自动认"的规矩）。
    //
    // 这里只做字符串检查，不去解析 XML——为一个断言引一个 XML 解析器不值当，
    // 而这份文件是我们自己手写的，内容就两行。
    let path = PathBuf::from("Info.plist");
    assert!(
        path.is_file(),
        "Info.plist 不见了：菜单栏应用得靠它声明 LSUIElement"
    );
    let raw = std::fs::read_to_string(&path).expect("读不出 Info.plist");

    // LSUIElement 后面**紧跟**的那个值必须是 <true/>。
    // 光在文件里找 "<true/>" 这几个字会被别处的 true 骗过去——要找的是
    // 紧跟在键后面的那一个值。
    let after_key = raw
        .split_once("<key>LSUIElement</key>")
        .map(|(_, rest)| rest.trim_start())
        .expect("Info.plist 里没有 LSUIElement 这个键");
    assert!(
        after_key.starts_with("<true/>"),
        "Info.plist 里的 LSUIElement 该是 true（应用不该在 Dock 里占位）"
    );
}
