# Complete follow-up review of the migration worktree

This is a review of the implemented code and the remaining migration scope,
not acceptance of a complete GPUI desktop. Worktree: `aipass-gpui`, branch
`refactor/gpui-desktop`, base `65a1f1f5a5e1b51311132659f9b972131c8ea551`.
No commit, push, release, personal vault mutation or real CLI config write.

## Findings corrected in this round

| Priority | Reproduction and consequence | Correction and regression |
| --- | --- | --- |
| P1 | Start proxy, hold `server_config_set`, lock the vault, then release the save. The old UI still sent `server_start`. Other proxy operations could publish late data/errors after locking. | Extract UI mutation ownership into `services/serverMutation.ts`. Save/start/stop/rotate/clear check the session revision after awaits. App regressions assert no start or token rotation after locking; controller tests cover late errors, re-unlock, busy rejection and retry. |
| P1 | Replace a recovery key while its confirmed dialog is closing. The old 220 ms callback acknowledged the new content and its checkbox remained confirmed. | Reset confirmation/open state and cancel the close timer on every key identity change; cancel on teardown. A replacement-key regression failed before and passes after. |
| P2 | Update a proxy address, token or upstream protocol without changing route ID while its quick-integration preview is open or pending. The old preview remained valid. | Include route configuration, bind address and effective model in the invalidation key. Three regressions cover both open and delayed previews and assert no stale apply. |
| P2 | Close a provider modal and unmount it before its exit animation completes. `onClose` still ran later against its parent. | Own and cancel the close timer on teardown. The regression failed before and passes after. |
| P2 | Complete an incorrect GPUI password request when its background refresh is due. Submitting Refresh cleared `Session.error` before the auth view copied it. | Capture the localized error before scheduling the refresh. The GPUI test asserts the message, cleared busy state and hidden workspace. |

The auth correction changes presentation feedback, not Agent authentication.
The proxy correction suppresses future UI continuations and stale feedback;
it does not claim to cancel a backend command that was already submitted.
Persistence and authorization remain in Rust. The existing save-success /
status-refresh-failure behavior is preserved. App.svelte became shorter;
operation ownership is a real module, not an include fragment.

Seven assertions were observed failing before their respective fixes: one GPUI
password case, three proxy preview cases, two modal cases and one App startup
case. The token-rotation sibling and controller cases were added for coverage.

## Review coverage

| Surface | Current evidence |
| --- | --- |
| Proxy cards, chart, usage table, routes and live metrics | Production Svelte App with synthetic IPC; real WebKit geometry/scroll checks at 960x640 and 1280x820, both locales and themes. Includes 9-column alignment, horizontal scrolling, ranges, live tooltip values and clearing. |
| Proxy controls and logs | Start/stop/address save failure and retry, route toggle, clear/cancel, 1,000-row log following, manual scroll anchor and failed refresh. App unit tests additionally exercise lock during pending operations. |
| Agent accounts | Account selection, captured preview, cancel, failed apply, retry and rollback, timezone offset and unmanaged states. |
| Provider forms | Invalid endpoint, failed save, preserved draft, retry, reopening, header merge, WebSocket opt-out and concurrency persistence. Close teardown now has a focused regression. |
| Quick integration and JSON diff | Rust-generated synthetic diffs, four files, additions/deletions/unchanged file, four change groups, line/column geometry, horizontal and vertical scroll synchronization, literal full-file content, keyboard tabs, cancel, captured apply and failure. |
| Auth, toast and updates | Native auth/session/curtain/toast tests plus desktop update-notice lock/restart checks. GPUI password feedback now has the overdue-refresh regression. |
| Layout and style | 19 selector families compared with the immutable reference for gaps, padding, margins and radii. Card spacing remains 14 px and proxy field right edges match in all eight combinations. Representative minimum-window proxy, JSON diff and provider-error screenshots were also inspected. |

The two browser rounds used the final application source and checked hashes
before and after execution:

- `artifacts/desktop-parity/complete-review-json-20261112`: **16/16** groups.
- `artifacts/desktop-parity/complete-review-business-20261112`: **48/48** groups.

These are production Svelte components with in-memory synthetic IPC, not native
GPUI business pages, vendor network acceptance or packaged Tauri runtime tests.
Screenshots use scale 2. The immutable reference was not overwritten.

## Local validation

All runs were native macOS with Homebrew Rust. Full CI workflow definitions were
reviewed before broad source checks. Logs and source snapshots are retained in
`artifacts/desktop-parity/complete-review-20261112`.

| Command / check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| Native crate Clippy with `--all-targets --features parity-render -- -D warnings` | Passed |
| `cargo test --workspace` | 1,369 passed; 4 explicitly ignored by existing tests |
| `cargo test -p aipass-desktop-native --features parity-render` | 35 passed (16 library, 19 executable) |
| `cargo build --workspace` and native build with `parity-render` | Passed |
| `pnpm install --frozen-lockfile` | Passed; no dependency change in this round |
| `pnpm licenses:audit`, `pnpm lint`, `pnpm typecheck` | Passed |
| `pnpm test` | Passed; desktop 54 files / 457 tests |
| `pnpm build` | Passed |
| Embedded control-panel diff check | Unchanged |
| Original `aipass` worktree status | Clean |

One earlier heavily concurrent desktop-only run timed out in the pre-existing
scroll-mask style-repair test (1 second wait). The subsequent complete Node
run passed that unchanged test and all 457 desktop tests. This does not establish
the timeout's root cause or prove the test is free from intermittent timing
failure; no production masking change or weakened assertion was made.

## Outstanding acceptance findings

- **P0: migration incomplete.** GPUI workspace navigation changes selection but
  still renders the synthetic empty workspace. Provider business/forms, Agent,
  local proxy, quick integration/JSON diff, settings and updater/tray/packaging
  replacements are not implemented. The existing Tauri desktop remains the
  production entry point; none of those missing surfaces can be accepted from
  the browser checks above.
- **P1: native recovery-kit parity is incomplete.** The GPUI overlay still uses
  pointer-only div controls and lacks the reference dialog's full focus/keyboard,
  copy and download behavior. It needs an actual native component and interaction
  acceptance before the Agent-connected preview can become production.
- **P2: existing compact-copy debt.** The Chinese proxy route subtitle still
  joins strategy and count with a middle dot in shared `ProxyRouteList.svelte`.
  This is unchanged from the immutable reference and conflicts with the current
  concise-copy rule; it is not silently marked accepted by spacing equality.
- Native inspection was retried and returned `Sky Computer Use native pipe
  startup failed` plus `The browser access host is unavailable`. This is a
  connection failure, not evidence that the screen is locked. No new real-window
  screenshot, physical keyboard/mouse acceptance or native animation acceptance
  is claimed. The rebuilt native binary was not installed over production.
- The packaged macOS startup/update artifact gate and remote CI did not run.
  The successful source checks do not complete the migration/release gate.

The full application cannot be called bug-free or ready to replace Tauri.
