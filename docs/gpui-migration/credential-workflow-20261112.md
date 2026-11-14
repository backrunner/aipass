# Native provider credential workflow

Worktree: `/Volumes/BRData/projects/aipass-gpui`, branch `refactor/gpui-desktop`.
Base: `65a1f1f5a5e1b51311132659f9b972131c8ea551`.
This follows the [provider workflow](provider-workflow-20261112.md). It implements
the API credential editor and its Agent operations; it does not accept the whole
provider surface or replace the Tauri production entry point.

## Implemented behavior

- API credential rows now support add, edit, reveal/hide and removal while the
  provider is being edited. OAuth entries do not expose this API key editor.
- Each key has a label, protocol, group, endpoint and model override. Existing
  keys also have a collapsible billing editor for rate, currency and unit price.
  Saving preserves an existing billing note; clearing overrides restores
  inheritance. The protocol menu supports arrow keys, Home/End, Enter and Escape.
- Reads and mutations use the existing typed, authenticated Agent IPC. Reads use
  exact stable secret IDs, including when another key's label equals an ID.
  The native UI does not own vault persistence or load a provider engine.
- Empty labels/keys and malformed or embedded-auth endpoint URLs are rejected.
  A failed save preserves the draft, and duplicate submission is blocked.
  Saving the enclosing provider first commits an open credential editor and
  resumes the provider save only after success.
- Requests carry provider and generation scopes. Switching provider, closing the
  editor, replacing the key during refresh, or locking invalidates late replies.
  Lock/selection teardown clears plaintext, input values and undo history.
  Search and background deletion now close a form belonging to the previous
  selection rather than leaving an old provider's form or key visible.
- Revealed keys expire after 30 seconds and can be hidden immediately. Long
  revealed values have a measured horizontal scroll range instead of being
  clipped into an inaccessible tail. This timeout is not yet wired to settings.

The implementation is split between `src/credentials.rs` (Agent operations),
`src/providers/credentials.rs` (transient state), and its `view.rs` and `tests.rs`
modules. Shared menu keyboard handling moved to `src/menu.rs`.

## Comparison rounds

All captures use the same synthetic provider fixture in the frozen Svelte
reference and GPUI renderer. The displayed key is explicitly synthetic; no
personal vault, vendor upstream or system clipboard is used. Captures use reduced
motion and do not certify animation or real-window interaction.

| Directory under `artifacts/desktop-parity` | Evidence and corrections |
| --- | --- |
| `credential-native-20261112-r1` | Six minimum-window pairs. Review found an incorrectly centered, unbordered billing header, stretched Add control, small password dots and excess field line height. |
| `credential-native-20261112-r2` | Build failure retained as diagnostics; the layout observation hook was attached after converting a Div to Stateful. No screenshot pass is claimed. |
| `credential-native-20261112-r3` | Six pairs after the first corrections. Measured reference field heights, label/hint line heights and placeholder colors; corrected accumulated vertical offsets. |
| `credential-native-20261112-r4` | Six pairs; observed editor label, inheritance hint and billing bounds passed the 4 CSS px guard. Manual billing/reveal review exposed missing rate/currency placeholders, subsequently added. |
| `credential-native-20261112-r5` | All 48 pairs passed capture/state/geometry checks with no page errors. Observed editor bounds differed by at most 1 CSS px. Manual review found the Save control's missing disabled opacity and oversized height/font, plus an extra expanded-billing divider. |
| `credential-native-20261112-r6-editors` | All 24 affected add/edit/billing pairs passed after correcting Save height/font/padding/disabled opacity and the billing divider. No page errors; observed editor bounds differ by at most 1 CSS px. Source hashes match the final code. |

The harness moves the reference pointer to a neutral location before capturing
an idle frame; otherwise the last click leaves the billing header hovered while
the offscreen native frame has no hover. Hover acceptance remains separate.
Collapsed billing summaries use separate rate/currency/price fields, avoiding
dotted Chinese text. No reference pixels are injected or images resampled to
create agreement.

The reference's computed Save style is 26 px high, 12 px text, 9 px horizontal
padding and 0.45 opacity when disabled. Its expanded billing header computes a
transparent 1 px bottom border; the native header now retains that spacing with
no visible divider. These corrections are based on rendered style observations.

