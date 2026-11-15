# Controls and renderer audit — 2026-11-14

This checkpoint follows [subscription overview acceptance](ui-acceptance-20261113.md).
It uses the frozen Svelte/WebKit reference and the native Metal renderer with
synthetic data. It is an audit of the GPUI migration checkout, not a production
release or a declaration that every migration surface has achieved parity.

## Corrections

- Opacity now composites a complete subtree once. Disabled white text no longer
  blends through its own blue background, and nested opacity and translucent
  siblings retain independent composition. GPUI core, Apple Metal and WGPU use
  the same opacity contract.
- Disabled buttons clear hover/pressed presentation. Inputs, selects, switches,
  icon buttons and primary buttons use their reference disabled alpha and focus
  treatment. SVG color inheritance is opt-in; the runtime close icon now paints
  and follows its hover color.
- CSS shadow blur is converted to Gaussian sigma (half the CSS radius). Popup,
  dialog and switch shadows previously used the CSS radius directly.
- Runtime inputs/textareas, checkboxes, switches, tabs and select menus match
  measured padding, line heights, corners, borders and focus rings. Tab text uses
  shaped advances without accumulating integer rounding. The checkbox uses the
  macOS WebKit light/dark treatment and a dedicated check path.
- Runtime pickers retain their contents through exit, disable closing items,
  and use the reference 160 ms enter / 120 ms exit transitions with a top-left
  transform origin. Visual translation and scale carry matching interaction
  transforms. A live computed-style check found that Add field has no menu
  animation in the frozen reference; it keeps immediate appearance/dismissal,
  while its advanced-field disclosure animates independently.
- A closed, uninitialized menu/modal returns a settled frame before constructing
  transitions. Previously it repeatedly initialized enter values, animated to
  different exit values and reset, continuously requesting frames.
- Dropdown opening focuses the selected item. Opening from two minutes, moving
  Down and releasing Enter now chooses five minutes and restores the trigger.
  The old path started before the first item instead of the selected value.

Own implementations remain split by responsibility. `menu/motion.rs` owns menu
motion; `subscription_ui/motion/idle_tests.rs` isolates modal idle regressions.
The repository size gate passes with 877 source files and 19 existing no-growth
exceptions. Pinned upstream renderer files retain their licensed structure under
the user's third-party exception. Origins, hashes and reviewable patches are in
`vendor/upstream.json`, `vendor/patches/` and `vendor/README.md`.

## Automated visual evidence

The final matrix uses 960×640 and 1280×820, English and Simplified Chinese, and
light/dark themes. Reports retain native/reference PNGs, binary/source SHA-256
fingerprints and side-by-side review pages. These are offscreen Metal renders;
reference interactions are Playwright events and native control events are
injected into the application's test window. They do not prove OS input delivery
or displayed frame pacing.

All 400 primary comparison cases passed their declared gates, with a further
16 affected menu endpoint cases after the final motion correction:

| Matrix | Cases | Maximum mapped geometry delta | Gate |
| --- | ---: | ---: | ---: |
| Control states | 120 | 0.40625 px | 0.5 px |
| Subscription states | 208 | 0.5625 px | 0.75 px |
| Overview | 16 | 0.703125 px | 0.75 px |
| Provider lists | 24 | 0.296875 px | 0.5 px |
| Add field / advanced forms | 32 | 0.5 px | 0.5 px |
| Final menu endpoint recheck | 16 | 0 px | 0.5 px |

The selected control pixel samples differ by at most 1/255, below the 4/255 gate.
This metric samples specific borders, focus rings and foregrounds, not every pixel
in each screenshot. Review pages:

- [Controls](../../artifacts/desktop-parity/ui-controls-20261114-final/index.html)
- [Subscriptions](../../artifacts/desktop-parity/ui-subscriptions-20261114-final/index.html)
- [Overview](../../artifacts/desktop-parity/ui-overview-20261114-final/index.html)
- [Provider lists](../../artifacts/desktop-parity/ui-provider-list-20261114-final/index.html)
- [Add field / advanced forms](../../artifacts/desktop-parity/ui-fields-20261114-motion-final/index.html)

The main matrix binary is `f38f6142fa41a9d40bdf357c1833f392b67b081bfe68a772286a960d45fb0d7f`.
The final Add field and [menu endpoint](../../artifacts/desktop-parity/ui-menus-20261114-final/index.html)
rechecks use `bb13dc3a99d2fef2b1b56f60f399ac48f0180546159f6ac46d70dc4af8efe192`.
Only `menu/motion.rs` and `providers/optional_fields.rs` changed between them:
correcting the select transform origin and retaining Add field's immediate menu.
These changes do not change settled/reduced-motion subscription, overview or list
layout. Their affected endpoints and Add field matrix were rerun; final Rust checks
use the later source. `visual-summary.json` records this distinction and lists
later source differences instead of presenting all captures as one binary.
Computed reference animation evidence is in `reference-menu-motion.json` and
`reference-menu-settled.json` in the validation directory.

Control cases cover focused input/textarea, select hover/open/item-hover/disabled,
switch keyboard focus, disabled primary hover, ghost hover, checkbox empty/checked/
keyboard focus, close-icon hover/keyboard focus, and tab keyboard focus. The control
geometry gate is 0.5 logical px, with selected pixel channels within 4/255. Menu
bounds and label/icon foreground samples are checked separately. Checkbox glyph
bounds allow 0.5 px and thresholded ink area allows 25%; this is not a claim that
font/icon antialiasing is pixel-identical. Broad subscription geometry is now held
to 0.75 px, replacing the earlier 3 px gate that admitted 52 mappings above 0.75 px.

