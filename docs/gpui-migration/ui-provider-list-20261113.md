# Provider list checkpoint — 2026-11-13

This checkpoint addresses the reported list-pane screenshot. It covers the
titlebar, search/action toolbar and two populated provider rows, including a
long title. It does not accept the whole migration or the presentation timing gate.

## Corrections

- Unselected hover now uses `surface2`; selection takes precedence and uses
  `accent-soft`. The previous blue hover made two rows look selected. Rows expose
  their actual selection to native accessibility and retain stable provider IDs.
- Background and title transitions follow the reference's 80 ms and 120 ms CSS
  `ease`. Associated-alpha interpolation avoids darkening a transparent row.
- Row corners are 8 px, title/subtitle line heights are 16/15 px. The previous
  shorter lines placed titles approximately 3 px too low. Reference row height,
  zero inter-row gap, icon column, padding and toolbar dimensions are preserved.
- The titlebar has its own 13 px content inset, correcting the previous 4 px
  logo/menu displacement. Its wordmark uses a 15 px line and 0.01em tracking;
  letters retain whole-word shaping/kerning rather than independently measured
  glyphs. This closes the measured wordmark width/height discrepancy.

## Matched reference evidence

`artifacts/desktop-parity/ui-list-20261113-final/report.json` contains 24 cases:
idle, unselected hover and selected hover, each at 960×640 and 1280×820, in
English/Chinese and light/dark. All 18 mapped bounds per case pass the unchanged
0.5 logical-pixel tolerance. Explicit numeric styles also pass. New scoped
appearance checks cover each row/icon background, all four corner radii and
title/subtitle/icon colors; all 24 cases pass. Selection and hover IDs are checked
separately from appearance.

The comparison waits for the reference hover transition to finish before
measuring computed colors. Earlier captures measured before that transition's
first frame and retained transparent computed color despite a hovered screenshot.
The `ui-list-20261113-before` and `ui-list-20261113-probe` rounds remain intact,
including the original geometry differences and intermediate measurement issue.
Final source/binary/image hashes, geometry and computed styles are in the report;
`index.html` provides paired images and adjustable overlays.

The captures use reduced motion and offscreen rendering. They establish mapped
geometry and final styles, not onscreen animation timing or pixel identity.

## Native interaction and validation

The freshly built `ui-list-native-20261113-r1/AIPass GPUI Validation.app` was
exercised at 960×640 in English light and Chinese dark modes. Moving the pointer
over the second row kept the first selected and used a neutral hover background.
Clicking the second row updated the details and AX selected state exclusively.
Moving to the toolbar preserved the new selection in the light-mode check.
The real-window screenshots and AX observations are recorded in this session.
The preview used synthetic data and did not connect to a production vault.

The new GPUI regression verifies an intermediate hover fade, its settled color,
selection remaining unchanged on hover and exclusive selection after a click.
The appearance-checker regression rejects blue hover, radius drift and missing
observations. `artifacts/desktop-parity/ui-list-validation-20261113/` retains the
macOS/Homebrew validation logs: workspace Clippy and parity-feature Clippy with
`-D warnings`, 115 native tests, formatting, comparison-tool tests and repository
checks. No commit, push, release or remote CI is part of this checkpoint.

The existing detail-pane gaps and the unresolved zero-time Metal callbacks in
[the presentation checkpoint](ui-performance-20261113.md) remain open. This list
comparison does not certify those surfaces or presentation performance.
