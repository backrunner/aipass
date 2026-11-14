# Native provider editing follow-up, 2026-11-14

This continues [the earlier consistency checkpoint](ui-consistency-20261114.md).
The edit workflow now contains the missing advanced fields and saves their
values through the authenticated Agent. Full UI acceptance remains open. The later user instruction makes completed UI
acceptance the prerequisite for Tauri removal; functional checks may follow
afterward. A passing build or these screenshots alone do not establish that gate.

## Changes

- Restore editable model aliases, existing header names plus a blank assignment
  input, and conditional gateway group display. Aliases preserve escaped commas,
  equals signs, backslashes and Unicode. Untouched aliases follow refreshed
  metadata; typed drafts survive refresh, and an explicit empty edit clears them.
- Merge header assignments case-insensitively with a fresh Agent read on the
  ordered IPC worker. Decode `ProviderHeaderValues`, including its sensitive
  values, instead of assuming the Tauri bridge's unwrapped array response.
  A failed or locked read aborts the entire update. Blank edits preserve headers
  without revealing them. Read failures use existing English/Chinese copy.
- Treat inferred API domains as placeholders, and derive the outgoing domains
  from current API hosts when no explicit override exists. Console hosts do not
  participate. Explicit overrides remain intact. Refresh the placeholder when
  the endpoint changes.
- Match inline edit selects, labels, placeholders, underlines, field ordering,
  WebSocket switch dimensions/motion and the title's inline input baseline.
  Render header names in monospace instead of proportional UI text; exact glyph
  rasterization remains part of the open visual review.
  Reuse the solid-surface scroll fade and keep its paint inside the content
  column so the pane's bottom corners remain round.
- Match integration card padding, heading line height, radius, scan/check icons,
  last-row divider and Codex mode grid typography/sizing. Allow read-only
  installation detection during editing while configuration writes remain
  blocked. Split tool-row presentation into `integrations/tool_rows.rs` and edit
  switch presentation into `providers/edit_controls.rs`.

## Evidence

The immutable WebKit reference and GPUI offscreen renderer use identical
synthetic data at 960x640 and 1280x820, both locales/themes, scale 2. The 0.5
logical-pixel threshold is unchanged. Added mappings cover advanced rows,
selects, the switch, header names and integration children. A separate
`providers-edit-fields` scenario scrolls both implementations to 550 logical
pixels so the advanced controls are actually visible; the existing bottom-scroll
scenario is retained with its failures.

- `artifacts/desktop-parity/edit-close-final-20261114`: 48 paired cases,
  1,118/1,352 geometry checks match, 28/48 whole cases pass, zero missing pairs or
  capture errors. The subsequent change only constrains the edit scroll-fade
  paint to its content column; other 24 add/detail cases remain applicable.
- `artifacts/desktop-parity/edit-close-verified-20261114`: final-source edit
  captures after the corner and header-font fixes: 854/1,088 geometry checks and
  4/24 whole cases pass, with zero capture errors or source-hash mismatches.
  `report.json` retains failed comparisons and unreviewed full presentation
  status; captures are not passes.
- `artifacts/desktop-parity/edit-close-gate-20261114`: check commands/results and
  logs. Native tests pass: 91 library, 123 UI simulation and 4 top-level integration
  tests. The real isolated Agent workflow covers alias updates/clearing, header
  merge/preservation, failed reads and locked writes. It neither uses the user's
  vault nor satisfies the separate native-host Keychain gate.
- Native macOS/Homebrew checks pass: `cargo fmt --all -- --check`,
  `pnpm repo:check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  additional strict native Clippy with `parity-render`, and comparison tests.
  The final paint change also has a fresh UI-test and both Clippy passes.

## Remaining acceptance items

- Expanded bottom-scroll positions retain a 0.703125 logical-pixel difference;
  some Codex first-segment positions differ by 0.5625. Do not loosen the threshold
  or change padding to conceal these. Earlier roughly 200px missing-content
  drift is gone, but the new, more granular matrix still fails.
- Native numeric inputs still need the reference's step controls; notes still
  need vertical drag resizing. Codex mode keyboard navigation and its edit-mode
  availability also need a dedicated interaction comparison.
- Final images were inspected for visible advanced fields, placeholder treatment,
  integration wrapping, scrolling and both themes. Raw declaration differences
  and rasterization have not become full style acceptance. The conditional
  gateway group is implemented but is absent from this default visual fixture.
- Actual native focus/Tab behavior, sustained presented animation frames,
  production lifecycle and the full workspace Keychain-dependent regression are
  still open. These results do not supersede the failed/interrupted performance
  evidence in the previous checkpoint. No remote CI, package or release was run.

Per the repository guardrails, required checks must pass before delivery. No
commit, push or Tauri removal is included in this checkpoint.
