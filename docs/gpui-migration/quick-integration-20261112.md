# Native automatic Quick Integration

Worktree: `/Volumes/BRData/projects/aipass-gpui`, branch `refactor/gpui-desktop`.
Base: `65a1f1f5a5e1b51311132659f9b972131c8ea551`.
This follows [credential editing](credential-workflow-20261112.md). It adds the
automatic API integration flow; it does not accept the entire provider surface
or replace the Tauri production entry point.

## Implemented

- The 16 automatic tools reuse the repository's Rust installation detector and
  config writers. Tool IDs are mapped explicitly: detector snake_case and Agent
  kebab-case are different contracts. Config files alone are not installation
  evidence; Cursor requires its local Agent runtime.
- Compatibility follows each key's effective protocol/authentication and endpoint.
  Tools requiring a model remain visible but disabled when the model is missing.
  Multi-key providers require an explicit stable key ID. Codex supports auth.json
  and experimental bearer configuration modes. Tool SVGs are shared with Svelte.
- Preview, confirm and restore use typed authenticated Agent IPC. The UI retains
  the exact preview request and ID. Provider/key/model/endpoint changes, lock and
  stale replies invalidate it. Read-only previews and missing preview IDs cannot
  submit a write. Duplicate submits are blocked. Agent errors remain retryable.
- Codex/Claude status, environment override warnings and status-owned restore IDs
  are displayed. Applied-path feedback is dismissible and expires after four
  seconds; the restart reminder remains until the selection changes.
- The native dialog supports multiple files, changes/full-file views, real hunk
  line numbers, paired replacements, Unicode-safe changed-word ranges, syntax
  colors, change navigation, synchronized horizontal/vertical scrolling and
  hatched empty cells in new/deleted files. Changes and complete-file modes both
  support keyboard scrolling. LCS alignment has a fixed memory budget.
  JSON is rendered as literal text, including markup-like strings.

Agent `handlers/tools.rs` now validates any supplied preview ID before generic
Quick Integration writes. Previously it only validated generic managed switches.
Legacy callers without a preview retain their previous contract. This is a
preimage check, not a claim that arbitrary external editors participate in a
cross-process atomic file lock.

## Evidence

The reference is frozen Svelte code at the baseline, fed the same synthetic
provider catalog and redacted Rust-produced diffs as GPUI. No reference pixels
are injected into native output. Reduced-motion offscreen captures do not prove
native input, accessibility, compositor animations or packaging acceptance.

| Round under `artifacts/desktop-parity` | Result |
| --- | --- |
| `integration-native-20261112-r1` | Harness attempted to expand an already expanded card, leaving its contents inert; no capture pass. |
| `integration-native-20261112-r2` | Five minimum-window pairs. Review found unequal file-tab columns, an extra 4 px code gap, and 5 px excess footer height. |
| `integration-native-20261112-r3` | Compile failure while measuring native file-tab labels; no capture pass. |
| `integration-native-20261112-r4` | Five minimum-window pairs after corrections. Header, file bar, code and footer bounds all match the reference. Manual review led to using the platform monospace font and compact inline word highlights. |
| `integration-native-20261112-r5` | 40 pairs across two sizes, two locales and two themes; no capture errors. All four named geometry regions match exactly. Review found missing empty-cell hatching, which was then implemented. |
| `integration-native-20261112-r6` | 40 pairs with hatching and corrected key-picker focus; all four regions still match. Manual review caught native segment selection color/weight and unequal mode-tab widths, plus transient reference tab colors. |
| `integration-native-20261112-r7` | Final 40-pair matrix passes. Selected file/mode/path, exact reference key/mode request, viewport and native state checks pass; all 160 named region bounds have zero delta. Source/binary hashes match the final tree. Five representative pairs were manually inspected; see `manual-review.json` beside the captures. |

The reference tab's `aria-selected` and file content changed before its CSS color
transition completed. The harness now fast-forwards finite animations and checks
selected file, mode and path, so stable-state comparison cannot mistake that
transition for the target styling. Native mode/file segments now use matching
selection colors, medium weight, equal columns and active shadows.

The manually reviewed final pairs cover English/light diff, English/dark
read-only and new-file views, Chinese/dark unchanged state at 960×640, and
Chinese/light complete-file view at 1280×820. These checks establish the named
geometry and state results, not exact typography or complete visual acceptance.
The comparison gallery is `artifacts/desktop-parity/integration-native-20261112-r7/index.html`.

The final source checks pass on macOS with Homebrew Rust and Node 26:

