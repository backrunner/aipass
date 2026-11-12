# Native provider workflow implementation

Worktree: `/Volumes/BRData/projects/aipass-gpui`, branch `refactor/gpui-desktop`.
Base: `65a1f1f5a5e1b51311132659f9b972131c8ea551`.
This advances the missing GPUI implementation. It does not accept the whole
provider surface or replace the Tauri production entry point.

Follow-up: [native credential workflow](credential-workflow-20261112.md) now adds
API key CRUD/reveal, protocol/override/billing editing and further Agent/state
regressions. The validation counts and remaining-work list below describe this
earlier provider round; use the follow-up for current credential status.

## Implemented behavior

- Typed Agent catalog reads for active, archived and trashed providers. Populated
  rows, selection, counts, favorites, provider-kind filters, basic metadata search and detail
  cards now render in GPUI. Archive/trash lists and restore use the same catalog.
- Native add dialog and inline edit form, including title, key name/API key on
  add, multiple API/console endpoints, domains, model, tags, multiline notes,
  protocol/auth compatibility, concurrency and WebSocket preference.
- Authenticated Agent add/update/favorite/archive/trash/restore operations.
  Persistence, secret handling and WebSocket recovery verification stay in the
  existing Rust Agent. The UI does not create a second storage authority.
- Validation and save errors preserve the draft; duplicate submission is blocked
  while saving. A successful add clears a query/filter that would hide the new
  entry; editing retains the selected filter. Escape/backdrop dismissal use
  the native dialog's focus handling.
- Successful mutations are acknowledged before catalog refresh. A later refresh
  failure cannot turn a successful add into a retryable duplicate save. Lock
  clears the catalog, selection, pending UI action, form and input undo history;
  stale completion generations cannot reopen the form or workspace.
- Untouched concurrency and WebSocket values are omitted from updates. Background
  catalog refresh updates those visible values and non-editable metadata without
  replacing user-entered fields. Account identity, existing header values,
  credential kind, secret metadata and subscription state remain Agent-owned.
- Escaped commas/backslashes in tags/domains and commas within endpoint queries
  round-trip. Multiple endpoints preserve existing IDs and metadata; inserted
  endpoints receive distinct IDs rather than stealing an existing endpoint ID.

The remaining fields and controls below are deliberately not represented as
implemented by a screenshot or by a passing source test.

## Comparison rounds and corrections

All fixtures are synthetic and shared by the frozen Svelte reference and native
renderer (`tests/desktop-parity/fixtures/providers.json`). There are two active,
one archived and one trashed entry; long titles, multiple credentials, metadata,
notes and domains use reserved `.invalid` addresses. No real credential appears
in the fixtures or screenshots.

| Round directory under `artifacts/desktop-parity` | Purpose / result |
| --- | --- |
| `provider-native-20261112-r1` | Initial probes; the first add capture incorrectly showed auth. Preserved the failed evidence and corrected workspace selection. Subsequent probes exposed wrapping and flex-card clipping. |
| `provider-native-20261112-r2` | Three matched minimum-window pairs. Corrected 56 px rows, header ellipsis, non-shrinking cards, form surface colors/default provider, and inline editing instead of an edit modal. |
| `provider-native-20261112-r3` | 24 matched pairs: detail/add/edit × two sizes × two locales × two themes. These captures predate the advanced-form and later style corrections. |
| `provider-native-20261112-r4-probe` | Five minimum-window pairs, including advanced forms. Revealed the stretched add-field button, misplaced select arrows, missing concurrency hint and duplicate Notes label. |
| `provider-native-20261112-r5` | Expanded 40-pair matrix. Corrected the above, list icon column width, filled favorite state and bordered More control. Review then identified accumulated field-line-height differences and the API reveal button's placement. |

`provider-native-20261112-r6` completed all 40 pairs without page errors. Its
large-window inspection exposed a fixed 760 px add-dialog height: the collapsed
form had excess blank space and the expanded form did not use the available
height. The final correction uses intrinsic content height with a viewport-minus-
32 px maximum and non-shrinking header/footer.

`provider-native-20261112-r7-add` repeats all 16 affected add/advanced-add pairs
(two sizes, two locales, two themes). All capture and geometry assertions pass;
header/body/footer x/y/width/height differ from the frozen reference by at most
2 CSS px. This focused layout guard does not accept missing controls, styles or
animations. Existing R6 detail/edit captures remain historical evidence; those
render paths were not changed by the final add-dialog sizing correction.

Representative minimum/large, English/Chinese and light/dark pairs were manually
reviewed across these rounds; not every captured pair has received manual review.
Each final directory includes `index.html`, `report.json` and a separate
`presentation-review.json` describing the exact review scope and open findings.
Capturing a pair is not a visual acceptance result.

The capture script requires two rendered active rows and the correct add/edit
state, viewport and scale. A successful process exit cannot count a blank/auth
screen as a provider capture. It retains source hashes, binary hash, paired
images, reference geometry and mock IPC traces. Provider fixtures reject a real
vault argument; they require the `parity-render` build feature.

