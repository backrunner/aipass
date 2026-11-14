# Logo and authentication follow-up, 2026-11-12

This continues the component/layout/shape/style review. Exact pixel identity is
not required. No complete inventory component is accepted by this follow-up;
Tauri is still the production desktop.

## Changes

- The original 256px logo is retained. The native renderer now downsamples it
  to the actual display pixels for each 16px/28px placement, using Lanczos3 and
  premultiplied alpha before uploading. This avoids direct GPU minification
  dropping thin lines and introducing jagged edges. Asset keys include the
  display resolution, so moving to another scale requests the matching asset.
- Recovery now includes the reset link, warning, confirmation field, submit and
  cancel controls. Only an open recovery panel with trimmed, case-sensitive
  `RESET` can issue a request. Pending requests cannot repeat. Enter in the
  confirmation field does not submit, matching the reference's separate panel.
  Reset uses the existing authenticated `VaultReset` Agent request and waits for
  an absent, locked vault acknowledgement. Failure preserves state for retry.
  No real vault was reset during these checks.
- Scrollable authentication cards now fade at the scrolling edges, following
  the reference's 16px maximum mask depth. The native fade uses its own opaque
  vector backdrop; no captured/reference pixels are used in the application.
  The card shadow is clipped when the reference's scroll mask would clip it.
- Loading buttons render the 12px spinner, with the reference's 700ms linear
  rotation and a static reduced-motion presentation. Password values survive
  pending/failed authentication and are cleared on the existing mode/lock
  boundaries. The incorrect-password notice now has its icon, border, fill and
  localized message. Expandable error diagnostics remain unported.
- A tiny original geometric font makes the secure editor's mask character
  match the reference password disc's size and advance. It contains no copied
  vendor outlines. It is selected only for nonempty masked values; placeholders
  and revealed text use the regular fonts. The existing editor still owns
  masking, selection and clipboard behavior. Its required measurement glyph
  prevents GPUI from silently substituting a smaller system bullet.
- Disabled password ink is painted once at the reference opacity, while the
  disabled native editor remains mounted. Navigation links use intrinsic widths
  and the reference padding, with primary/secondary hover colors restored.

## Evidence and limitations

All round directories are under `artifacts/desktop-parity/`. Reference images
come from the unchanged baseline Svelte components in Playwright WebKit.
Candidate images come from the actual GPUI offscreen renderer, at scale 2.
These are not packaged-app interaction passes. Each successful pair preserves
the original images, diff, geometry, source archive and executable hash.

| Round | Evidence |
| --- | --- |
| `logo-reset-initial-20261112` | 64 pairs of the first logo/reset implementation. Exploratory: the bridge gained unused auth-state branches near the end of capture; its original source archive is retained, so this is not claimed as a fully immutable reference run. |
| `logo-reset-correction-20261112` | Stopped after 3 pairs. The error fixture waited for an event while its first bridge implementation only provided a status response. Failure diagnostics are retained. |
| `logo-reset-feedback-probe-20261112` | 3 pairs: error, reset and scrolled reset; corrected event fixture. |
| `logo-password-probe-20261112` | 2 pairs exposing GPUI's fallback for the initially incomplete mask font. |
| `logo-dot-correction-probe-20261112` | 2 pairs confirming the loaded mask font and disabled ink against busy/error reference states. |
| `logo-reset-regression-20261112` | 88 pairs completed with no fixture errors; 11 states across both sizes, locales and themes. |
| `logo-reset-final-20261112` | Source archiving failed before the first capture on the development `node_modules` symlink. Failure retained; dependency directories are now consistently excluded. |
| `logo-reset-final-correction-20261112` | 16 reset/scroll pairs after the placeholder correction. Visual review exposed a missing native locale alias: the Cancel key was rendered literally. |
| `logo-reset-alias-regression-20261112` | 16 pairs completed with no fixture errors after resolving shared aliases and English fallback. English/light and Chinese/dark scrolled minimum-window pairs were visually reviewed. |

