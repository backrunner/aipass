# Native presentation and consistency checkpoint — 2026-11-13

Follow-up: the new renderer's [native acceptance checkpoint](ui-acceptance-20261113.md)
records eight passing presentation cases. The failed zero-time callback results
below remain the historical evidence for the earlier renderer.

This checkpoint does **not** accept the complete UI or certify zero dropped
frames. Measurements use synthetic previews on local macOS, with no release,
production vault or update operation.

## Corrections and visual evidence

- Auth scroll fades had an actual painting defect: opacity on Canvas did not
  reach its custom image-paint callback. A parent Div now establishes opacity.
  `ui-fade-20261113-r1` contains 16 reset/top and reset/bottom pairs across both
  sizes, locales and themes. All mapped geometry checks pass. Review of the
  corrected minimum-window image confirms that the opaque strip is gone.
- Provider add forms now match the reference's advanced heading line height,
  weight, separator and add-field button height. The outer bordered dialog and
  inner form each have their own height constraint, matching the reference's
  clipping; the former global one-pixel positioning offset is removed.
  `ui-provider-metrics-20261113-r3` had 16/16 mapped geometry and numeric-style
  passes. The subsequent frozen-checker round, `ui-provider-metrics-20261113-final`,
  has 15/16 geometry passes and 16/16 numeric-style passes: in the large Chinese
  dark advanced form, the first three fields and their labels differ vertically
  by one pixel at the scroll end. This repeatability discrepancy remains open;
  the earlier passing round does not supersede it.
- Real-window review found another defect that rectangle measurements did not
  detect: the inner form's white background protruded through the outer rounded
  corners. The inner form now has its own 13 px radius inside the 14 px bordered
  frame. `ui-provider-corners-20261113-min` and `ui-provider-corners-20261113-large`
  contain four additional geometry/numeric-style passes after that correction.
- The reference field's 4 px label/control gap maps to the native inner column.
  Outer padding is still checked on the outer row; the unrelated horizontal
  remove-button gap cannot substitute for it. Missing explicit style owners
  fail validation.

The provider `r1` build failure (an observation wrapper placed before a button
handler) and intermediate `r2` one-pixel natural-height differences remain in
their original rounds. Current numeric checks do not accept badge shaping,
glass compositing, dialog motion, unmapped inline editors or other inventory gaps.

## Real-window interaction

The earlier computer-control blocker did not recur. The actual `ui-native-20261113-r5`
window at 960×640 responded to password input/reveal, forgot-password navigation,
opening the reset explanation, scrolling to the final cancel button and returning
through recovery to unlock. The disabled reset action was not submitted. This is
native interaction evidence for those actions, separate from screenshots.

The rebuilt `ui-native-20261113-r6` window confirmed gradual scroll fades at both
ends in Chinese dark mode at 960×640. Cancel returned to recovery. The large add
dialog responded to advanced expansion, scrolling and Escape; its corner defect
was then fixed. The final `ui-native-20261113-r7` bundle visibly has rounded inner
backgrounds at both 960×640 and 1280×820, and Escape removes the dialog at both
sizes. Both test windows were quit and their stopped state verified. A drag
attempt did not establish a resized viewport; the minimum-size check used a
separate explicit 960×640 launch instead. Resize timing is not accepted evidence.

## Timing evidence

