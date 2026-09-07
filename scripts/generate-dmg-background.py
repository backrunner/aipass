#!/usr/bin/env python3
"""Render the Retina DMG background (apps/desktop/src-tauri/dmg/background.tiff).

The TIFF contains 1x and 2x representations with the same 660x400 point size.
Finder selects the Retina representation without enlarging or cropping the
layout. Render at 4x, then downsample each representation. Requires macOS and
Pillow; tiffutil checks that both representations have matching logical sizes.
"""

from pathlib import Path
import subprocess
from tempfile import TemporaryDirectory

from PIL import Image, ImageDraw, ImageFilter, ImageFont

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
        / "apps/desktop/src-tauri/dmg/background.tiff"
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

    def centered_text(text, y, size, color, font_path):
        font = ImageFont.truetype(font_path, size * S)
        d.text((W * S / 2, y * S), text, font=font, fill=color, anchor="mm")

    latin_font = "/System/Library/Fonts/Helvetica.ttc"
    chinese_font = "/System/Library/Fonts/STHeiti Medium.ttc"
    centered_text("AIPass", 52, 28, (30, 41, 59), latin_font)
    centered_text("Double-click AIPass to install", 284, 19, (30, 41, 59), latin_font)
    centered_text("双击 AIPass 即可安装", 313, 17, (30, 41, 59), chinese_font)
    centered_text("Or drag to Applications  /  或拖入「应用程序」", 340, 13, (100, 116, 139), chinese_font)

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

    img = img.convert("RGB")
    with TemporaryDirectory(prefix="aipass-dmg-background-") as temp_dir:
        representations = []
        for scale in (1, 2):
            path = Path(temp_dir) / f"background@{scale}x.tiff"
            img.resize((W * scale, H * scale), Image.LANCZOS).save(
                path, compression="tiff_lzw", dpi=(72 * scale, 72 * scale)
            )
            representations.append(str(path))
        subprocess.run(
            ["/usr/bin/tiffutil", "-cathidpicheck", *representations, "-out", str(out_path)],
            check=True,
        )
    print(f"Wrote {out_path} ({W}x{H} points; 1x + 2x Retina)")


if __name__ == "__main__":
    main()