The frozen dark reference itself shows a black detail heading. The native
heading remains readable; that reference defect is recorded rather than
silently calling it a pixel match. The bottom of an expanded edit page differs
because the reference includes subscription/proxy settings and quick integration
which are still missing natively. This is a component gap, not rasterization.

## Regression coverage and evidence boundaries

`apps/desktop-native/tests/provider_agent.rs` launches the real Agent in a child
process using a disposable vault, runtime directory and isolated HOME/vendor
paths. It creates a local-only vault, adds and edits a synthetic provider,
changes favorite/archive/trash state, restores it, locks, rejects a locked
mutation, unlocks and verifies persisted state. It uses real typed authenticated
IPC and no vendor upstream or personal vault. The owned child is stopped and
the disposable directory is removed. The nested client process prints its own
test result; it is one integration scenario, not two independent scenarios.

Native state and GPUI tests additionally cover catalog filtering, invalid keys,
URLs and concurrency, preserved credential metadata, escaped list parsing,
protocol/auth transitions, failed-save retry, duplicate submission, form dismissal
at 960x640, draft clearing, filtered add/edit behavior, stale completions and
background metadata refresh while editing.

GPUI test-platform input and offscreen GPU rendering are separate from physical
native interaction. Computer use was retried and returned `Sky Computer Use
native pipe startup failed` and `The browser access host is unavailable`, with
no available app/browser surfaces. This is a connection failure, not a new claim
that the Mac is locked. No new native mouse/keyboard acceptance, compositor
animation acceptance, installed-app verification or packaged runtime result is
claimed in this round.

## Still incomplete

- Provider credential copy/reveal/add/edit/remove and pricing actions; endpoint
  copy, protocol/capability probing controls, complete provider logo/favicons,
  model-alias/header/group/billing editors and inference behavior.
- Complete search/filter semantics: Agent-backed secret/fingerprint lookup,
  the reference's full metadata search fields and phrase matching, recent-use
  ordering, tag selection and non-RFC3339 expiration values are not ported yet.
- Full dropdown keyboard/selection parity, error/focus styles, number steppers,
  rounded-child clipping, all scroll/focus edge cases, and dialog/disclosure animations. Captures use
  reduced motion and do not certify animation parity.
- Subscription/proxy settings and quick integration/JSON diff on provider detail.
- Native Agent and local proxy pages, including live charts, tables, routes,
  logs and text updates; settings, recovery-kit export/copy, updater, tray,
  singleton/lifecycle and packaging cutover.

The source inventory still has **0 of 98 fully accepted native replacements**.
Implemented portions above are useful progress but cannot satisfy missing
component, runtime and interaction acceptance. No commit, push or release was
performed, and the original `aipass` worktree remains clean.


## Local validation

Logs are in `artifacts/desktop-parity/provider-delivery-20261112/`.
All checks ran natively on macOS with Homebrew Rust. The complete Node and Rust
source gate commands ran after the provider transport/dependency implementation.
Unchanged lint, typecheck and build tasks used Turbo's validated cache hits;
the Node tests ran. Final
native presentation and metadata-refresh changes then received current fmt,
full workspace Clippy, feature-enabled native Clippy and the entire native
suite again. After the final add-dialog sizing-only correction, fmt and both
Clippy gates passed once more, the 960x640 open/Escape regression passed, and all
16 affected layout captures passed. The other passing native state/Agent tests
and unchanged Node/core checks are reused rather than counted twice.

| Check | Result |
| --- | --- |
| `pnpm install --frozen-lockfile` | Passed |
| `pnpm licenses:audit`, `pnpm lint`, `pnpm typecheck` | Passed |
| `pnpm test` | Passed; desktop 457, extension 183, shared UI 8, control panel 23 |
| `pnpm build` and embedded control-panel diff | Passed; generated embedded output unchanged |
| Parity fixture TypeScript check | Passed |
| `cargo fmt --all -- --check` | Passed on final native source |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed on final native source |
| Native all-target Clippy with `--features parity-render -- -D warnings` | Passed on final native source |
| `cargo test --workspace` | Passed; 4 existing ignored tests retained |
| `cargo test -p aipass-desktop-native --features parity-render` | 56 unique tests passed: 28 library, 27 executable, 1 real Agent workflow |
| `cargo build --workspace` | Passed after provider transport/dependency implementation |
| Native `parity-render` build | Passed during final capture |
| Repository size/ownership gate | Passed; 793 source files, 19 existing no-growth exceptions |
| `git diff --check`; original worktree status | Passed; original worktree clean |
| macOS desktop bundle / packaged startup-update gate | Not run; incomplete native production packaging/lifecycle |
| Remote CI / native Windows or Linux | Not run in this turn |

To reproduce the synthetic preview without selecting a real vault:

```sh
rtk proxy cargo run -p aipass-desktop-native --features parity-render -- --parity-preview --scenario=providers
```

To reproduce comparison captures, start the frozen-reference Vite server from
the migration README, then run `rtk proxy node tests/desktop-parity/capture-providers.mjs NEW-ROUND`.
Use `--add-only` for the 16-case add-dialog layout regression. Round names must be unique; existing evidence is never overwritten. The original
installed app and the previous preview bundle were not replaced by this work.