The repeatable runner is `tests/desktop-parity/capture-performance.mjs`, with
`presentation-probe.m` and `presentation-metrics.mjs` separated by responsibility.
It uses Metal's actual presentation timestamp; it does not count a render request
or offscreen readback as an onscreen frame. See the [timing method](component-validation.md#presented-animation-timing).

Early diagnostics are retained:

- `ui-performance-20261113-locked-debug.trace`: Instruments Animation Hitches
  reached its recording limit but did not finish exporting. Its process sample
  is retained; this is not a usable FPS result.
- `ui-performance-20261113-probe`: the first observer watched `nextDrawable`
  only, missing display-link-supplied frames. It correctly failed with no steady
  data. The observer now measures command-buffer presentations.
- `ui-performance-20261113-probe-r2`: the window stopped presenting after about
  one second. No sustained result is accepted.
- `ui-performance-20261113-foreground`: 78.62 FPS, P95 34.73 ms, maximum 41.68 ms;
  13 zero-time callbacks. Its 144/30 FPS alternation has no activation log, so
  the cause cannot be attributed conclusively. A visual capture was finishing
  during its startup; this round is diagnostic, not an isolated benchmark.
- `ui-performance-20261113-state`: continuous foreground activation was recorded.
  With geometry auditing still enabled, 2,390 frames presented; steady FPS was
  142.97, P95/P99 6.95 ms, maximum 20.84 ms. There were 21 zero-time callbacks
  and three in-flight frames at deliberate termination. The strict gate fails.
- `ui-performance-20261113-runtime`: removing geometry auditing did not eliminate
  zero-time callbacks. This intermediate run predates the final silent-tail
  check and is not the final reproducibility snapshot.

## Final presentation matrix

`artifacts/desktop-parity/ui-performance-20261113-matrix/report.json` is complete.
All eight cases used the same Release binary without geometry/style collection,
18 seconds per case, and a 3 second warmup. No other capture/build was running.
Every case recorded becoming the key window and active app, with no resignation.
The observer and full source hashes are in the report; raw NDJSON remains intact.

| Case | Steady FPS | P95 interval (ms) | Max interval (ms) | Zero-time callbacks |
| --- | ---: | ---: | ---: | ---: |
| `locked-960-light` | 143.20 | 6.95 | 13.89 | 14 |
| `locked-960-dark` | 142.96 | 6.95 | 20.84 | 14 |
| `locked-1280-light` | 143.50 | 6.95 | 13.89 | 8 |
| `locked-1280-dark` | 142.76 | 6.95 | 13.89 | 18 |
| `reset-960-light` | 142.90 | 6.95 | 20.84 | 14 |
| `reset-960-dark` | 143.23 | 6.95 | 20.84 | 10 |
| `reduced-960-light` | — | — | — | 34 |
| `reduced-1280-dark` | — | — | — | 34 |

All six moving cases have P99 around 6.95 ms, no interval over 25 ms after warmup,
and no stale unmatched submission. Each has four drawables still in flight when
terminated deliberately. First tracked presentation ranged from 293 to 450 ms;
this is measured from observer initialization, not from the user's launch action.
The callback observer itself is part of the measured process.

The strict gate remains **failed** because of the zero-time callbacks. Their
presence does not by itself distinguish missed deadlines, superseded submissions
or compositor decisions; no cause has been established. Do not describe this as
zero dropped frames or complete performance acceptance, despite the high steady
frame rate. The two reduced-motion cases each submitted only 35 frames and
presented one; no continuous animation was observed. Their 34 zero callbacks
remain in the report, and the runner did not waive them into a pass.

The earlier 78.62 FPS result is not representative of this isolated active-window
matrix, but is retained without assigning an unproven cause.

## Validation and open work

Local macOS checks passed; logs and exit codes are retained under
`artifacts/desktop-parity/ui-performance-validation-20261113/`:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo clippy -p aipass-desktop-native --all-targets --features parity-render -- -D warnings`
- `cargo test -p aipass-desktop-native --features parity-render` (109 distinct tests)
- `node --test tests/desktop-parity/compare.test.mjs tests/desktop-parity/component-audit.test.mjs tests/desktop-parity/presentation-metrics.test.mjs` (13 tests)
- `pnpm repo:check`, `git diff --check`, and the `aipass-revise` skill validator

After the final corner-only change, formatting, both Clippy commands, repository
checks, both focused capture rounds and native bundle packaging were rerun and
passed (`corner-results.json`). The presentation matrix predates that provider
corner change; no login animation code changed afterward. Its recorded source
hashes identify the measured revision. No remote CI, commit or release occurred.

The full UI inventory, English badge advances, inline-editor mappings and all
interactive/transition states still need acceptance, alongside the one-pixel
advanced-form discrepancy and zero-time presentation callbacks above. These
remain explicit gaps; neither capture completion nor a high FPS is a full pass.

Follow-up: the [optional-field checkpoint](ui-optional-fields-20261113.md)
resolves the advanced-form one-pixel discrepancy and adds menu/disclosure
coverage. It does not resolve the presentation callbacks or full UI inventory.