## Logic and GPU evidence

Final Homebrew Rust/macOS checks are in
`artifacts/desktop-parity/ui-controls-20261114-validation/`:

- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- Native all-targets Clippy with `--features parity-render`: pass.
- `cargo test -p aipass-desktop-native --features parity-render`: 164 tests pass
  (68 library, 92 UI/state, four integration; subprocess repetitions excluded).
- `cargo build --workspace` and native Release build: pass.
- `pnpm repo:check`: pass. Parity comparison, presentation-metric and vendor
  provenance tests: 20 pass. The edited revise skill validates.

The native suite includes four actual offscreen Metal regressions at 1×/2×:
current-frame backdrop blur without stale background reuse, complete-subtree
scaling, nested opacity, and independent translucent siblings. Input tests cover
transformed/clipped/cached hit regions and IME geometry. Motion tests cover exit,
reversal, reduced motion and zero frame requests both before opening and after
closing. Navigation tests exercise selected-value opening via pointer and
keyboard, and Proxy URL → Username → Password Tab order. Button activation tests
send both key-down and key-up events.

The standalone WGPU crate passes 39 tests on macOS, including shader/layout and
WGSL/WebGL coverage. This does not establish rendering on Windows or Linux GPUs.
The complete workspace Rust test run and Node install/license/lint/typecheck/test/
build gates passed earlier in this audit. Their logs are in
`artifacts/desktop-parity/ui-controls-20261113-r1/`; affected native tests, builds,
Clippy and repository/provenance checks were rerun after the final native changes.
Node production sources and dependency inputs did not change afterward.

## Native interaction and actual presentation

Native automation intermittently showed a stale image while accessibility values
changed. A temporary, preview-only navigation diagnostic recorded
`WindowVisibility::Hidden`, including after a Raise action. AX-readable content
and a window screenshot therefore did not establish a currently presented frame.
The diagnostic log is
`artifacts/desktop-parity/ui-controls-20261114-visibility/events.log`; the temporary
source instrumentation was removed before final checks and captures.

The observed CPU sample fell from roughly 88% to 1% after the closed-transition
fix, but the hidden-window finding means those samples are **not accepted as a
visible-window idle benchmark**. The zero-frame-request regression establishes
the scheduling correction independently.

After presentation sampling, a fresh final fixture passed the following native
CUA checks at 960×640, with the live AX tree and screenshots confirming outcomes:

- English/light quota menu starts at two minutes; Down + Enter selects five
  minutes and returns focus to the trigger.
- Editing the synthetic proxy URL, Tab to Username, paste, Tab to Password, then
  Tab + Space reaches and checks the clear-authentication control.
- Cancel closes settings; Add opens its form. Add field presents nine available
  fields in the reference groups. Notes accepts Chinese paste, Return and a
  second line. Advanced expands its controls; collapse removes them from AX.
- Chinese/dark Tab navigation reaches the checkbox and Space toggles it with the
  dark focus treatment. Balance scrolls to its final controls; its textarea
  accepts synthetic JSON and displays the corrected focus ring.

These fresh passes supersede the earlier failed automation attempts for these
flows, without erasing them or establishing physical IME composition. Drafts were
cancelled and the fixture app exited; no settings save, credential operation,
webhook, credit reset or vendor login was submitted. Exact bundle identity and
observations are in `native-observations.json`.

The earlier presentation matrix predates this opacity renderer change and is
not reused. A fresh Release matrix passed all eight cases, after capture/build
jobs stopped. Each case runs for 18 seconds with a three-second warmup and actual
`MTLDrawable.presentedTime`. Geometry auditing is disabled in that binary.

| Case | Steady FPS | P95 interval (ms) | Maximum interval (ms) |
| --- | ---: | ---: | ---: |
| locked-960-light | 143.06 | 6.95 | 20.84 |
| locked-960-dark | 142.96 | 6.95 | 20.84 |
| locked-1280-light | 143.03 | 6.95 | 20.84 |
| locked-1280-dark | 143.30 | 6.95 | 13.89 |
| reset-960-light | 143.30 | 6.95 | 13.89 |
| reset-960-dark | 143.30 | 6.95 | 20.84 |
| reduced-960-light | 1.95 | 527.91 | 527.91 |
| reduced-1280-dark | 1.96 | 520.97 | 527.91 |

All cases have zero invalid presentation callbacks and zero stale unpresented
frames. Reduced-motion cases preserve intermittent updates such as caret blinking.
The sampled app/window were active/key, without a resignation during sampling.
Frames in flight at deliberate termination remain recorded separately. This is
current evidence for the sampled auth/background paths, not universal zero missed
frames or measured frame pacing for every modal/menu transition.

Evidence: `artifacts/desktop-parity/ui-controls-20261114-performance-final/report.json`.
The Release binary SHA-256 is `394fc5001229c107841f97b78ad01920623b52c1344a1253ed67cea98107892f`.
The separate successful preflight is retained in `ui-controls-20261114-performance-probe/`.

This checkpoint does not certify every intermediate animation frame, physical IME
composition, all menus' visual states, or production auth/proxy/webhook operations.
The new menu motion applies to runtime pickers. Source review identified
remaining animation parity work for provider-type, credential, workspace and
integration popovers; they still use immediate Popover appearance/dismissal.
Their detailed state-pixel matrices are also not part of the 120 control cases.
The subsequent [menu follow-up](ui-menus-20261114.md) addresses these motion paths
and records their later evidence and remaining functional gaps.
Add field intentionally has no menu animation, matching the computed reference. No remote CI, production app replacement, DMG/runtime update
gate, commit, push or release was performed.
