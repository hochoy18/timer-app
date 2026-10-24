#!/usr/bin/env bash
#
# 验打包出来的 FocusTimer.app。
#
# 这个脚本验的是**产物**：Info.plist 里该有的键、图标、可执行文件、签名状态、
# 隔离属性。配置层面的验在 `src-tauri/tests/bundle.rs`（`cargo test` 的一部分），
# 那个不需要产物，这个需要。
#
# 用法：
#   scripts/check-app-bundle.sh [路径/to/FocusTimer.app]
#
# 不给路径就查默认位置：src-tauri/target/release/bundle/macos/FocusTimer.app

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="${1:-$ROOT/src-tauri/target/release/bundle/macos/FocusTimer.app}"

FAILURES=0

ok()   { printf '  ✓ %s\n' "$1"; }
bad()  { printf '  ✗ %s\n' "$1"; FAILURES=$((FAILURES + 1)); }
info() { printf '    %s\n' "$1"; }

# 从 Info.plist 里读一个键（用 plutil，不自己解析 XML）。
plist_get() {
  /usr/libexec/PlistBuddy -c "Print :$1" "$APP/Contents/Info.plist" 2>/dev/null || true
}

printf '\nFocusTimer.app 验收\n'
printf '  路径：%s\n\n' "$APP"

# ── 包在不在 ──────────────────────────────────────────────────────────
if [[ ! -d "$APP" ]]; then
  bad "包不存在：$APP"
  printf '\n先跑一次打包：npm run tauri build\n\n'
  exit 1
fi
ok "包在"

PLIST="$APP/Contents/Info.plist"
BIN="$APP/Contents/MacOS/focus-timer"

# 该有的 bundle id 以 tauri.conf.json 为准（那里是唯一的真相；这个字面量
# 由 src-tauri/tests/bundle.rs 钉住）。脚本里再抄一份，改了配置两边就对不上。
EXPECTED_IDENTIFIER="$(/usr/bin/plutil -extract identifier raw "$ROOT/src-tauri/tauri.conf.json" 2>/dev/null || true)"
if [[ -z "$EXPECTED_IDENTIFIER" ]]; then
  bad "读不出 src-tauri/tauri.conf.json 里的 identifier"
  EXPECTED_IDENTIFIER="com.focustimer.app"
fi

# ── Info.plist ────────────────────────────────────────────────────────
if [[ ! -f "$PLIST" ]]; then
  bad "没有 Contents/Info.plist"
else
  ok "Info.plist 在"

  IDENTIFIER="$(plist_get CFBundleIdentifier)"
  if [[ "$IDENTIFIER" == "$EXPECTED_IDENTIFIER" ]]; then
    ok "CFBundleIdentifier = $IDENTIFIER"
  else
    bad "CFBundleIdentifier 是 '$IDENTIFIER'，期望 $EXPECTED_IDENTIFIER"
  fi

  NAME="$(plist_get CFBundleName)"
  if [[ -n "$NAME" ]]; then
    ok "CFBundleName = $NAME"
  else
    bad "CFBundleName 是空的"
  fi

  EXECUTABLE="$(plist_get CFBundleExecutable)"
  if [[ "$EXECUTABLE" == "focus-timer" ]]; then
    ok "CFBundleExecutable = $EXECUTABLE"
  else
    bad "CFBundleExecutable 是 '$EXECUTABLE'，期望 focus-timer"
  fi

  VERSION="$(plist_get CFBundleShortVersionString)"
  if [[ -n "$VERSION" ]]; then
    ok "CFBundleShortVersionString = $VERSION"
  else
    bad "没有 CFBundleShortVersionString"
  fi

  # LSUIElement = true：菜单栏应用，不在 Dock 里占位。
  LSUI="$(plist_get LSUIElement)"
  if [[ "$LSUI" == "true" || "$LSUI" == "1" ]]; then
    ok "LSUIElement = true（菜单栏应用，不占 Dock）"
  else
    bad "LSUIElement 是 '$LSUI'，期望 true —— 应用会在 Dock 里露脸"
  fi

  CATEGORY="$(plist_get LSApplicationCategoryType)"
  if [[ -n "$CATEGORY" ]]; then
    ok "LSApplicationCategoryType = $CATEGORY"
  else
    info "LSApplicationCategoryType 没写（不影响运行）"
  fi

  # 图标：CFBundleIconFile 或 CFBundleIconName 都算。
  ICON_FILE="$(plist_get CFBundleIconFile)"
  ICON_NAME="$(plist_get CFBundleIconName)"
  if [[ -n "$ICON_FILE" || -n "$ICON_NAME" ]]; then
    ok "图标键在：CFBundleIconFile='$ICON_FILE' CFBundleIconName='$ICON_NAME'"
  else
    bad "Info.plist 里没有图标键 —— 访达里会显示默认图标"
  fi

  # plist 指着哪个文件，那个文件就得到位。
  #
  # 只查"有图标键"不够：键在、值写错（比如 CFBundleIconFile=typo.icns），
  # 访达里照样给你看默认图标——键在而文件不在，是最容易漏过去的一种错。
  if [[ -n "$ICON_FILE" ]]; then
    ICON_BASE="${ICON_FILE%.icns}"
    if [[ -f "$APP/Contents/Resources/$ICON_FILE" \
       || -f "$APP/Contents/Resources/$ICON_BASE.icns" ]]; then
      ok "plist 指着的图标在包里：Contents/Resources/$ICON_FILE"
    else
      bad "CFBundleIconFile 指着 '$ICON_FILE'，但 Contents/Resources 里没有这个文件"
    fi
  fi
