# OG font subset

`NotoSansCJKsc-Regular.subset.otf` is a build-only subset of Noto Sans CJK SC 2.004. It is used by `scripts/generate-og-images.mjs` so Chinese social images render consistently on macOS and Linux. It is not copied to the public site.

Source: <https://github.com/notofonts/noto-cjk/blob/Sans2.004/Sans/OTF/SimplifiedChinese/NotoSansCJKsc-Regular.otf>

The font is licensed under the SIL Open Font License 1.1; see `OFL.txt`.

When page metadata introduces a new character, add it to `og-glyphs.txt` and rebuild the subset from `apps/web`:

```bash
curl -L https://github.com/notofonts/noto-cjk/raw/refs/tags/Sans2.004/Sans/OTF/SimplifiedChinese/NotoSansCJKsc-Regular.otf \
  -o /tmp/NotoSansCJKsc-Regular.otf
pyftsubset /tmp/NotoSansCJKsc-Regular.otf \
  --text-file=assets/fonts/og-glyphs.txt \
  --output-file=assets/fonts/NotoSansCJKsc-Regular.subset.otf \
  --layout-features='*' \
  --name-IDs='*' \
  --name-legacy \
  --name-languages='*' \
  --notdef-glyph \
  --notdef-outline \
  --recommended-glyphs
```
