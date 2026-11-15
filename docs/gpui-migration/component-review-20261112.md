# Alignment and populated business review

Worktree: `/Volumes/BRData/projects/aipass-gpui`, branch `refactor/gpui-desktop`.
Original reference: `65a1f1f5a5e1b51311132659f9b972131c8ea551`.
The original `/Volumes/BRData/projects/aipass` worktree remains clean.

## Scope and evidence boundary

This round addresses the user's marked titlebar edges and checks populated local
proxy, Agent management and provider forms. It uses synthetic in-memory IPC data;
no personal vault, API account, CLI configuration or running proxy was changed.
Fixture endpoints use `.example.invalid`, and all credential values are synthetic.

**The GPUI business pages remain unported.** The populated tests below exercise
the real Svelte App and its components through a test-only bridge. They establish
behavior and expose defects to avoid carrying into the port. They do not prove
GPUI data rendering, writes, charts, dialogs, or Agent integration. The native
toolbar still has unimplemented Add/Connect/Settings operations. The migration
remains unaccepted; Tauri has not been removed from the production entry point.

## Changes

| Surface | Change | Renderer actually changed |
| --- | --- | --- |
| Titlebar | Move the logo/search left edges and menu SVG/Add right edges onto common inset lines; account for the pane's 1 px border | GPUI |
| Proxy header | At the minimum desktop pane width, preserve the full title and all labeled actions in compact rows | Svelte candidate |
| Usage chart | Resolve hover/focus data by bucket date from the latest series; update totals in place and drop expired buckets | Svelte candidate |
| Agent usage | Send local-minus-UTC offset, with explicit daily granularity; match the proxy's reporting window | Svelte candidate |
| Provider form | Scroll newly reported validation/save errors into the visible form body while preserving the draft | Svelte candidate |

At `960x640`, the original proxy header clipped the English title. An intermediate
CSS attempt preserved the title but stacked every action vertically; its screenshot
was rejected and replaced by the final compact rows. Controls, labels and actions
remain the same. The usage table itself needed no CSS change: the first apparent
inset mismatch was caused by the test's automatic horizontal scroll. Resetting its
scroll position exposed the correct existing inset; tests now inspect both ends.

The timezone issue is a real request-contract defect: Singapore sent `-480` from
AgentAccountDetail and `+480` from the proxy page. Backend `usage_window_start`
uses local-minus-UTC. Four unit cases now cover JS offsets `-480`, `300`, `-345`
and `0`. Chart regressions cover focused refresh, clear, and bucket expiry.

## Native alignment

`artifacts/desktop-parity/alignment-20261112/` retains eight paired reference/GPUI
captures, raw Metal readback, geometry, diff images, source archive and executable.
The original Svelte source was unchanged during this capture.

- Two sizes (`960x640`, `1280x820`), two locales and two themes.
- `audit-alignment.mjs` checks 16 edge constraints; every logical-pixel delta is 0.
- The menu's 16 px SVG box is centered in a 28 px hit target. Internal SVG stroke
  insets are preserved; the assertion is about layout boxes, not painted pixels.
- Executable SHA-256: `40e358fe5f9797a1a18ce41d05dd3ef9ec7ae799cce01b199611c12d2de7d5d6`.
- Installed that exact executable into the separate `AIPass GPUI Parity.app`
  fixture. Actual native screenshot verified the new edges, and opening/dismissing
  the menu verified its anchor. Screenshots from CUA are in the conversation;
  the saved PNGs in the round are offscreen captures, not native-window captures.
- A separate real Edge interaction with the synthetic original App verified
  preview cancellation and reopening. This is browser evidence, not GPUI evidence.

## Populated contracts

The full App fixture includes a long provider title, two credentials on one
provider, a second protocol/provider, an archived provider with historical usage,
enabled and disabled route groups, available and cooling channels, large counters,
three consistent usage periods, installed and stale-config-only Agents, and 1,000
synthetic log rows. Unknown commands fail and remain in the command trace.

