# Administration save results and static feedback, 2026-11-15

> Historical checkpoint. Production cutover is now implemented; see [the native cutover record](native-cutover-20261115.md) for current ownership and validation. The findings below describe the earlier state.

The latest user instruction excludes animation matching and smoothness sampling.
This continuation addresses static UI and functional failures. It does not mark
the whole app accepted or remove Tauri. The earlier [administration checkpoint](native-administration-20261115.md)
remains the record for previously reviewed states.

## Corrected behavior

- Preferences save only changed Agent-owned policy/language values. Local-only
  changes no longer rewrite them. Each acknowledged result is retained when a
  later write fails; lost replies attempt readback from the affected owner.
  Local-file failure keeps the prior local preferences while exposing confirmed
  Agent values. Retry starts from that confirmed state. Transport failure restores
  the last acknowledged preference and numeric editor value.
- Saving a sync target and running sync have separate results. A failed run
  preserves the saved target and clears the acknowledged password editor; retry
  retains the Agent-owned password without resending it. A failed target save
  never starts sync. Conflict refresh preserves an unsaved target-mode selection.
- Sync completion requests a catalog refresh even when the run may have applied
  changes before failing. A nested locked error retires the unlocked session and
  clears recovery/catalog state just like an ordinary Agent error.
- Settings success feedback survives its follow-up read, and tool-write feedback
  survives installation rescanning. Preference feedback uses the newly confirmed
  language before the global locale observers run.
- Settings feedback matches the reference position above tabs, 12px text,
  16.8px line height, 8px radius, banner insets and light/dark semantic colors.
  Long diagnostics scroll within 120px, preserving tabs and content. Refresh is
  offered for a failed initial read, not as a full-width action for every error.
  A saved destination followed by failed sync deliberately displays both distinct
  results; the frozen reference displays only its error.

The new logic remains in the native adapter and focused modules. Agent protocols,
credential ownership and storage formats are unchanged. All new production and
test files remain below their repository size limits.

## Evidence

`artifacts/desktop-parity/admin-feedback-final/` contains 16 paired GPU/reference
captures: save success/failure at 960×640 and 1280×820, English/Simplified Chinese,
light/dark. All mapped drawer, banner, tab, content and card bounds pass the
unchanged 0.5 logical-pixel tolerance; there are no unsupported fixture calls.
Paired image review covers English light failure and Chinese dark success. This
does not assert pixel equality or every descendant style. The headless reference
does not reproduce the native backdrop blur; it remains an explicit boundary.

The isolated real-Agent workflow now covers native preference save/readback,
partial Agent/local-file failure, retry, saved local target with failed sync,
and successful local-sync retry. It uses disposable storage and no real provider
or cloud target. Scripted adapter/UI tests cover unavailable replies and the
password-retirement state. These tiers are separate from visible native input.

The workspace regression exposed an intermittent Grok fixture failure: its
negative replay assertion counted a TCP accept as a repeated generation.
Diagnostic reproduction captured `GET / HTTP/1.1`, not the generation path.
The fixture now reads complete bounded requests, rejects only empty connections
and unauthenticated root probes, and keeps authenticated-root/API traffic in its
assertions. A deliberate discovery probe and filter-boundary regression cover
this correction. Production Grok retry/replay behavior is unchanged.

Review of intermittent automatic-sync failures found a separate real fallback
defect. If filesystem watcher creation failed, its dropped callback disconnected
the scheduler's channel and polling stopped before bootstrap. The scheduler now
keeps that channel alive independently of notifications. An injected unavailable
notification backend reproduced the failure before the fix; the regression
requires both bootstrap and a later local mutation to publish within the original
eight-second per-phase deadlines. Explicit stop/shutdown behavior is unchanged.

Validation logs and source hashes are retained in
`artifacts/desktop-parity/admin-functional-20261115/`. Final command results are
recorded there; no commit, push, remote CI or packaged-artifact acceptance is
claimed by this checkpoint.

Final local results on macOS with Homebrew Rust 1.99.1 and Node 26.12.1:

- `cargo test -p aipass-desktop-native --features parity-render`: 101 library,
  185 UI/binary and four integration tests passed after the sync fallback fix.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed without
  exclusions or added suppressions. Additional feature-enabled native Clippy,
  `cargo build --workspace` and `cargo fmt --all -- --check` also passed.
- Frozen dependency install, license audit, `pnpm lint`, `pnpm typecheck`,
  `pnpm test` and `pnpm build` passed. Turbo reused unchanged package results;
  the desktop suite ran 456 tests. Embedded control-panel output stayed unchanged.
  `pnpm repo:check` passed for 999 source files and 18 existing exceptions.
- `RUST_TEST_THREADS=4 cargo test --workspace` passed all 535 Agent tests and
  progressed through native desktop and other crates, then blocked in native-host
  Keychain access. A process sample captured `SecItemCopyMatching`; the owned
  test process was terminated and its exit verified. The full workspace test is
  **incomplete**, not passed. Default-concurrency failure logs and the successful
  targeted reproductions are retained. Concurrency was bounded after observing
  a host load average of 79; no test filters, exclusions or longer deadlines
  were used in that workspace run.

The final feedback matrix has the same image bytes as the reviewed English-light
failure and Chinese-dark success pairs from the earlier matrix. The report and
`reviewed-source-hashes.json` record the capture and final source inputs separately.

## Still open

The subsequent [UI continuation](native-final-states-20261115.md) closes the
file-selection/certificate-error/tool-preview states below, adds startup language
and provider-link flows, and records the user's Keychain waiver and reuse policy.
The following paragraph is the earlier checkpoint, not an instruction to repeat
its completed or waived checks.

Integrated native-window input and a visible OS light/dark round trip remain
unaccepted. The fresh ad hoc signed functional-review bundle started, but the
window interface returned `cgWindowNotFound` twice; the owned process was then
stopped and its exit verified. This does not establish a rendering defect or
native interaction success. The broader review still needs
file-selection/certificate-error/tool-preview result states and whole-app
Chinese review. Keychain-dependent workspace regression and the complete delivery
and packaged startup/update gates remain
separate. Animation work is waived rather than a reason to hold this review open.
