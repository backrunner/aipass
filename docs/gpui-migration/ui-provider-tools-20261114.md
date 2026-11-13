# Provider maintenance follow-up — 2026-11-14

The header typography acceptance below was corrected after user visual review.
See [Header correction](#header-correction-after-user-review): matching a defective
reference was insufficient. Historical matrix files remain unchanged.

This implements the three explicit gaps in the [menu audit](ui-menus-20261114.md):
Reset Provider, endpoint/usage actions, and the separate Notes card. The native
migration as a whole remains unaccepted; real-window interaction and new popup
presentation timing are still open, as detailed below. Work is isolated to the
GPUI migration worktree. No production credentials, installed application,
release, commit or push changed.

## Implemented behavior

- The main menu contains Settings, Reset Provider and Lock (150×100). Reset
  requires an explicit confirmation, retains the original key IDs/fingerprints
  across retries and reopens the API editor or subscription reconnect flow with
  freshly read metadata. Cancelling from an existing editor preserves that form.
  Busy mutation cannot be dismissed or submitted twice; partial failure refreshes
  metadata and keeps the failure visible. Dark destructive buttons now use a
  contrasting foreground instead of white on pale red.
- Provider actions contain Probe endpoint, Refresh usage/group, the separator,
  Archive and Trash (200×139). Probe displays endpoint health/model/WebSocket
  results. The usage dialog includes Auto/New API/SubAPI/advanced modes, stable
  key selection, base URL, masked transient token and user ID, preview, Test and
  explicit Apply. Empty, busy, failed and successful states are implemented.
  Closing a read-only request or changing provider invalidates its late reply.
- Notes is a separate headed card after runtime settings and before integration.
  It retains readonly selection/copy, wrapping, an empty placeholder, internal
  scrolling and vertical resizing. Lock clears its value and resizing state.
  Correcting the preceding Advanced header's height/border removed a cumulative
  four-pixel position error instead of offsetting the Notes card itself.

The new `provider_tools` adapter only calls existing authenticated typed Agent
operations; it neither opens the vault nor resolves secret values in the UI.
All final writes remain Agent-owned. Reset preflights each removal and detects
changed/new keys; these are sequential requests, **not an atomic transaction**.
Usage application preserves the reference's SubAPI clearing and partial gateway
merge semantics. Token text and undo are cleared when ownership moves to the
worker, as well as on dismissal/provider change/lock.

The controller, fields, result presentation, view and fixtures are real Rust
modules. New production files remain below 1,000 lines. The conflict message is
present in English and Simplified Chinese, and shared locale changes have been
rebuilt into the embedded control panel. No vendor code changed in this follow-up.

## Visual comparison

[Paired review](../../artifacts/desktop-parity/provider-tools-20261114-final/index.html)
and [machine-readable report](../../artifacts/desktop-parity/provider-tools-20261114-final/report.json)
contain PNG pairs, component bounds, sampled colors, source hashes and the frozen
binary hash. The runner is `tests/desktop-parity/capture-provider-tools.mjs`.
The immutable Svelte UI is rendered by WebKit; the candidate uses GPUI/Metal.
Fixture-only bridge replies provide the same synthetic usage states without
calling a vendor.

The matrix covers 11 cases at 960×640 and 1280×820, English/Simplified Chinese,
light/dark: both menus, five usage states, the key menu and normal/empty/long
Notes. Mapped geometry uses a 0.75 px threshold. Test/Apply have a separate 2 px
horizontal threshold for measured text advance differences; it is not applied
to other controls. Surface samples use 4/255. These are component checks,
not whole-image equality or a blanket all-controls acceptance.

Final result: **88/88 paired cases passed**, with zero missing observations,
page errors or gate violations. Maximum mapped geometry difference is 0.25 px
outside the two text-sized footer buttons, whose maximum horizontal difference
is 1.8125 px. Sampled surface colors match exactly. All 16 reset snapshots passed.
The final binary is
`9dabd2877b0556b18bed892bb3bf1d641b35117573ef36d38bb78c46abb155bc`;
every recorded source hash matched that checkpoint. Earlier probe/matrix failures remain
preserved, including the scroll mismatch and dark reset-foreground correction.
See [validation metadata](../../artifacts/desktop-parity/provider-tools-20261114-final/validation.json).

Notes comparisons use the same reachable scroll offset in both renderers
(300 px at minimum height, 150 px at the larger height). The initial large-window
failure is preserved: reference scroll was 300 px but native clamped to 236.5 px,
producing a 63.4375 px apparent position difference. Neither component coordinates
nor thresholds were adjusted to hide it.

[Reset snapshots](../../artifacts/desktop-parity/provider-tools-20261114-final/reset-report.json)
cover confirmation/failure across the same eight size/locale/theme combinations.
They assert that the dialog and confirmation button remain within the viewport.
The reference uses OS confirmation, so these 16 native cases do not claim a
pixel-paired system-alert comparison.

The usage dialog follows the reference's computed layout, including its actual
14 px normal-weight title/description, close-button flex sizing, 32 px modes,
body/footer clipping and 14 px corners. It uses genuine 4 px backdrop blur and
the shared compositor. WebKit's captured backdrop does not visibly match that
blur despite its computed filter; whole-background equivalence remains open.
Reference fixture runtime loading and integration expansion also differ outside
the scoped Notes mapping; this report does not accept the whole detail pane.

Popup entrance uses 220 ms cubic-bezier(.22,1,.36,1), an 8 px translation and
0.98→1 scale; overlay entrance uses 180 ms CSS ease. The key menu uses the shared
160/120 ms transitions, selected-item focus and scroll behavior. Reduced motion
is supported. These captures use reduced motion and **do not measure actual
display presentation or certify the new animations' frame pacing**.

## Local validation

- `cargo test -p aipass-desktop-native --features parity-render --quiet`: 182
  passing tests (74 library, 104 UI and 4 integration; child repetitions excluded).
- `cargo clippy --workspace --all-targets -- -D warnings`: passed on macOS with
  Homebrew Rust. Parity-feature all-target Clippy also passed.
- `cargo fmt --all -- --check`, `pnpm repo:check`: passed; 890 source files,
  19 existing size exceptions.
- UI and control-panel typechecks: zero errors/warnings. Shared UI's eight
  existing tests and the embedded control-panel build passed.
- Component-audit, vendor-provenance and presentation-metrics suites: 14 passed.
  The revised pitfalls reference passed the skill validator in an isolated
  PyYAML environment; the initial system-Python run lacked PyYAML.

Tests exercise scoped/lost replies, partial reset retries, changed/new keys,
cancel/duplicate prevention, selected-key keyboard navigation, stale preview
invalidation, token clearing, edit-to-reset transitions and readonly Notes
copy/resize. A disposable real Agent/IPC workflow additionally tests a loopback
model probe, usage HTTP failure, successful quota/group persistence, key reset
and locked rejection. No real provider token or external vendor is used. The
unrelated existing external-tool-config integration branch still reports its
documented fail-closed Keychain boundary in the disposable HOME; its handler
tests cover apply/rollback success. That is distinct from the passing maintenance
workflow.

## Real-window boundary

Two native attempts were made, including a signed fixture bundle copied from
the final comparison binary. The latter started and exposed the correct usage
dialog and controls in accessibility. A key-selector click did not show the
menu; after Raise/Tab, accessibility removed the dialog while the screenshot
still displayed it. The process remained alive with no panic/backtrace output.
The earlier attempt also encountered computer-use error `-10005 timeoutReached`.
The cause is not established; these observations cannot pass mouse/keyboard
interaction or establish current display presentation.

Actual key selection, mode/field Tab order, reset cancellation and Notes resize
must therefore be rechecked in a reliably interactive visible window. A fresh
presented-frame sample is also required for the new popups. Earlier auth/menu
performance reports are historical evidence for their own binaries and are not
reused as a pass here. Remote CI, release artifacts and production startup/update
gates were not run in this checkpoint.

## Header correction after user review

The frozen reference rendered title and description with the same 14 px size,
normal weight and foreground. Its source actually specified a 15 px/650 title
and a 12 px muted description with a 5 px gap: local Svelte selectors had not
reached the markup generated by Bits UI. Copying the faulty computed result into
GPUI was an incorrect acceptance decision.

The Svelte selectors are now scoped global descendants of the header. Both
implementations use a 15 px/650 title with 20 px line height, a 12 px/400 muted
description with 17.4 px line height and a 5 px gap, plus a fixed 30 px close
button. English copy is “Preview usage before applying changes.”; Chinese is
“预览用量信息，确认后再应用。” The embedded control panel locales were rebuilt.
The shared native Reset header also receives the corrected title style.

[Updated comparisons](../../artifacts/desktop-parity/provider-header-20261114-final/index.html)
compare the corrected working-tree Svelte component against native output.
`--current-reference` is explicit; the immutable baseline and inventory were
not edited. Source hashes now include the Svelte dialog and locale files.
The gate measures title and description bounds, font size, weight, line height
and color independently, and rejects identical title/description colors rather
than relying on outer geometry. New visual checks remain offscreen evidence;
the real-window and presentation boundaries above are unchanged.

The corrected header passed **48/48 paired cases** (six usage states, both window
sizes, locales and themes), plus four minimum-size native Reset header snapshots.
Header bounds differ by at most 0.25 px; title/description size, weight, line height
and theme colors pass their explicit checks. Re-running the immutable old page as
a negative control correctly rejects both fonts, description color and missing
color hierarchy; that expected failure is retained in
`provider-header-20261114-negative-control/report.json`.

The three affected frontend typechecks, embedded panel rebuild, 11 focused native
provider-tool tests, repository/diff/format checks, skill validator, workspace
Clippy and parity-feature all-target Clippy passed for the correction. See
[header validation metadata](../../artifacts/desktop-parity/provider-header-20261114-final/validation.json).
