# Native pricing and endpoint copy, 2026-11-14

This checkpoint continues [native actions](native-actions-20261114.md). It
implements the two pricing dialogs previously absent from the native provider
workspace. It does **not** accept production cutover or remove Tauri.

## Implemented

- Each credential has a pricing action and an assignment badge. The dialog reads
  current Agent pricing metadata before enabling writes; loading failures have
  a retry action rather than showing empty defaults as valid data.
- Assign default/shared prices and a nonnegative multiplier to the exact
  entry/credential pair. Preview USD estimates and optionally use the gateway's
  reference multiplier. Unsaved assignment changes block editing shared rules.
- Add/edit shared model-prefix rules, including all cache rates and UTC off-peak
  windows. New required prices stay blank until explicitly supplied. Optional
  one-hour rates retain the distinction between absent and explicit zero.
- Confirm application to all history or from now on; inspect/load historical
  versions and confirm version/group deletion. The Agent remains responsible for
  persistence, manual flags, validation and historical repricing.
- A successful new-group write followed by a failed credential binding retains
  the returned config and retries **only the binding**, using the same IDs.
  It does not repeat the already successful group/version write.
- Requests retain provider/generation scope across transport startup errors.
  Provider/key retirement and lock discard stale completions. Background metadata
  reads do not set the workspace's foreground busy state. They run on selection
  or explicit dialog opening, not on the periodic catalog poll.
- Public endpoint copy uses the existing platform clipboard without requesting
  a secret. Multiple endpoint values use the shared escaped-list encoding. Copy
  feedback expires and is cleared on lock; retired provider actions are ignored.

The UI reuses both locales and theme tokens, shared dropdown focus/navigation,
disclosure timing, modal focus trapping and retained exit animation. Price
steppers support clicks and Up/Down; local history dates and UTC clock fields
have English/Chinese presentation. Decimal-to-micro-dollar conversion avoids an
f64 round trip, including the maximum supported exact integer.

Dialog translation now resolves CSS percentage offsets from **the current
layout bounds** for paint and input together. Gateway billing/history disclosures
animate; the off-peak conditional follows the reference's immediate appearance.
The small compositor extension has a hit-testing regression test.
Both dialogs use the reference's 12 px corners and light/dark modal shadows;
the multiplier field keeps its distinct 10 px horizontal inset. Credential masks
and reference badges align to the right, with their internal bounds included in
the comparison rather than checking only the containing row.

## Engineering and validation

Pricing is split into Agent requests, draft parsing, presentation, UI controller,
assignment view, group view, picker, fields and modal composition. New production
modules remain below 1,000 lines. No new dependency, protocol command, credential
authority or persistent UI store was added. Shared control-panel output is
unchanged.

Local macOS evidence is recorded in
`artifacts/desktop-parity/pricing-complete-gate-20261114/`. Source hashes, command
logs and exit codes accompany the passing checks:

- `cargo fmt --all -- --check`
- `pnpm repo:check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- Native `parity-render` Clippy with `--all-targets -- -D warnings`
- 87 native library tests, 117 UI/rendering tests and four top-level disposable
  Agent/update/instance regression cases
- Syntax checks for the capture/appearance harness and locale-key checks for the UI
- Updated `aipass-revise` skill validation. The initial system Python invocation
  lacked PyYAML; validation passed using `uv run --with pyyaml`, without changing
  repository dependencies. Both results are retained in the preceding
  `pricing-verified-gate-20261114/` checkpoint; the skill has not changed since.

The real authenticated Agent test covers shared groups, exact-key assignments,
manual flags, version scopes/deletion, lock/reunlock persistence and rejected
unknown-key/negative-multiplier writes. Synthetic UI tests separately cover
failed loads, partial commits, stale completions, unsaved drafts, picker keyboard
navigation, numeric stepping and endpoint-copy ownership.

## Visual evidence and open items

The later [UI consistency checkpoint](ui-consistency-20261114.md) supersedes the
intrinsic-width and reference-badge measurements below and records a separate
paint-spill regression. This original evidence remains a point-in-time report.

`artifacts/desktop-parity/pricing-complete-20261114/` contains paired screenshots,
source/binary hashes, element bounds/styles and a review gallery. Eight states
are compared at 960×640 and 1280×820, English/Chinese and light/dark: assignment,
gateway reference, source menu, rules, save confirmation, deletion confirmation,
history and off-peak prices. The reference uses the immutable Svelte baseline
with the same synthetic data. Missing optional prices are omitted from its JSON,
matching the real protocol, rather than incorrectly supplied as `null`.

The strict 0.5 logical-pixel comparison retains intrinsic text-width failures;
it is not widened to turn them into passes. Container, spacing and field geometry
are assessed separately from font widths. The report also retains raw style
differences: native text fields use explicit vertical padding where HTML inputs
center their internal editor with zero CSS padding. This is not a full style
acceptance. Body gaps are measured on the actual native child-spacing owner;
measuring the enclosing scroll/error wrapper would compare unrelated properties.
The report and `metrics-summary.json` retain the exact measurements, including
separate corner/shadow checks.

Final results: 64 captures with zero capture errors; all 64 corner/shadow
comparisons pass. Of 728 mapped geometry checks, 624 meet the 0.5 px tolerance.
The remaining 104 checks concern intrinsic title/action/masked-text widths
(up to 0.90625 px) and reference-badge positioning (up to 0.63770 px). Every
case retains at least one strict failure, so **0/64 cases fully pass**. Raw styles
also retain 96 input-padding and 24 inline-badge line-height differences. The
remaining matched boxes differ by at most 0.38770 px. Direct pixel-coordinate
inspection confirms right-aligned mask/reference ink; a concern raised from
the scaled preview was a visual-reading error, not a demonstrated renderer bug.

These are offscreen comparisons and simulated input tests. Physical native
input/clipboard, presented animation timing, system widget details and the
reference WebKit fixture's missing backdrop blur still require real-window
acceptance. Automatic remote provider-price synchronization and the global
pricing/settings pages are not implemented by these credential dialogs.

The prior full-workspace Rust run remains interrupted at native-host Keychain
access. It was not retried while the user was unavailable, bypassed or reported
as passing. The earlier delivery gate remains incomplete; this checkpoint is not
a commit, push, release, notarization or Tauri-removal result.
