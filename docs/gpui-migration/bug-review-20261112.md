# Follow-up defect review

Scope: existing desktop proxy, Agent and provider flows, quick integration,
JSON preview, and the implemented GPUI authentication/workspace/toast code.
All changes remain in `/Volumes/BRData/projects/aipass-gpui` on
`refactor/gpui-desktop`. No commit, push, release or real CLI config write.

## Reproduced and corrected

### Quick integration could remain permanently busy after rediscovery

If a detected Agent disappeared while its preview or apply request was pending,
the old request became invalid, but its busy state could survive. Rediscovering
the same Agent then left its Write config button disabled. The pending preview
path also registered its tool too late to invalidate an outstanding request.

`IntegrationCard.svelte` now records the pending tool before awaiting a preview.
Selection changes and missing-tool updates share a complete invalidation path:
it advances the request generation, clears the pending plan/dialog, removes
success timers and resets confirmation and per-tool busy state. Old completions
cannot open or modify a newer preview. This resets presentation state; it does
not claim to cancel an already-submitted backend write.

Two added regression tests failed before the fix and pass after it. They cover
discovery loss during both apply and preview, rediscovery, and an old preview
response arriving while a new request is pending. Existing tests continue to
cover repeated confirmation, captured requests, selection changes, guided
setup and uninstalled/config-only tools.

### Segmented controls lacked tab keyboard navigation

The Changes / Full file control declared tab semantics but did not respond to
Left/Right/Home/End and put every option in the Tab sequence.
`SegmentedControl.svelte` now maintains one tab stop, moves focus and selection
with those keys, wraps arrow navigation, and leaves modified shortcuts alone.
Mouse behavior and styling remain unchanged. The shared correction also applies
to the same control in settings and provider usage configuration.

The new keyboard regression failed before the fix and now passes. The real
App's JSON preview was also exercised with ArrowRight and Home in all eight
viewport/locale/theme combinations; focus, selected state, tab order and actual
full-file/diff content are asserted.

## Other checks

An additional App regression checks that clearing proxy usage updates the chart
without advancing periodic timers. It passed before any production change:
`endServerMutation` already schedules an immediate refresh. No speculative fix
was made to that path. The existing whole-page mock tests cover clear/cancel,
chart refresh, tables, bind-address failure/retry, Agent account switching and
rollback, provider creation/edit persistence, and virtualized log following,
manual scroll anchoring and refresh failure.

The source review also rechecked preview generation guards, diff coordinates,
literal full-file rendering/escaping, tab resets, and log refresh cancellation.
The native tests exercise current auth cancellation/locking, toast lifetimes,
focus/hover, menu/filter keyboard handling and animation milestones. No new
native production source was changed in this review.

## Validation evidence

All checks ran natively on macOS. Homebrew Rust remains the configured toolchain.

| Check | Result |
| --- | --- |
| Desktop tests before new regressions | 50 files / 441 passed |
| Added bug reproductions against pre-fix code | 3 failed as expected: 2 discovery cases and 1 keyboard case |
| Complete desktop suite after fixes | 51 files / **445 passed** |
| `cargo test -p aipass-desktop-native --features parity-render` | **34 passed**: 16 library and 18 executable tests |
| Desktop typecheck | 0 errors, 0 warnings |
| Desktop Vite production build | Passed; existing large-chunk advisory remains |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| `pnpm repo:check` | Passed: 776 sources, 19 tracked oversized files |
| `git diff --check` | Passed |

Browser evidence under `artifacts/desktop-parity/`:

- `bug-review-json-20261112`: **16/16** spacing and JSON-preview groups, with
  screenshots, field/card geometry, keyboard interactions and command traces.
- `bug-review-business-20261112`: **48/48** populated business groups, covering
  two sizes (960×640, 1280×820), English/Chinese and light/dark.

The browser reports hash their input sources before and after execution. A
combined source archive and validation record are retained alongside the JSON
review. The immutable reference remains `65a1f1f5a5e1b51311132659f9b972131c8ea551`;
candidate changes are explicitly separate from it.

## Remaining acceptance boundary

Passing these checks is not a proof that the entire application is bug-free.
The browser runs use production Svelte components with synthetic, in-memory IPC;
they do not exercise live vendor services or writes to a personal vault/CLI.
The native tests cover implemented components only. GPUI local proxy, business
Agent/provider pages and quick-integration/diff views remain unimplemented, so
the migration still cannot be accepted or cut over from Tauri.

Native inspection was retried. Computer Use still returned no apps/browsers,
`Sky Computer Use native pipe startup failed` and `The browser access host is
unavailable`. No new actual native-window interaction or screenshot is claimed.
This is an inspection connection failure, not evidence of a locked screen.