fi

# ── 图标文件 ──────────────────────────────────────────────────────────
if [[ -f "$APP/Contents/Resources/icon.icns" ]]; then
  SIZE="$(wc -c < "$APP/Contents/Resources/icon.icns" | tr -d ' ')"
  if [[ "$SIZE" -gt 10000 ]]; then
    ok "icon.icns 在（${SIZE} 字节）"
  else
    bad "icon.icns 太小（${SIZE} 字节），多半是空壳"
  fi
  if /usr/bin/file "$APP/Contents/Resources/icon.icns" | grep -q "Mac OS X icon"; then
    ok "icon.icns 是合法的 icns"
  else
    bad "icon.icns 不是合法的 icns 文件"
  fi
else
  bad "没有 Contents/Resources/icon.icns"
fi

# ── 可执行文件 ────────────────────────────────────────────────────────
if [[ -x "$BIN" ]]; then
  ok "可执行文件在"
  FILE_INFO="$(/usr/bin/file -b "$BIN")"
  info "$FILE_INFO"
  if [[ "$FILE_INFO" == *"Mach-O"* ]]; then
    ok "是 Mach-O 可执行文件"
  else
    bad "不是 Mach-O：$FILE_INFO"
  fi
else
  bad "没有可执行文件：$BIN"
fi

# ── 签名与 Gatekeeper ─────────────────────────────────────────────────
if codesign -dv "$APP" >/dev/null 2>&1; then
  SIGN="$(codesign -dv "$APP" 2>&1 | grep -E '^(Identifier|Signature|Authority)=' | tr '\n' ' ')"
  ok "有签名：$SIGN"
else
  info "没有签名（本地打包的正常状态）"
fi

if spctl -a -vv -t exec "$APP" >/dev/null 2>&1; then
  ok "Gatekeeper 认可这个包"
else
  info "Gatekeeper 不认（未签名 + 未公证的本地包，预期之中）"
  info "装到 /Applications 后按文档跑一次：xattr -cr /Applications/FocusTimer.app"
fi

# ── 隔离属性 ──────────────────────────────────────────────────────────
QUARANTINE="$(xattr -p com.apple.quarantine "$APP" 2>/dev/null || true)"
if [[ -n "$QUARANTINE" ]]; then
  info "有隔离属性（com.apple.quarantine）：$QUARANTINE"
  info "从浏览器 / 邮件 / 隔空投送拿来的包会带它，Gatekeeper 就是靠它拦的"
  info "装到 /Applications 后跑一次：xattr -cr /Applications/FocusTimer.app"
else
  ok "没有隔离属性（本地打出来的包本来就没有）"
fi

printf '\n'
if [[ "$FAILURES" -gt 0 ]]; then
  printf '%d 项没过。\n\n' "$FAILURES"
  exit 1
fi
printf '全过。\n\n'