The capture runner now executes the archived binary, verifies reference fixture
hashes before and after each case, and includes changed tracked Rust inputs in
the source archive. Rebuilding the workspace cannot silently change the binary
used halfway through a capture round. Failed cases retain screenshots and traces.

The 88-pair executable SHA-256 is
`905837c6b4c98a51053c5c378ec9d3ddd4d790bc403a5b278d5439d167b8a411`.
Its 136 outer pane/card rectangle comparisons differ by at most 0.40625 logical
pixels (`logo-auth-outer-layout-20261112.json`). This checks only outer layout,
not internal alignment, typography, backgrounds, motion or interactions.

Direct inspection includes the corrected English/light busy and error states,
English/dark and Chinese/dark error states, and the minimum-window
English/light scrolled reset, the English/light empty workspace, and the large
English/light reset panel.
The disc shape/spacing, spinner, notice and reset controls now follow the
reference. The reset placeholder was still too dark in the first full matrix;
the reference's actual computed placeholder color is `rgb(169,169,169)` in both
themes. The next correction applies that value and resolves the shared
`auth.reset.cancel` alias instead of displaying its key. A native locale regression
checks all aliases in both languages. Full matrix capture does not mean every image has received manual review.

The auth backdrop blobs/blur/saturation, letter spacing, some small vertical
offsets, hero animation, hover/focus transitions and strength-color transitions
remain review items. Provider/subscription data and operations, settings,
Agent/proxy panels, nested dialogs, import/export, tray, updater and production
packaging still need native implementation and acceptance.

## Native window observations

The updated isolated `.app` was launched at 960×640, English/light, with no Agent
or vault connection. Its binary matches the final alias round:
`aa707f78cd81eb04aa0d5e9ac2b4e416ff8f09aa2a56547eb6137de0b3bcb04d`.
The prior bundle's jagged 16px logo was observed before replacement; the updated
16px workspace and 28px auth logos were then inspected in the real window.

Native input confirmed the larger password discs, reveal/hide, recovery/reset
navigation, scrolling edge fade, disabled submit for lowercase `reset`, enabled
submit for `RESET`, no Enter submission, cancellation, and an empty confirmation
field on reopening. Synthetic text was used; no reset was submitted. CUA's first
simulated typing omitted the trailing `!`; pasting the same synthetic string
showed it correctly. This is not evidence of a physical-keyboard punctuation pass.

After those observations, the final cancel/return-to-unlock step failed first
with a stale element and then `cgWindowNotFound`, also on app reacquisition.
That last step remains unverified. The bundle stays updated; no user session or
system settings were changed. These limited native observations do not turn the
offscreen matrix into a packaged-app interaction pass.

## Dependency regression checks

The full workspace suite exposed an indirect effect of GPUI's dependency graph:
`serde_json/preserve_order` changes object iteration order after Cargo feature
unification. Copilot allowance windows/scoped model IDs and Codex buckets now
sort explicitly to preserve their previous presentation order. A regression
feeds reversed vendor key order. The Claude cache and streaming-tool-call fixtures now compare parsed JSON
while retaining environment, payload, event and completion assertions. The
streaming regression was moved out of the oversized proxy test root into its
own real module.

An initial macOS LaunchAgent test failed to observe its fixture child within its
deadline. Its isolated rerun passed without code changes. The original failure
and rerun remain separate evidence. It registers only a uniquely named
disposable test service; existing user LaunchAgents and vaults are untouched.

## Validation

Logs and command results are in `logo-reset-validation-20261112/`.

- Node/macOS: frozen install, license audit, lint, typecheck, tests and build
  passed; generated embedded control-panel output remained unchanged.
- Native `parity-render` tests: 28 passed. Comparison/coverage tests: 6 passed.
- Full workspace Clippy and the additional `parity-render` Clippy passed with
  `-D warnings`, using Homebrew Rust 1.99.1 on macOS.
- Repository checks passed for 770 source files; formatting and diff checks
  passed. Full workspace tests (1366 passed, 4 ignored) and build passed (`rust-alias.json`),
  including the formerly failing Agent and proxy cases.
- Desktop bundle/startup/update artifact acceptance and remote CI have not run
  for this migration. No commit, push or release was performed.