The matrix covers detail, provider edit, key add, key edit, expanded key billing
and reveal at 960×640 and 1280×820, English/Chinese, light/dark. It checks rendered
state, pending state, observed editor geometry, page errors and unsupported mock
IPC. Source/binary/image hashes, reference traces and native bounds are retained.
Capture success is not whole-surface acceptance; the manual review file names
the inspected pairs and remaining differences.
R5 has three manually inspected pairs; R6 has four, spanning both sizes,
locales and themes and all three affected editor states. The remaining pairs
have automated capture/state/geometry evidence only.

## Business and regression validation

The native suite passes **69 unique tests**: 31 library, 37 executable/GPUI and
one real-Agent workflow. The nested child process prints the integration result
again; it is counted once. Tests cover validation, failure/retry, duplicate
submission, stale responses after cancel/selection/key replacement/lock, input
and undo cleanup, reveal expiry, protocol keyboard navigation, long-key scroll
extent, composite-save ordering, and selection changes during editing.

`tests/provider_agent.rs` now includes `tests/provider_agent/credentials.rs`.
The real Agent runs in a disposable directory with isolated HOME/vendor/runtime
paths and a local-only vault. It checks add, duplicate-label rejection, stable-ID
rotation, protocol/override/billing persistence, an ID-versus-label collision,
exact-ID reveal/removal, locked read/write rejection, and unlock persistence.
The owned child and disposable directory are cleaned up. This exercises real
authenticated IPC and storage, not a mocked success result.

Logs: `artifacts/desktop-parity/credential-delivery-20261112/`.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| Native all-target Clippy with `--features parity-render -- -D warnings` | Passed |
| `cargo test --workspace` | Passed |
| `cargo test -p aipass-desktop-native --features parity-render` | Passed, 69 unique tests |
| `cargo build --workspace` | Passed |
| `pnpm repo:check` | Passed, 798 source files and 19 existing no-growth exceptions |
| Parity fixture TypeScript check | Passed |
| Production Node install/license/lint/typecheck/test/build and embedded output | Reused passing provider-delivery checks; their production inputs are unchanged in this follow-up |
| macOS artifact startup/update gate; remote CI | Not run |

All new Rust checks ran natively on macOS with Homebrew Rust. The source gate
does not establish packaging or installed-app runtime correctness.
After the final rendering-only Save/divider correction, fmt, both Clippy gates
and the repository gate passed again (`style-validation.json`). The 69-test
business/state result is reused; the affected editor presentation is recaptured.

Computer use was retried this turn: `Sky Computer Use native pipe startup
failed` and `The browser access host is unavailable`, with no app/browser
surfaces. This is a tool connection failure, not a claim that the Mac is locked.
GPUI test-platform events and offscreen GPU images do not replace native window
input, accessibility, compositor animation or installed-app acceptance.

## Remaining work

- Native clipboard ownership, confirmation and cleanup, key Copy and pricing
  actions, and endpoint Copy. Those controls are still absent, making the key
  value area wider than the reference. No clipboard service is faked.
- Selectable revealed text, scroll fades, full hover/focus styles, disclosure
  animation and preference-driven reveal expiry. Credential feedback currently
  uses inline notices/errors rather than the reference toast presentation.
  Long masked-input clipping, glyph metrics and Chinese font rendering still
  differ; the measurements above do not accept typography parity.
- Provider model aliases, header editors, inference/probing, complete logos and
  full Agent-backed search/filter/recent-use semantics.
- Native local proxy and Agent business pages, live charts/tables/logs/text,
  subscription operations and quick integration plan/apply/rollback/JSON diff.
  The subsequent [quick integration follow-up](quick-integration-20261112.md)
  implements the automatic API flow; its remaining gaps and evidence are recorded there.
- Settings, recovery export/copy, updater, tray, lifecycle and packaging cutover.

The inventory remains **0 of 98 fully accepted native replacements**. The frozen
reference shows a black heading in the English/dark reveal capture but a readable
heading in the Chinese/dark billing capture; native keeps the heading readable.
Missing lower-page business components remain visible comparison gaps.
No commit, push, release, installed-app replacement or personal data mutation was
performed.

Reproduce the credential-only comparison matrix with the frozen-reference Vite
server described in the migration README running:

```sh
rtk proxy node tests/desktop-parity/capture-providers.mjs NEW-ROUND --credentials-only
```

Add `--probe` for six English/light 960×640 pairs, or `--key-editors-only` for
the 24-pair add/edit/billing subset. Round names must be unique.