The browser checks cover:

- Nine-column table header/cell alignment, numeric right alignment, preserved
  credential order and archived labels, both horizontal scroll endpoints and no
  document overflow; chart/summary/table consistency for all three periods.
- Event-driven counter/chart/tooltip refresh; stop, failed bind save, retry,
  route toggle, clear cancellation and one confirmed clear.
- Agent credential selection without changing the active account, preview
  cancellation without a write, switching to the second credential with the
  exact preview ID, corresponding usage totals, failed apply, conflict and
  restoration, and unmanaged state.
- Log virtualization, tail following on append, manual scroll preservation,
  refresh failure with retained content, and closing the dialog.
- Provider dialog bounds and input alignment; invalid URL without a write;
  failed creation with retained title/key and visible error; retry and selection
  of the created provider; header merging and preservation of untouched WebSocket
  and concurrency settings on edit/reopen.

`business-baseline-final-20261112` completed 40 contract groups with no harness
errors, while retaining 16 findings (the stale tooltip and wrong Agent timezone,
each in eight combinations). Its `source/` directory freezes every baseline input.
Passing other contracts does not turn those known defects into passes.

`business-candidate-final-20261112` completed all 48 correction groups, adding the
live-log checks. `business-candidate-regression-20261112` then completed all 48
again after preventing a cleared tooltip from resurfacing when new data arrives.
Both matrices have zero errors and zero findings. Their reports label
`candidate: true`, `baselineOnly: false`, and record
explicit before/after hashes for the intentional Svelte corrections. It still
labels `nativeInteractionVerified: false`. Screenshots, geometry and synthetic
command traces are retained for every combination. The final regression also
retains `inputs.zip` with matching source/fixture hashes. Intermediate failed
probes are retained separately and do not count as successful comparisons.

Test harness corrections are not product defects: invalid initial mock enum
values were fixed and the fixtures now pass TypeScript; freezing RAF stranded
dialog pointer cleanup, so this suite fixes Date only and uses real timers;
the first lazy log-module load caused Vite dependency optimization/reload, so that
failed probe was retained and the warm run rerun. A real interaction confirmed
that cancellation/reopening worked before changing product code.

## Validation and reproduction

Logs: `artifacts/desktop-parity/business-validation-20261112/`.

- Desktop affected regressions: 22 files, 188 tests passed, including the six new
  timezone/chart cases. After the final clear/repopulate guard, the 10 chart
  regressions and desktop typecheck passed again. Unaffected passing results are
  reused. These are component/application tests with mocks.
- Native tests: 34 passed. Native screenshot/menu checks are separate above.
- Desktop typecheck and typed business fixtures passed.
- `cargo fmt --all -- --check` passed.
- `cargo clippy --workspace --all-targets -- -D warnings` passed on native macOS
  Homebrew Rust, plus the native crate's all-targets `parity-render` Clippy pass.
- `pnpm repo:check` passed, 774 production source files and 19 existing tracked
  oversized files. No limits or lint suppression were increased.

With the reference Vite server running, use the repository's RTK wrapper:

```sh
rtk proxy pnpm exec tsc --project tests/desktop-parity/tsconfig.json
rtk proxy node tests/desktop-parity/audit-business.mjs unique-candidate-round --candidate
rtk proxy node tests/desktop-parity/audit-alignment.mjs alignment-20261112
```

The strict original-reference capture tools intentionally reject the Svelte
changes in this worktree. Run original-reference checks against the frozen
reference revision, not the corrected candidate; do not regenerate the inventory
from this worktree or overwrite old screenshots. The new candidate switch only
permits the explicitly listed correction paths and verifies all hashes again at
the end of the audit.

No commit, push, release, updater installation, artifact runtime gate or remote CI
was performed in this round. These checks do not validate live vendor APIs or
personal-vault behavior, and do not complete the native business migration.