| Check | Result and evidence |
| --- | --- |
| `cargo fmt --all -- --check` | Passed; `final5-rust-0.log`. |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed with no warnings; `final5-rust-1.log`. |
| Native all-target Clippy with `--features parity-render` | Passed; `final5-rust-2.log`. |
| `cargo test --workspace` | Passed; `final5-rust-3.log`. Four existing opt-in CLI/browser/Keychain cases remain ignored. |
| Native tests with `--features parity-render` | 41 library + 43 binary + 1 authenticated Agent workflow passed; `final5-rust-4.log`. The child workflow output is not counted twice. |
| Workspace and native parity builds | Passed; `final5-rust-5.log`, `final5-rust-6.log`. |
| Node install, license audit, lint, typecheck, tests and build | Passed; `node-validation.json`. Embedded control-panel output remains unchanged. |
| Repository source-size/ownership gate, parity TypeScript and diff whitespace | Passed. |

This is source validation. The macOS desktop artifact/runtime release gate and
remote CI were not run, and the migration is not ready for production cutover.

The Agent handler regression uses a disposable vault/home, synthetic installed
CLI and the existing test-only credential-store isolation. It exercises redacted
preview, external-file rejection without byte changes, refreshed apply, encrypted
backup, byte-exact rollback and invalidation after rotating the same secret ID.

The native transport test starts a real authenticated Agent in disposable child
processes. Preimage conflict and lock rejection are exercised over IPC. In this
environment a fresh write is refused because the disposable HOME has no default
Keychain; the test checks the exact error and unchanged bytes. It does **not**
claim real-Keychain apply/rollback success. No recovery safeguard is disabled and
no personal vendor profile or login Keychain is used by this isolated fixture.

The first complete workspace test run encountered an existing Native Host fixture
that uses the host HOME. A process sample showed `SecItemCopyMatching` blocking
unlock during recovery audit. That owned test process was stopped; the failed run
and stack sample are retained. The rerun uses a disposable HOME and the existing
Cargo registry with native Homebrew Rust.

A later validation wrapper incorrectly retained the long macOS temporary path
and assigned one global XDG directory to all per-vault fixtures. Its failed run
is retained as `final-rust-*`; it is not a product failure or a passing gate.
The corrected wrapper uses `TMPDIR=/tmp`, a short disposable HOME and no XDG or
vendor overrides. Its next run exposed a pre-existing fixture race in the Claude
bridge plan-approval test: file existence was observed before the shell finished
writing JSON. The fixture now publishes the mode and acknowledgement files by
atomic rename, without changing production bridge behavior.

Logs: `artifacts/desktop-parity/integration-delivery-20261112/`.
Computer use returned `Sky Computer Use native pipe startup failed` and
`The browser access host is unavailable`. This is a connection failure, not
evidence that the Mac is locked. No installed-app replacement or release occurred.

## Remaining acceptance work

- Six manual integration guides and subscription/OAuth login/reconnect flows.
- Real-window confirmation, focus restoration, arrow-key navigation between
  file/mode tabs, text selection/copy, scrollbar fades, remaining hover styling,
  dialog shadow/backdrop blur, disclosure/dialog animation and
  status refresh on window activation. The shared diff parser and native scroll
  event checks do not establish these unimplemented visual/interaction details.
- More populated-card and post-write/status comparison cases. Current screenshot
  acceptance is limited to the recorded dialog geometry and inspected examples;
  it does not accept all card states or typography.
- Native local proxy and Agent pages with live charts/tables/logs, settings,
  clipboard ownership, updater/tray, lifecycle and packaging cutover.

The migration inventory remains **0 of 98 fully accepted native replacements**.
To reproduce the dialog matrix with the frozen Vite reference on port 14732:

```sh
rtk proxy node tests/desktop-parity/capture-integration.mjs NEW-ROUND --strict-geometry
```

Use `--probe` for five English/light pairs at 960×640. The full matrix uses
960×640 and 1280×820, English/Chinese, light/dark. Round names must be unique.

## Requested preview copy reduction

The subsequent user request removes the repeated tool/provider subtitle and
automatic-backup boilerplate from both native and Svelte previews. Necessary
credential-access notices and environment-loading commands remain. Header
alignment and the obsolete 4 px footer margin were adjusted to reclaim the space.
This is an intentional copy/layout change from the frozen baseline above.

`artifacts/desktop-parity/preview-copy-20261112-r2/` records eight current-Svelte
versus GPUI offscreen cases at 960×640: writable/read-only, English/Chinese,
light/dark. All named geometry bounds match; English/light writable and
Chinese/dark read-only pairs were manually reviewed. Native-window input remains
unverified. The first copy-capture round caught the leftover footer margin.
Desktop typecheck, 49 related desktop tests, six native integration tests and
`cargo clippy --workspace --all-targets -- -D warnings` pass. The obsolete subtitle
test was removed; the stale-preview regression now verifies the fresh file path.
