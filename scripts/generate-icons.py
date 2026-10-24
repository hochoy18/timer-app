#!/usr/bin/env python3
"""生成 FocusTimer 的全部图标资源。

产物：
  src-tauri/icons/32x32.png
  src-tauri/icons/128x128.png
  src-tauri/icons/128x128@2x.png
  src-tauri/icons/icon.icns        (macOS 打包用)
  src-tauri/icons/icon.ico         (Windows 打包用，本期用不到但配置里列了)
  src-tauri/icons/tray.png         (菜单栏常态，template image)
  src-tauri/icons/tray-alert.png   (菜单栏提醒态)

菜单栏图标是 macOS 的 template image：纯黑 + 透明背景，由系统按菜单栏明暗
反色渲染。提醒态刻意不做成 template——它是彩色的，才能一眼看出"在报警"。
"""

from __future__ import annotations

import os
import shutil
import subprocess
import tempfile

from PIL import Image, ImageDraw

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ICON_DIR = os.path.join(ROOT, "src-tauri", "icons")

BLUE_TOP = (99, 132, 255)
BLUE_BOTTOM = (49, 78, 191)
WHITE = (255, 255, 255)
ALERT_RED = (220, 38, 38)
TRAY_BLACK = (0, 0, 0)

SS = 4  # 超采样倍数，画完再缩小以获得平滑边缘


def _rounded_app_icon(size: int) -> Image.Image:
    """应用图标：圆角方块 + 白色表盘 + 指向 12 点的指针（读作"一小时"）。"""
    n = size * SS
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    # 竖直渐变底
    grad = Image.new("RGBA", (1, n))
    for y in range(n):
        t = y / max(1, n - 1)
        grad.putpixel(
            (0, y),
            tuple(
                int(BLUE_TOP[i] + (BLUE_BOTTOM[i] - BLUE_TOP[i]) * t) for i in range(3)
            )
            + (255,),
        )
    grad = grad.resize((n, n))

    mask = Image.new("L", (n, n), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        (0, 0, n - 1, n - 1), radius=int(n * 0.225), fill=255
    )
    img.paste(grad, (0, 0), mask)

    d = ImageDraw.Draw(img)
    cx = cy = n / 2
    r = n * 0.33
    ring = max(2, int(n * 0.062))
    d.ellipse((cx - r, cy - r, cx + r, cy + r), outline=WHITE, width=ring)

    # 指针：一根指向 12 点，一根指向 1 点
    hand = max(2, int(n * 0.055))
    d.line((cx, cy, cx, cy - r * 0.74), fill=WHITE, width=hand)
    d.line((cx, cy, cx + r * 0.46, cy - r * 0.30), fill=WHITE, width=hand)
    dot = n * 0.045
    d.ellipse((cx - dot, cy - dot, cx + dot, cy + dot), fill=WHITE)

    return img.resize((size, size), Image.LANCZOS)


def _tray_icon(size: int, color, with_dot: bool) -> Image.Image:
    """菜单栏图标：纯黑（template）或彩色的表盘。

    菜单栏只显示 22pt，所以形状必须极简：外圈 + 指针。提醒态额外加一个角标。
    """
    n = size * SS
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    cx = cy = n / 2
    r = n * 0.42
    ring = max(2, int(n * 0.115))
    d.ellipse((cx - r, cy - r, cx + r, cy + r), outline=color, width=ring)

    hand = max(2, int(n * 0.11))
    d.line((cx, cy, cx, cy - r * 0.66), fill=color, width=hand)

    if with_dot:
        # 右下角一个实心圆点，和常态图标区分开。
        dr = n * 0.17
        dx, dy = cx + r * 0.66, cy + r * 0.66
        d.ellipse((dx - dr, dy - dr, dx + dr, dy + dr), fill=color)

    return img.resize((size, size), Image.LANCZOS)


def main() -> None:
    os.makedirs(ICON_DIR, exist_ok=True)

    app = _rounded_app_icon(1024)
    app.resize((32, 32)).save(os.path.join(ICON_DIR, "32x32.png"))
    app.resize((128, 128)).save(os.path.join(ICON_DIR, "128x128.png"))
    app.resize((256, 256)).save(os.path.join(ICON_DIR, "128x128@2x.png"))
    app.save(
        os.path.join(ICON_DIR, "icon.ico"),
        sizes=[(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )

    _tray_icon(44, TRAY_BLACK, with_dot=False).save(
        os.path.join(ICON_DIR, "tray.png")
    )
    _tray_icon(44, ALERT_RED, with_dot=True).save(
        os.path.join(ICON_DIR, "tray-alert.png")
    )

    # macOS .icns：用 iconutil 从 iconset 目录合成
    if shutil.which("iconutil"):
        with tempfile.TemporaryDirectory() as tmp:
            iconset = os.path.join(tmp, "icon.iconset")
            os.makedirs(iconset)
            for base, size in [
                ("icon_16x16", 16),
                ("icon_16x16@2x", 32),
                ("icon_32x32", 32),
                ("icon_32x32@2x", 64),
                ("icon_128x128", 128),
                ("icon_128x128@2x", 256),
                ("icon_256x256", 256),
                ("icon_256x256@2x", 512),
                ("icon_512x512", 512),
                ("icon_512x512@2x", 1024),
            ]:
                app.resize((size, size)).save(os.path.join(iconset, f"{base}.png"))
            subprocess.run(
                ["iconutil", "-c", "icns", iconset, "-o", os.path.join(ICON_DIR, "icon.icns")],
                check=True,
            )
            print("✓ icon.icns")
    else:
        print("! 找不到 iconutil，跳过 .icns（仅影响 macOS 打包）")

    print(f"✓ 图标已生成到 {ICON_DIR}")


if __name__ == "__main__":
    main()
