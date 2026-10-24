#!/usr/bin/env bash
#
# 把打包出来的 FocusTimer.app 装进 /Applications。
#
# 这一步会动系统目录，所以默认先问一句（-y 跳过）。
#
# 用法：
#   scripts/install-app.sh [-y] [--build]
#
#   -y       不用问，直接装
#   --build  先跑一次 npm run tauri build（产物不在时本来也会跑）
#
# 装完顺手做掉 Gatekeeper 那一件：去掉隔离属性（com.apple.quarantine）。
# 详见 docs/install.md。

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILT="$ROOT/src-tauri/target/release/bundle/macos/FocusTimer.app"
TARGET="/Applications/FocusTimer.app"

ASSUME_YES=0
DO_BUILD=0

usage() {
  # 这里用heredoc，不从文件头里 sed 出来：脚本顶部的注释一改行数，
  # sed 的行号就跟着作废，--help 印出一堆不相干的行。
  cat <<'EOF'
把打包出来的 FocusTimer.app 装进 /Applications。

用法：
  scripts/install-app.sh [-y] [--build]

  -y       不用问，直接装
  --build  先跑一次 npm run tauri build（产物不在时本来也会跑）

装完顺手做掉 Gatekeeper 那一件：去掉隔离属性（com.apple.quarantine）。
详见 docs/install.md。
EOF
}

for arg in "$@"; do
  case "$arg" in
    -y|--yes) ASSUME_YES=1 ;;
    --build)  DO_BUILD=1 ;;
    -h|--help) usage; exit 0 ;;
    *) printf '不认识的参数：%s\n' "$arg" >&2; exit 2 ;;
  esac
done

# cargo 在 ~/.cargo/bin。交互式 shell 一般已经把它加进 PATH 了（见
# scripts/setup-toolchain.sh），非交互式的跑法（比如从 CI 或别的脚本调）
# 就没有——这里补上，不改用户的配置。
export PATH="$HOME/.cargo/bin:$PATH"

# 应用的身份（bundle id）以 tauri.conf.json 为准——那里是唯一的真相。
# 脚本里再抄一份字面量，改了配置两边就对不上：装的时候会以为别人的应用
# 是自己家的（或者反过来，不敢覆盖自己家的）。
IDENTIFIER="$(/usr/bin/plutil -extract identifier raw "$ROOT/src-tauri/tauri.conf.json" 2>/dev/null || true)"
if [[ -z "$IDENTIFIER" ]]; then
  printf '读不出 src-tauri/tauri.conf.json 里的 identifier，不装。\n' >&2
  exit 1
fi

# ── 打包 ──────────────────────────────────────────────────────────────
if [[ "$DO_BUILD" == 1 || ! -d "$BUILT" ]]; then
  printf '打包中（npm run tauri build）…\n'
  (cd "$ROOT" && npm run tauri build)
fi

if [[ ! -d "$BUILT" ]]; then
  printf '打包没出东西：%s\n' "$BUILT" >&2
  exit 1
fi

# ── 装之前看一眼 ──────────────────────────────────────────────────────
if [[ -e "$TARGET" ]]; then
  EXISTING_ID="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' \
    "$TARGET/Contents/Info.plist" 2>/dev/null || true)"
  if [[ "$EXISTING_ID" != "$IDENTIFIER" ]]; then
    printf '%s 已经在那儿了，而且不是本应用（bundle id 是 %s）。\n' \
      "$TARGET" "${EXISTING_ID:-读不出}"
    printf '不覆盖它。换个名字再说。\n' >&2
    exit 1
  fi
  printf '注意：%s 已经装过了，这次会覆盖它。\n' "$TARGET"
fi

printf '\n要装的是：%s\n' "$BUILT"
printf '装到：      %s\n\n' "$TARGET"

if [[ "$ASSUME_YES" == 0 ]]; then
  read -r -p '装吗？[y/N] ' reply
  if [[ ! "$reply" =~ ^[Yy]$ ]]; then
    printf '不装了。\n'
    exit 0
  fi
fi

# ── 装 ────────────────────────────────────────────────────────────────
if [[ -e "$TARGET" ]]; then
  rm -rf "$TARGET"
fi
cp -R "$BUILT" "$TARGET"

# ── Gatekeeper：去掉隔离属性 ──────────────────────────────────────────
#
# 为什么这一步在这：
#
# macOS 给"从别处拿来的"应用打上 com.apple.quarantine 属性，Gatekeeper 靠它
# 决定要不要拦。本地打出来的包本来没有这个属性——但只要这个包经过一次拷贝
# （从别的机器拷来、或者从压缩包里解出来），属性就跟着来了，Gatekeeper 就
# 会拦：双击弹"无法打开，因为无法验证开发者"。
#
# 本期不做签名与公证（spec §2），所以没有别的办法能让 Gatekeeper 放行；
# 这一行就是那一步。它只影响这个应用包，不改系统设置。
xattr -cr "$TARGET" 2>/dev/null || true

if xattr -p com.apple.quarantine "$TARGET" >/dev/null 2>&1; then
  printf '\n⚠ 隔离属性还在，Gatekeeper 可能还会拦。\n'
else
  printf '\n✓ 已装好，隔离属性已清掉。\n'
fi

cat <<EOF

下一步：
  · 双击 ${TARGET}（或 open ${TARGET}）
  · 托盘图标出现即表示起来了；点它开面板，右键它退出
  · 第一次运行会弹一次引导，在引导里点「启用通知」——macOS 会问一句要不要
    允许通知，要点「允许」，否则提醒会静默失败
  · 想开机自启：系统设置 → 通用 → 登录项 → 把 FocusTimer.app 加进去
    （本期不做自动自启，这是手动加登录项）

卸载：
  rm -rf $TARGET
  rm -rf ~/Library/Application\ Support/com.focustimer.app
EOF
