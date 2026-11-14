# UI-first cleanup checkpoint, 2026-11-14

The user now requires complete UI acceptance before removing Tauri, with
functional regression deferred until afterward. This supersedes the old
production/Keychain prerequisites in the [cutover audit](cutover-audit-20261114.md).
The cleanup is still conditional: missing pages, controls and incomplete native
interaction acceptance are UI gaps. The user subsequently waived foreground
smoothness sampling; it is no longer a removal prerequisite and is not a pass.
No Tauri source has been removed in this checkpoint.

## Corrections

- Preserve fractional text line heights and measured disclosure extents until
  layout. Repeated sections no longer accumulate rounded heights at the bottom
  of a scroll view. The same layout rule applies to interrupted reveal motion.
- Keep intrinsic Codex segment widths fractional, restore switching while
  editing, and implement Left/Right/Home/End with one roving Tab stop. Existing
  configuration writes remain blocked while a provider draft is open.
- Add concurrency steppers with mouse press/repeat, keyboard stepping, bounds,
  empty/fractional value alignment, undo, disabled state and release cancellation.
  Keep the existing editor as the single focus/accessibility owner.
- Add vertical resizing to editable notes, preserving the text and undo state,
  enforcing the 64px minimum and resetting transient sizing on cancel/clear.
- Match the header-name monospace face and the platform stepper presentation.
  Advance width alone had missed the visibly different Courier glyphs; inspected
  Menlo glyph crops now match the reference's letter shapes.
- Use the macOS system monospaced family for recovery-key input, matching the
  reference's `ui-monospace`. This is distinct from the header field's generic
  `monospace` face. Inspect the glyphs as well as the enclosing input bounds.
- Add a conditional gateway-group fixture with mixed English/Chinese text. This
  exposed a missing fallback-font line metric, corrected using the shaped line's
  ascent/descent. Both renderers use the same fixture and scroll position.
- Restore the subscription status badge's border, pill corners, 11px type and
  no-wrap behavior. The old `active` label wrapped to two lines, moving the
  collapsed runtime card down by 11px. Include the preceding plan and badge in
  overview geometry/style checks so scroll alignment cannot conceal this defect.
- Remove a second inset from the glass canvas: absolute child layout already
  excludes the parent's border. The duplicate inset left a visible unpainted
  strip around workspace panes and the authentication card.
- Follow the containing card's bottom corners on the final gateway-group row.
  GPUI's rectangular overflow mask otherwise leaves the row's bottom separator
  visible outside the curved card. The row's measured height stays unchanged.
- Record foreground application changes in the presentation observer and require
  continuous foreground/key-window evidence. A high frame rate while backgrounded
  cannot satisfy acceptance. Preserve interrupted samples and diagnostic traces.

## Evidence boundaries

The edit matrix before the final stepper/font/conditional-fixture additions
(`ui-controls-matrix-20261114`) passed 24 cases and all 1,088 geometry checks.
`ui-gateway-probe2-20261114` then passed the new minimum-size gateway case.
Neither result constitutes full UI acceptance. Final-source regressions and
command logs are collected under `ui-final-gate-20261114` and the
`ui-regression-*-20261114` artifact directories.

The three `ui-acceptance-present*-20261114` attempts remain failed/incomplete
acceptance samples. The third trace identifies `org.godotengine.godot` taking
the foreground at about 463ms; AIPass lost key/active state immediately afterward.
Do not diagnose background throttling as a measured foreground renderer limit.
No performance pass is claimed, and no other application's process was stopped.
The user initially deferred another foreground sample while Godot was running,
then explicitly waived smoothness sampling. Preserve these failed/incomplete
attempts as diagnostics; do not schedule another sample as a cleanup prerequisite.

The glass edge audit checks adjacent pixel strips inside the side borders,
excluding text, corners and outer shadows. It finds 80 failing pane checks among
112 old captures; all 16 pane checks pass in the eight new
`ui-regression-glass-20261114` captures. This is targeted paint evidence, not
whole-image acceptance. Subscription geometry and control checks captured before
the glass-only change retain their original source hashes; they do not prove the
corrected glass painting. The separate glass captures and subsequent authentication
captures cover that correction. The final gateway-row correction is captured in
`ui-regression-glass-final-20261114`: eight cases retain matching geometry and all
16 glass edge checks pass.

### Paired regression results

All rows cover 960×640 and 1280×820, English and Simplified Chinese, light and
dark, at 2× scale. Passing geometry/targeted appearance checks does not mean every
pixel, interaction or presented frame has been accepted.

| Artifact round | Result |
| --- | --- |
| `ui-regression-providers-20261114` | 56 cases, 1,792 matching geometry checks; zero capture errors. |
| `ui-regression-overview2-20261114` | 16 cases; 352 absolute and 352 card-relative geometry checks, explicit styles, scoped appearance and icon paint checks pass. The earlier `overview` round retains the 11px badge-wrap failures. |
| `ui-regression-usage-20261114` + `ui-regression-usage-unknown-20261114` | 48 cases covering usage, stale, empty, console-only, unknown and collapsed states; zero geometry violations or capture errors. |
| `ui-regression-controls-20261114` | 120 cases covering buttons, input/textarea focus, checkbox/switch keyboard states, tabs, select menus and disabled/hover states; zero geometry/pixel violations or errors. |
| `ui-regression-auth-20261114` | 64 cases; 576 geometry and 1,568 explicit numeric/white-space style checks pass; all 64 captures observe a complete first background frame. |
| `ui-auth-font-final-20261114` | 24 replacement captures for recovery/reset states after correcting the system monospaced font; geometry and explicit styles pass with zero capture errors. |
| `ui-regression-pricing-20261114` | 64 cases; 728 geometry checks and all 64 scoped appearance checks pass. All 16 badge-ink checks pass; zero capture errors. |

