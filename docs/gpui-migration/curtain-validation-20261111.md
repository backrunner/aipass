# Native lock/unlock curtain validation

Work began on 2026-11-11 and continued on 2026-11-12. This is development
evidence, **not acceptance of the migration or this component**. The baseline
is `65a1f1f5a5e1b51311132659f9b972131c8ea551`; the original
`UnlockTransition.svelte` and its styles remain unchanged.

## Implementation and lifecycle

`apps/desktop-native/src/curtain.rs` paints native GPUI paths and quads. The
curtain is above the desktop chrome and does not consume pointer events. It
uses the reference's two cubic wave paths, direction, overlap, color mix and
1200 ms timing, with cover at 540 ms and the second phase starting at 660 ms.
WebKit's 1/64 px layout units and the flipped SVG's device-pixel transform
origin are accounted for. No reference image is an application asset.

The default GPUI Metal gradient adds RGB and alpha noise. This implementation
instead paints native solid row runs for the CSS sRGB ramp to its Oklab-mixed
endpoint. WebKit's own quantization still differs; this is not pixel parity.

Lock starts its Agent request and animation together. In the synthetic preview,
the workspace remains mounted until cover, then authentication receives focus.
Repeated lock does not restart the clock. A replacement transition invalidates
the former subscription/generation, so its late cover/done events cannot reopen
the workspace or finish the newer transition. Reduced motion emits the two
milestones without drawing the curtain. The existing session controller's
failed-lock behavior still differs from the reference and is not accepted.

Tests cover once-only milestones, skipped frames, reduced motion, replacement
timelines, repeated lock, superseded unlock callbacks and mounted focus routing.
GPUI test events establish controller behavior, not physical display timing.

## Capture contract

`tests/desktop-parity/capture-motion.mjs` mounts the original Svelte transition
in Playwright WebKit and pauses its CSS animation at explicit times. The native
fixture freezes only its clock; it uses the same production curtain painter.
These isolated transition fixtures do not prove the entire application's
transition, Agent timing, live CSS scheduling or native window presentation.

Full rounds cover 960×640 and 1280×820 at scale 2, light/dark, lock/unlock,
normal motion at 0, 60, 135, 270, 405, 539, 540, 600, 660, 795, 930, 1065 and
1200 ms, and reduced motion at 0 and 1200 ms: 120 image pairs per round.
Smoke rounds use 960×640 with normal samples at 270, 540 and 930 ms plus the
two reduced-motion endpoints: 20 pairs each.

Every round preserves the reference and candidate PNGs, exact RGBA difference
images, layout/transform data, source hashes and executable. Full rounds also
retain a verified source archive. Existing directories cannot be overwritten.
Missing capture data and even one differing channel fail the comparison.

Earlier rounds exposed capture differences as well as rendering differences:

- The first three smoke rounds composited WebKit over its default background.
- `curtain-rgba-full-20261111` retains WebKit transparency. GPUI still used an
  opaque window background, which made the empty corners opaque black.
- `curtain-transparent-regression-20261111` uses a transparent GPUI window
  matching the reference configuration. It also corrects the flipped wave's
  transform origin. GPUI's Metal readback retains premultiplied RGB.
- `curtain-straight-alpha-20261111` writes standard straight-alpha PNGs from
  that readback and retains the unchanged readback as `*.metal-rgba.png`.
  This encoding conversion preserves alpha and opaque pixels. It does not
  resize, mask, blend over a background or relax comparison tolerance.

Do not compare the aggregate metrics across different capture contracts as
though each decrease were a presentation improvement. Rounded-edge coverage,
gradient quantization and path rasterization still need correction. In
particular, GPUI rectangular child clipping does not reproduce all of the
reference's nested rounded clipping.

## Completed comparisons

| Round | Pairs | Pixel-exact passes | Mean channel error (0–255) |
| --- | ---: | ---: | ---: |
| `curtain-initial-20261111` | 20 | 0 | 0.382902 |
| `curtain-correction-20261111` | 20 | 0 | 0.162330 |
| `curtain-srgb-correction-20261111` | 20 | 0 | 0.126349 |
| `curtain-rgba-full-20261111` | 120 | 0 | 0.133216 |
| `curtain-transparent-regression-20261111` | 120 | 0 | 0.127767 |
| `curtain-straight-alpha-20261111` | 120 | 0 | 0.127524 |

All six reports are complete, with 420 pairs total. The final complete round
retains executable SHA-256
`d42f63f5a58a26b484dfe39ce58a8f9f323ce7a0499c14eac9aa8bbb08269841`.
Its maximum recorded block/wave geometry difference is 0.00006104 logical px.
That numerical agreement does not override failed pixel comparisons.

The subsequent lint correction uses the stable array-chunk iterator for PNG
encoding and scopes its mutable image binding to macOS. It does not change the
curtain painter. The final reduced-motion PNG is byte-identical to the same
case in the last full round; its native window observations are below.

## Live window observation

The first native lock inspection used executable
`68d6a241a4ade3055884716bd727b5f6b6d505ee3142cfeb186a0d5b2f3343a0`.
Computer use captured the blue cover in the actual 960×640 window, followed by
the password screen. A synthetic character reached the focused password field;
it was cleared, and Command-Q exited. No vault was opened.

`artifacts/desktop-parity/native-curtain-live-20261111.json` contains 175 render
observations. Cover fired at 545.843 ms and done at 1205.299 ms; the median gap
was 6.944 ms and the maximum gap was 8.371 ms. These are application render-loop
timestamps, **not compositor presentation timestamps or a Tauri cadence
comparison**. Screenshots were inspected in the computer-use conversation;
they are not archived paired native screenshots.

The final build, SHA-256
`0780704de2881e600c02c60cf127780bc24a7067f6ff9d1b0d106b4dc507a007`,
was inspected again with normal and reduced motion. Normal motion produced a
visible cover, then the focused password screen, and exited with Command-Q.
`native-curtain-final-20261112.json` records 174 render observations, cover at
543.802 ms, done at 1203.397 ms, median gap 6.942 ms and maximum gap 7.906 ms.
The reduced-motion run showed the password screen immediately and accepted a
synthetic character in its focused input. Its trace,
`native-curtain-reduced-20261112.json`, contains one observation at 1.744 ms,
with both milestones emitted and `reduced: true`. These observations still do
not establish pixel parity with the original native application.

Validation passed on macOS Homebrew Rust: 22 native tests (with the PNG test
rerun after the lint correction), the full workspace Clippy command with
`--all-targets -- -D warnings`, the additional native `parity-render` Clippy
check, formatting, repository checks, four comparator tests and the original
desktop's Svelte typecheck. The revised pitfalls skill validator also passed.

## Acceptance boundary

The strict report remains failed. No inventory entry is accepted. Hero motion,
popup/dialog animations, populated panels, production Agent interactions,
lock failure, tray, startup/update and full native paired capture remain open.
No commit, push or release has been performed.
