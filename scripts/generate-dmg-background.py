#!/usr/bin/env python3
"""Render the DMG installer window background (apps/desktop/src-tauri/dmg/background.png).

Finder renders DMG backgrounds at 1 image pixel = 1 point, so the output is
exactly 660x400 to match the windowSize in tauri.conf.json. Everything is drawn
at 4x and downscaled for antialiasing. Requires Pillow.
"""

from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

W, H = 660, 400
S = 4  # supersampling factor

# Brand palette (packages/ui/src/styles/base.scss light theme + app icon gradient)
ACCENT = (37, 99, 235)  # --accent: #2563eb
ICON_BLUE_TOP = (55, 123, 248)  # #377bf8
ICON_BLUE_BOTTOM = (11, 80, 209)  # #0b50d1
BORDER_STRONG = (196, 202, 216)  # --border-strong: #c4cad8

APP_CENTER = (180, 170)
APPS_CENTER = (480, 170)


def lerp(a, b, t):
    return tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3))


def vertical_gradient(size, top, bottom):
    w, h = size
    col = Image.new("RGB", (1, h))
    for y in range(h):
        col.putpixel((0, y), lerp(top, bottom, y / (h - 1)))
    return col.resize((w, h))


def main():
    out_path = (
        Path(__file__).resolve().parent.parent
        / "apps/desktop/src-tauri/dmg/background.png"
    )
    out_path.parent.mkdir(parents=True, exist_ok=True)

    img = vertical_gradient((W * S, H * S), (255, 255, 255), (239, 242, 249)).convert("RGB")

    # Soft brand-blue glow behind the arrow area
    glow = Image.new("RGBA", img.size, (0, 0, 0, 0))
    gd = ImageDraw.Draw(glow)
    gd.ellipse(
        [150 * S, 30 * S, 510 * S, 320 * S],
        fill=ICON_BLUE_TOP + (22,),
    )
    glow = glow.filter(ImageFilter.GaussianBlur(70 * S))
    img = Image.alpha_composite(img.convert("RGBA"), glow)

    d = ImageDraw.Draw(img)

    # Arrow from the app icon to the Applications alias
    y = 170 * S
    x0, x1 = 258 * S, 388 * S
    shaft = 8 * S
    head_len, head_half = 22 * S, 15 * S
    d.line([x0, y, x1 - head_len + 2 * S, y], fill=ACCENT + (255,), width=shaft)
    d.ellipse([x0 - shaft / 2, y - shaft / 2, x0 + shaft / 2, y + shaft / 2], fill=ACCENT + (255,))
    d.polygon(
        [(x1, y), (x1 - head_len, y - head_half), (x1 - head_len, y + head_half)],
        fill=ACCENT + (255,),
    )

    img = img.convert("RGB").resize((W, H), Image.LANCZOS)
    img.save(out_path, dpi=(72, 72))
    print(f"Wrote {out_path} ({W}x{H})")


if __name__ == "__main__":
    main()
