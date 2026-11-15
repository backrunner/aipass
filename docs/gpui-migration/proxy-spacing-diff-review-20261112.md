# Local proxy spacing and JSON preview review

Worktree: `/Volumes/BRData/projects/aipass-gpui`, `refactor/gpui-desktop`.
Reference: `65a1f1f5a5e1b51311132659f9b972131c8ea551`.

The Svelte candidate now restores the original proxy header spacing and aligns
the endpoint/token field edges. The production JSON diff component passed two
complete browser rounds and a final regression using synthetic documents processed by the real Rust
diff generator. No JSON viewer production change was necessary.

**This does not accept the native migration.** Local proxy, Agent business
panels and quick-integration/diff views still have no GPUI implementation.
Native replacement acceptance remains 0/98. The checks below exercise the real
Svelte App under Playwright WebKit with an in-memory IPC bridge, not a packaged
Tauri app or a GPUI business page. Real config writes and vendor connections
were not performed.

## Corrections and measurements

The preceding minimum-width heading correction accidentally changed vertical
padding from 18 to 14 px and header gaps from 10/12 to 8 px. Removed those
overrides while retaining its three-row layout below a 440 px detail-pane width,
so the heading and all actions remain visible at the 960×640 window minimum.
This three-row layout remains an intentional difference from the frozen
reference, whose English heading was squeezed by its actions. At 1280×820 the
original two-row layout remains.

The quick-integration endpoint row had one 26 px action, while the token row
had two actions and a 4 px gap. Their automatic action columns made the fields'
right edges differ by **30 px**. Both now reserve 56 px and right-align their
actions. Measured field-edge difference is **0 px** in all eight combinations.

| Element | Measured logical spacing |
| --- | --- |
| Workspace | 8 px outer padding and pane gap |
| Proxy header | 18 px vertical / 28 px horizontal padding; 10 px row / 12 px column gap |
| Header actions | 8 px gap |
| Detail body | 18 px top, 28 px sides/bottom; 14 px between every card |
| Status grid | 12 px vertical / 16 px horizontal padding; 12 px grid gap |
| Chart, usage breakdown, integration context | 12/16/14/16 px top/right/bottom/left padding |
| Quick-integration tool rows | 10 px vertical / 16 px horizontal padding; 12 px row gap |
| Endpoint/token rows | 12 px gap; 56 px action column |

The audit records actual rectangles plus computed display, gap, padding,
margin and radius for 19 selectors. Those properties match the immutable
reference after the correction. It separately asserts card left/right edges,
all three 14 px card gaps, heading visibility, field alignment and viewport
overflow. It records both reference and candidate screenshots at the top and
quick-integration scroll positions. The new action-column minimum width and
the minimum-width heading reflow are deliberate corrections, not hidden by a
pixel-difference threshold.

## JSON preview coverage

`generate-preview-fixtures.mjs` creates synthetic, already-redacted JSON and
passes it through `aipass-config-writers::diff_preview_from` using the
`parity_preview` example. The UI consumes the resulting actual compact-diff
format with two-character markers and real hunk coordinates. It never diffs
the user's credentials in JavaScript.

The full App's local-proxy quick integration opens the shared production
`IntegrationPreviewDialog` and `DiffViewer`. The checked cases include:

- Existing JSON: nested objects/arrays, an unchanged redacted token, changed
  endpoint/model, 52 preserved properties, a long replacement value and a
  removed object. Every displayed line's number and literal text is checked
  against its original document. Four change groups, seven removed lines and
  four added lines are expected. Unchanged properties stay context rows.
- New file, deleted file, and unchanged file, including blank cells on the
  opposite side and switching from the unchanged placeholder to full content.
- Identical 24 px row heights and vertical coordinates on both sides; character
  highlighting and JSON syntax highlighting; Unicode and HTML-looking text
  stay literal text without creating executable DOM nodes.
- Vertical scroll synchronization in both directions; long horizontal scroll
  while the shorter pane clamps; return to zero; next-change navigation.