The initial provider/control/overview/usage rounds precede the glass-only fix.
The original authentication round precedes the recovery-key font fix. Their
unchanged checks are reused only for that scope; later rounds cover the affected
painting and font. Raw declaration diagnostics are kept, not converted into full
style acceptance.
Pricing retains the same 144 raw declaration differences in native editor
padding, inline line-height and white-space ownership described in the
[earlier consistency review](ui-consistency-20261114.md); those diagnostics are
separate from its passing geometry, corners/shadows and badge paint checks.

## Local code validation

Homebrew Rust on macOS passed the following checks. No remote CI or release is
claimed, and no local Linux run was performed.

- `cargo fmt --all -- --check`, `pnpm repo:check` and `git diff --check`.
- `cargo clippy --workspace --all-targets -- -D warnings` and native
  `parity-render` Clippy with all targets and warnings denied. The last results
  are in `ui-final-gate-20261114/gate-complete.json`, with current code hashes in
  `source-complete.json`. The unchanged structural check is in `gate-final.json`.
- Before the final badge/glass/corner/font styling changes: 91 native library tests,
  127 UI/render tests, four disposable integration cases and two doctests passed.
  The final glass change additionally passed all three existing backdrop tests,
  including current-frame capture and resize retirement. These do not establish
  visible frame pacing or physical input acceptance.
- The comparison, presentation-metrics and vendor-provenance tests passed;
  the glass audit also detects the earlier defective images as a negative control.

The follow-up [system appearance checkpoint](system-appearance-20261114.md)
adds preference/platform observation, complete-frame theme transitions and an
atlas edge fix. Its tests and captures supersede the relevant appearance evidence;
foreground smoothness sampling is now waived by the user.

## Readiness review before onboarding and Agent follow-up

The findings in this subsection are historical. The [onboarding and Agent checkpoint](native-onboarding-agents-20261114.md) supersedes the missing import, cloud, recovery, loading and Agent-page findings below.

After the sampling waiver, a read-only source audit confirmed the following
visible gaps. This review does not rerun or expand the earlier capture matrices.
All 170 files hashed in `appearance-matrix-20261114/validation.json` still match.
The final appearance capture report has no changed production source; only its
subsequently corrected capture-inventory/filter script differs. The login report
also predates the documented test formatting and standalone WGPU lockfile changes.
The 137-test result and the 20-case, 176-capture appearance results retain their
stated scope; they do not establish coverage of the missing pages.

- [Workspace navigation](../../apps/desktop-native/src/workspace.rs) changes the
  selected section for Agent/local proxy, but still renders the provider layout.
  [Catalog entries](../../apps/desktop-native/src/catalog.rs) explicitly returns
  an empty list for sections 7 and 8. These pages are absent, not merely awaiting
  backend validation.
- [The settings menu](../../apps/desktop-native/src/workspace/chrome.rs) emits
  `WorkspaceEvent::Settings`; [the desktop subscriber](../../apps/desktop-native/src/desktop.rs)
  leaves that event in its wildcard no-op branch. Preference observation and the
  appearance transition do not supply the missing settings shell and controls.
- The same desktop subscriber maps `AuthEvent::Import` to `None`. The visible
  import entry therefore has no native import form. Native authentication also
  lacks the reference's cloud-empty/cloud-failed retry and conditional local-only
  create controls; creation always passes `local_only: false`.
- [The recovery view](../../apps/desktop-native/src/desktop/view.rs) displays a
  key and acknowledgment, but omits the reference's copy/copied control and uses
  a text-symbol acknowledgment instead of the shared checkbox. The current
  [reference app](../../apps/desktop/src/App.svelte) supplies `onCopy` but no
  `onDownload` to `RecoveryKitModal`; the component's optional download button is
  not an active reference requirement. This corrects the earlier broad recovery
  export claim in the historical cutover audit.
- The reference app has a distinct initial-status/initial-sync loading surface.
  Native root composition selects authentication or workspace without that state.
  Sync status presentation belongs to the missing global settings coverage.

## UI coverage still required

| Surface | Current gap |
| --- | --- |
| Agent management | Implemented with paired geometry evidence; integrated native-input acceptance remains |
| Local proxy | Main/detail views, channel indicators, logs, search settings and usage charts/range controls |
| Global settings | Settings shell, security policy, sync targets, control-panel settings, import and model-pricing fields |
| Authentication/setup | Implemented with paired geometry evidence; integrated native-input acceptance remains |
| Recovery kit | Implemented with paired geometry evidence; integrated native-input acceptance remains |
| Initial loading | Implemented, including failure/retry; integrated transition/input acceptance remains |
| Integrated native input | Physical focus/Tab, popup/disclosure interactions and resize across the two sizes/locales/themes |

Foreground smoothness sampling is waived, not included in this remaining-work
table. Fixed-time animation comparison and UI interaction coverage remain distinct
from frame-pacing measurement.

The 98-surface inventory is still a coverage inventory, not 98 accepted native
replacements. These UI gaps keep the removal condition unmet. Production entry,
platform service wiring, Keychain-dependent functional tests, packaging/update
runtime acceptance and remote CI are tracked separately for the later functional
phase. Deleting Tauri must preserve shared assets, Rust-owned services, the LAN
control panel and required third-party notices, and leave a buildable tree.
