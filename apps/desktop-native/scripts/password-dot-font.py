# /// script
# dependencies = ["fonttools==4.61.1"]
# ///
"""Build AIPass's original circular password-mask glyph, not a vendor font.

Run: uv run apps/desktop-native/scripts/password-dot-font.py
The input editor retains its secure masking, selection and clipboard rules.
Only U+2022's presentation changes to the reference's 13px disc geometry.
"""
from math import cos, sin, pi
from pathlib import Path
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

builder = FontBuilder(1000, isTTF=True)
builder.setupGlyphOrder([".notdef", "space", "disc"])
# GPUI requires an m glyph for editor measurements. This font is selected only
# for nonempty masked values, so this blank measurement glyph is never shown.
builder.setupCharacterMap({32: "space", ord("m"): "space", 0x2022: "disc", 0x25CF: "disc"})
empty = TTGlyphPen(None).glyph()
pen = TTGlyphPen(None)
cx, cy, radius = 312.5, 339, 213
pen.moveTo((cx + radius, cy))
for i in range(8):
    end = -(i + 1) * pi / 4
    mid = -(i + 0.5) * pi / 4
    control_radius = radius / cos(pi / 8)
    pen.qCurveTo((cx + control_radius * cos(mid), cy + control_radius * sin(mid)),
                 (cx + radius * cos(end), cy + radius * sin(end)))
pen.closePath()
builder.setupGlyf({".notdef": empty, "space": empty, "disc": pen.glyph()})
builder.setupHorizontalMetrics({".notdef": (625, 0), "space": (625, 0), "disc": (625, 100)})
builder.setupHorizontalHeader(ascent=952, descent=-213)
builder.setupNameTable({"familyName": "AIPass Password Dots", "styleName": "Regular",
                       "uniqueFontIdentifier": "AIPassPasswordDots-Regular-1",
                       "fullName": "AIPass Password Dots Regular", "psName": "AIPassPasswordDots-Regular",
                       "version": "Version 1.0", "copyright": "Original geometric artwork by AIPass contributors"})
builder.setupOS2(sTypoAscender=952, sTypoDescender=-213, usWinAscent=952, usWinDescent=213)
builder.setupPost()
builder.setupMaxp()
builder.font["head"].created = builder.font["head"].modified = 2082844800
builder.font.recalcTimestamp = False
output = Path(__file__).resolve().parents[1] / "assets" / "password-dots.ttf"
builder.save(output)
print(output)