- Full-file mode reproduces the entire literal JSON including indentation and
  trailing newline. Parsing it yields the expected document. Returning to
  Changes renders the comparison again.
- Read-only preview has no write action; cancelling issues zero apply calls.
  Reopening resets file and display mode. Pending confirmation disables the
  action and sends exactly one captured `{tool: "claude-code", routeId:
  "route-main"}` request. Synthetic success and failure remain visible in the
  tool card. Existing component tests also cover repeated confirmation and
  selection changes while requests are pending.

The dialog, footer, file tabs, panes and rows are recorded at 960×640 and
1280×820, English/Chinese, light/dark, Retina scale 2. These rounds use reduced
motion for stable geometry. They do not re-certify normal-motion transitions,
the native renderer, or real backend apply/rollback behavior.

## Rounds and reproducibility

Artifacts are under `artifacts/desktop-parity/`:

- `proxy-layout-probe2-20261112`: preserves the 30 px field mismatch and header
  spacing differences. JSON interaction stopped on an incorrect test selector
  targeting an inactive tab's navigation. It is not a passing acceptance run.
- `proxy-layout-probe3-20261112`: spacing repaired; test selector incorrectly
  requested a button instead of the segmented control's tab role. Fixed in
  the harness, not the product.
- `proxy-layout-probe4-20261112`: both groups pass at minimum English/light.
- `proxy-layout-round1-20261112`: **16/16** groups pass across all eight cases.
- `proxy-layout-round2-20261112`: **16/16** pass after making the JSON route a
  dedicated, reproducible `business-json` scenario. Every case includes
  screenshots, geometry and an IPC trace. No unsupported mock commands.
- `proxy-layout-final-20261112`: **16/16** pass after tightening the harness
  to reject missing components or different component counts explicitly.

Each full round hashes all inventory reference inputs and new test inputs,
then verifies the hashes again after the run. The second and final rounds have an
`inputs.zip` and archive manifest preserving candidate and frozen source.
The baseline server loads Git objects from the pinned reference and checks
each against the inventory hash; it does not overwrite the candidate or
regenerate the baseline from changed source.

From the worktree root, run the servers in separate terminals:

```sh
rtk proxy node node_modules/vite/bin/vite.js --config tests/desktop-parity/vite.config.mjs
rtk proxy env AIPASS_PARITY_BASELINE=1 node node_modules/vite/bin/vite.js --config tests/desktop-parity/vite.config.mjs
```

Then use a new artifact directory name:

```sh
rtk proxy node tests/desktop-parity/generate-preview-fixtures.mjs
rtk proxy node tests/desktop-parity/audit-proxy-layout.mjs proxy-spacing-new-round --strict
```

The candidate JSON scenario is available at
`http://127.0.0.1:14731/?scenario=business-json&locale=zh-CN&theme=dark`.
Select Local Proxy, scroll to Quick integration, and preview Claude Code.
All state and apply results on this fixture page are synthetic and ephemeral.

## Validation and open boundary

- Desktop typecheck: 0 errors, 0 warnings.
- Six affected existing test files: **28 passed** (ServerDetailPane,
  IntegrationCard, IntegrationPreviewDialog, DiffViewer, config-diff, highlight).
- `cargo test -p aipass-config-writers diff_tests`: **5 passed**, including
  existing directory-isolation tests selected by that module filter.
- Fixture TypeScript check: passed.
- `pnpm repo:check`: passed, 775 production sources, 19 tracked oversized files.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed natively on
  macOS using `/opt/homebrew/bin/cargo`, Homebrew Rust 1.99.1.
- `git diff --check`: passed. Original worktree remains clean. No commit,
  push, release, personal vault mutation or real CLI configuration write.

Actual desktop interaction was attempted twice. Computer Use reported
`Sky Computer Use native pipe startup failed`; inventory returned no apps and
`The browser access host is unavailable`. No new native-window screenshot or
interaction is claimed. This is a tool connection failure, not evidence that
the screen is locked. Full native proxy/diff acceptance remains pending both
implementation and a working native inspection connection.
