# Native actions checkpoint, 2026-11-14

This implements the clipboard, activity and trash gaps from the
[cutover audit](cutover-audit-20261114.md). Tauri remains required. This checkpoint
does not accept native production startup, every reference surface, physical
input or presented animation performance.

## Implementation

- `aipass-desktop-platform` now owns the existing preference schema/storage and
  secret clipboard service. Tauri uses the same implementation. Native copies
  respect confirmation, click action and expiry preferences. Confirmation occurs
  before the Agent reveals a secret; the UI receives only a copied/cancelled
  outcome. Single-use permits invalidate late writes after lock, selection
  changes or cancellation. Cleanup retains only an ownership digest, preserves
  unrelated clipboard contents and retries transient OS failures. Native lock,
  quit and update paths invalidate requests and clear owned contents.
- Native keyboard, pointer-down and wheel events feed throttled `SessionTouch`
  requests through the existing ordered worker. Old queued activity is dropped;
  background refresh cannot keep a session unlocked. Agent-owned inactivity and
  sleep/screen-lock policy remains authoritative. The worker is a separate module
  and routes early transport failures back to scoped panels, including credentials
  and trash, so they can leave their pending state.
- Trash provides permanent deletion and Empty Trash confirmation using existing
  authenticated Agent operations. Confirmation freezes the selected provider,
  prevents repeated submission, retains failures for retry and retires stale
  completions on lock. Partial Empty Trash failures refresh the catalog. The
  archive action, trash retention banner, action colors and confirmation spacing
  now follow the reference. Labels reuse the existing English/Chinese dictionaries.
- Apple text matching now maps named system font weights to their CSS classes.
  The upstream generic conversion mapped Medium to about 530, causing a request
  for 500 to select Regular. The correction also distinguishes Heavy and Black;
  other families retain their existing behavior. The maintained patch, standalone
  test lockfile and source inventory are recorded under `vendor/`. The new module
  is subject to the ordinary source-size limit.

The original source has a 120 ms hover/focus fade for credential copy. The native
control now has that transition, a localized copied state and reduced-motion
handling. These implementation and event tests do not prove visible frame pacing.

## Validation evidence

Evidence is local and ignored under `artifacts/desktop-parity/`; it is not uploaded
by a Git push. `gaps-gate-20261114/` holds command logs, exit codes, provenance and
the Keychain process sample. Tests use synthetic credentials and isolated Agent
profiles; no real credential was copied and no user provider was deleted.

- The initial Node gate passed frozen install, license audit, lint, typecheck,
  tests, build and unchanged embedded control-panel output. Repository structural
  checks, edited skill validation and renderer provenance checks also pass.
- Native/platform tests cover confirmation cancellation, late clipboard writes,
  ownership cleanup, expiry retry, activity throttling, capture-phase wheel events,
  trash confirmation/retry/stale completion and real isolated Agent deletion.
  Apple renderer tests cover distinct system weights and unchanged other families.
  One repeated Agent integration run failed at its synthetic HTTP probe. Its
  fixture assumed one TCP read contained complete headers; it now waits for the
  header boundary before replying/closing and reports the probe diagnostic on
  failure. The focused rerun passed. The original failing log is retained.
- Workspace formatting, strict Clippy, compilation and native parity-feature
  checks are recorded in the final command log. The mandatory command is
  `cargo clippy --workspace --all-targets -- -D warnings` on Homebrew Rust/macOS.
  `verified.json` records successful formatting, both Clippy commands, workspace
  build and the native/platform suite (80 library, 109 UI and 12 shared-service
  unit tests, plus isolated Agent/update/instance integrations). The standalone
  Apple renderer's 16 tests and strict all-target Clippy also passed.
- **The full workspace test gate is incomplete.** It reached native-host and
  stalled in `SecItemCopyMatching`; the process sample confirms the Keychain
  wait. The user was unavailable to handle the prompt, so only our blocked test
  process was terminated. `rust-workspace-blocked.json` records that interruption.
  Remaining crates were tested separately; they do not turn this into a complete
  workspace pass. Do not bypass Keychain or reuse an older complete pass.
- The current Tauri app/DMG passed helper-payload checks, non-GUI DMG checks and
  seven startup/update/restart runtime scenarios with updater signatures enabled.
  Disposable signing keys and ad hoc app signing were used. This verifies the
  shared-service extraction's current Tauri path, not a GPUI production artifact,
  Apple notarization or a published release.
- The synthetic native bundle could launch, but the window interface returned
  `cgWindowNotFound`. Physical keyboard traversal, real clipboard interaction,
  sleep/lock transitions and presented-frame acceptance remain pending.

## Visual coverage and remaining differences

The trash matrix covers list, permanent-delete confirmation and Empty Trash
confirmation at 960×640 and 1280×820 in both locales and themes: 24 pairs.
Comparison includes measured component boundaries and real offscreen Metal
images against the frozen Svelte/WebKit reference. It does not grant pixel-exact
or native-input acceptance.

- `gaps-trash-fonts-20261114`: all 24 cases pass mapped geometry at the unchanged
  0.5 logical-pixel tolerance. Both confirmation roots, titles and descriptions
  match exactly. The English Empty Trash button's width difference fell from
  1.734375 to 0.265625 logical pixels after the font correction.
- `gaps-providers-fonts-20261114`: 40 captured cases, with all 32 form cases
  passing mapped geometry. Eight provider-detail cases retain two badge geometry
  failures each; these remain failures, not threshold exceptions. Neither matrix
  reported page errors or unsupported fixture calls.
- `gaps-controls-fonts-20261114`: all 120 input, dropdown, switch, checkbox,
  textarea, button, icon and tab interaction-state cases pass the existing
  geometry, sampled-color and glyph checks, with no page errors. This is synthetic
  input/offscreen rendering evidence; it does not accept physical input or motion.
- `gaps-native-final-20261114` contains freshly rebuilt, ad hoc signed comparison
  apps. The obsolete synthetic preview was closed; the final bundle has not
  received physical-window acceptance.

Keep these findings open:

- Variable-weight text such as 650 still uses a named face in GPUI; its ink can
  differ from WebKit's variable-font rendering even when bounds agree.
- Provider-detail badge widths accumulate subpixel differences. The edit fixture
  also shows a different initial title-input focus/scroll position; distinguish
  fixture startup from actual Edit-button behavior before changing production
  focus handling. The missing endpoint-copy and pricing controls remain visible
  gaps in provider-detail screenshots.
- The confirmation source declares a 4 px backdrop blur. Native Metal renders
  the blur; the WebKit fixture image does not show the same blur. Verify the actual
  Tauri window before deciding whether the difference is fixture-engine behavior.
- Real copy confirmation, hover presentation, clipboard cleanup on quit/update
  and focus restoration require an available desktop session.
- Pricing assignment, global settings, Agent/proxy pages, vault import/recovery,
  platform adapters, native production launcher/updater/packaging and all other
  blockers in the cutover audit remain open. Retain Tauri and the shared Svelte
  control-panel dependencies until their replacements pass acceptance.

No commit, push, release or Tauri removal is performed by this checkpoint.
