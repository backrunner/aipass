# Native UI consistency follow-up, 2026-11-14

This checkpoint closes the previously recorded intrinsic text-width and pricing
badge defects and extends the edit-provider audit. It does not accept the full
migration or supersede the remaining [cutover blockers](cutover-audit-20261114.md).

## Changes

- Preserve fractional shaped advances through both GPUI text measurement and
  Taffy intrinsic layout. Round final absolute edges, retaining the existing
  measurement policy for non-text custom elements. Text measurement caches also
  include the display scale. The maintained upstream patch and source inventory
  remain pinned; all three changed renderer files were reconstructed exactly by
  applying their patch to verified upstream bytes.
- Position the inline pricing reference badge from font ascent/descent and the
  parent baseline. Keep badge/cost-preview text and shared provider buttons on
  one line. A screenshot review caught an escaped final letter even after the
  badge's outer geometry passed; the new pixel audit detects that failure.
- Give edit-provider actions their correct ghost treatment, padding and grouped
  spacing. Match inline edit inputs separately from add-form inputs: labels,
  size, corners, underline color, inset and row gaps. Restore title focus styling
  and end-of-value selection, including Unicode titles. Match notes and the
  advanced disclosure's typography, height and divider.
- Reuse the existing subscription/runtime and integration controllers inside the
  edit form. Editing continues to block integration writes. The same controllers
  retain request ownership and state across edit/cancel; no duplicate service or
  storage implementation was introduced. Align synthetic tool detections/status
  with the reference fixture before comparing their cards.

## Comparison method and results

Captures use the immutable WebKit reference and matched synthetic data at
960×640 and 1280×820, English/Chinese, light/dark, scale 2. Geometry keeps the
0.5 logical-pixel threshold. Missing mappings still fail. The audit now records
white-space declarations and maps edit headers/actions, basic fields, notes,
the disclosure trigger and both formerly missing cards.

The original provider capture harness had no mappings for either edit scenario.
Earlier claims about form capture counts must not be read as edit-layout passes.
The expanded mapping intentionally exposes failures rather than accepting those
previously unmeasured areas.

| Local artifact directory under `artifacts/desktop-parity/` | Result |
| --- | --- |
| `consistency-pricing-final-20261114` | 64/64 cases, 728/728 geometry checks, 64/64 corner/shadow checks and 16/16 badge paint checks pass; zero capture errors. |
| `fractional-final-controls-20261114` | 120/120 state cases pass geometry and targeted pixel checks; no violations/errors. Later edits affect provider forms and the pricing badge, not these control fixtures. |
| `consistency-list-20261114` | 24/24 cases, 432/432 geometry checks and all scoped list-appearance checks pass. |
| `consistency-trash-20261114` | 24/24 cases, 152/152 mapped geometry checks pass. |
| `consistency-edit-final-20261114` | 24/40 cases pass; 472/600 geometry checks match. All 16 edit cases retain failures. No missing pairs or capture errors. |

The pricing paint audit examines foreground-colored pixels in the otherwise
empty strip immediately below the badge. Its negative control is
`consistency-pricing-20261114`: all eight English cases spill 26 foreground
pixels each while their reference images spill zero. The Chinese cases do not
spill. This proves the new check detects the observed defect; matching boxes
alone were insufficient. The audit records original image hashes and its own
source hash without changing the captured images.

The final pricing report retains 144 raw declaration differences: 96 input
padding, 24 inline line-height and 24 white-space checks. The native badge uses
explicit no-wrap to prevent GPUI's emergency word breaking inside its snapped
box; the reference's normal inline span keeps that word together. This declared
style difference is retained, alongside the matching geometry and paint evidence.

Raw style reports remain diagnostic, not full visual acceptance. HTML editor
internals, inline line boxes and native wrapper ownership do not have identical
declarations. The WebKit fixture does not establish native backdrop presentation.

## Validation

`consistency-gate-20261114/results-accepted.json` and `source-accepted.json` record
the latest code checks and hashes. Homebrew Rust 1.99.1 on macOS passed:

- `cargo fmt --all -- --check` and `pnpm repo:check`.
- `cargo clippy --workspace --all-targets -- -D warnings`.
- Native `parity-render` Clippy with all targets and warnings denied.
- 87 library tests, 121 UI/render tests and four top-level disposable
  Agent/update/instance regression cases.
- Release build and paint-audit syntax check.

The unchanged comparison/provenance/presentation-metric suites passed 15 tests
in `results-final.json`. The edited skill validator also passed. New regressions
cover fractional adjacent labels, display-scale changes, fit/wrap/ellipsis,
Unicode editing and preservation of both panels with integration writes blocked.
The synthetic GPUI text system does not prove Apple glyph rasterization; actual
Apple text is covered by the paired screenshots. Vendor patch blank context
lines were normalized and re-applied exactly; that formatting does not alter
compiled source. No local Linux validation was performed.

`consistency-performance-20261114` contains a fresh Release presentation attempt
using `MTLDrawable.presentedTime`, after builds and captures stopped. All four
completed lock-screen cases failed continuous presentation: their windows lost
key/active state before the required 18 seconds. The first three lost focus at
about 1.83, 5.88 and 2.03 seconds, followed by long periods without presented
frames. The dark 960×640 case briefly reached 143.27 FPS / 6.95 ms p95, but this
is not a passing sample. Invalid callbacks also remain in the raw trace.

The fifth case was deliberately terminated after repeated interruptions;
`interruption.json` records the owned PID/UID/start time and reason. The matrix
is incomplete, no threshold or window policy was overridden, and no current
animation-performance pass is claimed. Repeat a single visible 18-second probe
successfully before attempting the full matrix again.

## Still open

- The edit title input remains about 0.594 logical pixels above the reference
  inline control. Its width and neighboring actions now match within tolerance.
- The edit advanced section still omits alias/header editing and the conditional
  gateway-group row. Its picker and other advanced-state styling need further
  mapping. The shorter content produces a 197.5–202.5 px difference at bottom
  scroll in the mapped basic fields; this is not fixed by shifting components.
- Integration-card internal spacing, row wrapping and its scan icon remain
  different. The card-height difference also moves bottom-aligned notes/runtime
  cards. Presence of the restored panels is not complete visual acceptance.
- Physical native keyboard/mouse/clipboard interaction, broader popup/disclosure
  presentation, and the other page/platform/production gaps in the cutover audit
  remain open. This checkpoint does not remove Tauri.

The earlier full-workspace Rust run remains incomplete at native-host Keychain
access. It was not bypassed or rerun while the user was unavailable. These scoped
passes are not a full delivery-gate pass, commit, push or release.
