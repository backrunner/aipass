# GPUI migration acceptance

Reference: `65a1f1f5a5e1b51311132659f9b972131c8ea551`.
Worktree: `/Volumes/BRData/projects/aipass-gpui`, branch `refactor/gpui-desktop`.

This migration is **in progress, not accepted**. Tauri remains the executable
reference until every surface and lifecycle has a working native replacement.
The native executable is not yet the production desktop entry point.

## Contract

- Port the existing design, including colors, typography, icons, translucent
  backgrounds, spacing, clipping, scroll masks, focus, keyboard navigation,
  context menus, dialogs, drawers, title bar, tray and animations.
- Preserve Agent authority, authenticated typed IPC, secret clipboard ownership,
  lock invalidation, background work cancellation and existing settings formats.
- No WebView in the final GPUI desktop. Shared Svelte controls remain necessary
  for the separately shipped LAN control panel.
- Every inventory entry needs actual scenarios and a native implementation.
  A source inventory is a coverage floor, not a complete state-space test.
- Use identical synthetic fixtures, locale, viewport, display scale, font and
  OS version for each reference/native pair. Do not capture personal vault data.
- Cover 960×640 and 1280×820, English/Chinese, light/dark, reduced/normal motion,
  empty/populated/long content, loading/success/failure and disabled states.
- Perform three rounds: initial comparison, correction comparison, full
  regression after integration. Each round must retain both images, difference
  images, geometry, interaction traces and source/artifact hashes.
- Sample animations at 0/25/50/75/100 percent and at semantic boundaries (for
  example unlock at 540 ms and 660 ms of 1200 ms). Check cancellation, reversal,
  repeated activation, focus restoration and reduced motion separately.
- User clarification, 2026-11-12: exact pixels are not required. Components,
  layout, shape and style must remain consistent, as must the requested motion
  and interaction behavior. Review controls/states, hierarchy/alignment/wrapping,
  dimensions/radii/icon silhouettes, and typography/colors/borders/shadows.
- Pixel differences remain diagnostics. Rasterization, gradient quantization and
  device-pixel snapping may be recorded without failing acceptance when they do
  not alter those visual properties. Do not use a global error threshold to
  excuse missing components, changed geometry, colors or typography. Keep paired
  images, source hashes and explicit review notes for every dimension; unreviewed
  and mismatched items still block acceptance. Historical pixel-exact failures
  remain unchanged and are not automatically reclassified as accepted.

## Cutover order

1. Freeze all UI and lifecycle sources; record reference fixtures and captures.
2. Port design tokens, localization, native controls and animation curves.
3. Port authentication, workspace, providers, subscriptions, Agent management,
   proxy, pricing, settings, every nested dialog and notification.
4. Extract reusable platform services from the Tauri bridge and connect GPUI
   through the existing `AgentClient`; preserve macOS Swift tray/CloudKit code.
5. Replace singleton, launch, deep links, updater, bundle scripts and CI entry
   points; remove Tauri runtime and desktop JS dependencies after parity passes.
6. Run the complete macOS gate, native startup/update artifact acceptance and
   three full visual/interaction rounds. Remote Linux/Windows evidence is separate.

`inventory.json` is generated from the immutable reference, never the candidate.
Pending, absent, skipped or failed evidence blocks acceptance.

## Current evidence (2026-11-11–12)

The latest [native provider implementation](provider-workflow-20261112.md)
connects populated lists, add/edit forms, favorites, archive, trash and restore
to the existing Agent. It includes real IPC against a disposable profile and
matched WebKit/GPUI rendering fixtures. This is partial provider functionality;
credential actions, quick integration, Agent and proxy pages remain open.
The report distinguishes implementation progress from native acceptance.

The [complete follow-up review](complete-review-20261112.md) fixes five classes
of async feedback/preview/dialog defects, including proxy commands continuing
after lock. Full Rust and Node source checks pass; desktop tests are 457/457,
native tests 35/35 and WebKit groups 64/64 at that earlier revision. Native business pages were absent then,
and native recovery interaction and packaged-runtime acceptance remain open.


The [follow-up defect review](bug-review-20261112.md) fixes quick integration
remaining busy after Agent rediscovery and adds keyboard navigation to segmented
controls. The full desktop suite passes 445 tests; native tests pass 34. These
checks cover implemented code and do not accept missing native business pages.

The latest [proxy spacing and JSON diff review](proxy-spacing-diff-review-20261112.md)
restores the original proxy header spacing, fixes a 30 px field-edge mismatch,
and records three 16-case WebKit rounds using Rust-generated synthetic JSON diffs.
It does not accept native proxy views. The subsequent automatic Quick Integration
implementation and its remaining gaps are recorded below.

The latest [alignment and populated business review](component-review-20261112.md)
fixes the marked GPUI titlebar edges and checks proxy tables/charts, Agent account
switching and provider forms with stateful mock data. It also fixes original
Svelte usage/form defects in this worktree. Those candidate corrections are
recorded separately from the immutable reference and do not accept unported
native business pages. Strict original-reference captures now reject those
intentional source differences; use the candidate audit documented in the review.

The latest dynamic behavior audit is in [dynamic review](dynamic-review-20261112.md)
with a [timer and animation inventory](dynamic-inventory.json). Proxy/update
runtime parity is still missing; baseline-only tests do not accept it.
The preceding component/layout/shape/style review is in
[logo/auth follow-up](logo-auth-review-20261112.md), following the
[presentation review](presentation-review-20261112.md). Earlier pixel-exact
statistics below remain historical diagnostics, not the current acceptance gate.

- 98 reference source components inventoried. **0 accepted native replacements**.
- 24 WebKit reference captures in `artifacts/desktop-parity/reference/`, each
  accompanied by element bounds and the test bridge trace. Two viewports,
  two locales, two themes, and three initial screens. These are synthetic
  frontend fixtures, not captures of the packaged Tauri application.
- The native preview uses GPUI 0.3.12 (`gpui-pre` and its matching platform
  crate). The empty workspace now includes the sidebar, title/menu, search,
  filter, toolbar and empty-state controls. Authentication includes native
  password inputs, confirmation, reveal/hide and initial setup/unlock screens.
  These are partial implementations: the provider catalog and its basic mutation
  workflow are now connected. [API credential editing and reveal](credential-workflow-20261112.md)
  now use typed Agent operations, with isolated storage regressions.
  [Native automatic quick integration](quick-integration-20261112.md) adds installed-tool
  filtering, credential selection, redacted multi-file JSON diff and Agent
  preview/apply/rollback. Manual guides and subscription login remain open.
  Credential copy/pricing, subscription operations, settings, Agent/proxy panels, tray, updater and production
  packaging remain unported. Toolbar events for unported operations do not have implementations.
- A renderer-independent session controller uses the existing authenticated
  `AgentClient`. Auth and lock requests execute in order; stale auth completion
  cannot restore a locked workspace. The optional `--vault=/explicit/path` is
  development-only, requires an already-running Agent and has **not** received
  production lifecycle acceptance. Listed entries are now rendered; the new
  provider transport has a separate isolated real-Agent regression. Import,
  clipboard, recovery export, OS lock/sleep and lifecycle behavior are
  incomplete. Reset now has its confirmation panel and typed Agent path, with
  mock success/failure coverage; live destructive reset acceptance is pending.
  A failed lock currently stays hidden until retried; this differs
  from the reference's failure behavior and is **not accepted parity**.
- During the earlier unlocked-Mac inspection, native screenshots and input worked. Initial
  inspection reproduced missing menu dismissal, keyboard routing to an unmounted
  password input, missing search focus styling and menu alignment/navigation
  differences. Two correction inspections verified Escape/outside dismissal,
  disabled filtering on Favorites, search input, directional menu selection,
  Tab dismissal, Enter to lock, password focus after lock and Command-Q exit.
  This is a limited empty-workspace interaction check, not acceptance of every
  UI surface or lifecycle. See [native inspection](native-inspection-20261111.md).
  The latest credential round cannot connect to the computer-use native/browser
  hosts; its offscreen images and test-platform events do not extend that native
  interaction acceptance.
- A test-only Tauri example hosts the same synthetic frontend fixtures in an
  actual native WKWebView window. It does not initialize the production desktop,
  Agent, vault, plugins or tray. Native inspection confirmed the reference menu's
  Down selection and Tab dismissal/forward focus. Packaged Tauri lifecycle and
  full native pixel/animation comparison are still pending.
- The `parity-render` feature exports the actual GPUI offscreen GPU scene while
  locked. It checks content size and asset readiness, writes layout bounds and
  retains exact pixel differences. This is **not** a visible native-window
  screenshot and does not replace the final Tauri/native interaction rounds.
  GPUI exports are 1920×1280 and 2560×1640 at display scale 2.
- Four completed initial-screen comparison rounds, 24 pairs each. These are
  limited development rounds, not the required full native acceptance rounds:

  | Round | Pixel-exact passes | Different-pixel range | Average channel error (0–255) |
  | --- | --- | --- | --- |
  | `offscreen-initial-20261111` | 0/24 | 85.34–98.82% | 11.316 |
  | `offscreen-correction-20261111` | 0/24 | 26.87–90.79% | 1.181 |
  | `offscreen-regression-initial-screens-20261111` | 0/24 | 26.87–90.79% | 1.181 |
  | `offscreen-native-input-regression-20261111` | 0/24 | 26.87–90.79% | 1.181 |

  Reports, paired PNGs and difference PNGs are under `artifacts/desktop-parity/`.
  Correction reports include executable/source hashes and toolchain identity.
  The regression directory also contains the matching executable and `source.zip`.
  Its 24 candidate images are byte-identical to the correction candidates after
  the authentication state regression fixes. This proves fixture repeatability,
  not parity with the reference.
  The fourth round follows the native input fixes and retains its matching
  executable and source archive. 23 candidate images match the third round
  byte-for-byte; the remaining image differs at 68 pixels in the password caret
  rectangle `[624,674,626,708]` (physical pixels), with maximum channel error 28.
  The focus/caret capture phase is not yet synchronized with the reference;
  this difference is retained, not masked. See that round's `repeatability.json`.
  All eight workspace cases match the three pane outer rectangles exactly;
  this does not prove inner control geometry. All eight unlock cards and four
  Chinese setup cards differ in vertical position by 0.25 logical pixels.
  GPUI's device-pixel snapping, text rasterization, opacity composition and
  backdrop/shadow rendering still differ. No masks or relaxed thresholds were
  used. Hero motion and other component animations remain unported. The native
  lock/unlock curtain is now implemented but has not passed visual acceptance;
  see [curtain validation](curtain-validation-20261111.md).
- Pixel/coverage comparator regressions: 6 passed. Native Rust tests: 34 passed
  (5 timing, 8 session ordering/failure tests, 4 GPUI authentication state tests,
  5 workspace interaction tests, 3 focus/transition regressions, 1 PNG encoding test, 1 password-strength scoring test, 1 locale-alias regression,
  3 toast timing tests and 3 toast interaction tests).
  GPUI's test platform is not physical keyboard/mouse acceptance.
  14 normalized unlock-curve values matched actual WebKit CSS transforms.
  This validates curve mathematics. Native curtain rendering, timeline callbacks
  and limited live lock inspection now have separate evidence in the curtain report;
  none establishes complete application animation parity.
- `pnpm repo:check` passed (776 source files). `cargo fmt --all -- --check` passed.
  `cargo build -p aipass-desktop-native --features parity-render` passed.
- `cargo clippy --workspace --all-targets -- -D warnings` and the additional
  native crate Clippy check with `--features parity-render` both passed without
  warnings using macOS Homebrew Rust 1.99.1. The earlier GPUI 0.2.2 dependency
  warnings were resolved by the dependency update; no lint suppression or
  workspace exclusion was added.
- Full Rust/macOS formatting, workspace Clippy, tests and build passed after
  correcting JSON-order assumptions exposed by the unified GPUI dependencies.
  The preceding full Node/macOS install, license audit, lint, typecheck, tests and build passed;
  embedded control-panel output remained unchanged. Detailed logs are in
  `artifacts/desktop-parity/logo-reset-validation-20261112/`.
  The later Svelte corrections use the proportionate desktop checks recorded
  in the populated business review; that earlier full Node gate is historical.
  The desktop artifact runtime gate and remote CI have not run for the migration.
  No commit, push or release has been performed.

## Reproduce the development checks

Apply the shell wrapper required by the repository instructions.

```sh
pnpm install --frozen-lockfile
pnpm --filter @aipass/schemas... build
npm ci --prefix tests/desktop-parity --ignore-scripts
node tests/desktop-parity/setup.mjs
node tests/desktop-parity/node_modules/playwright/cli.js install webkit
node node_modules/vite/bin/vite.js --config tests/desktop-parity/vite.config.mjs
# In another terminal, while the reference server is running:
node tests/desktop-parity/capture-reference.mjs
cargo build -p aipass-desktop-native --features parity-render
# Every round name must be new; an existing round is never overwritten.
node tests/desktop-parity/capture-native.mjs unique-round-name
# Expanded component/style matrix; successful capture still needs review:
node tests/desktop-parity/capture-presentation.mjs unique-presentation-round-name
# Populated provider detail, add/edit and advanced forms:
node tests/desktop-parity/capture-providers.mjs unique-provider-round-name
node tests/desktop-parity/compare-motion.mjs
node tests/desktop-parity/capture-motion.mjs unique-curtain-round-name
node --test tests/desktop-parity/compare.test.mjs
cargo test -p aipass-desktop-native --features parity-render
cargo run -p aipass-desktop-native -- --parity-preview
# Native Tauri fixture host; requires the reference Vite server above:
cargo run -p aipass-desktop --example parity_reference
# Expected to fail until every native surface has evidence:
node tests/desktop-parity/audit.mjs
```

The setup script creates the ignored reference dependency link and is safe to rerun. Later
captures use timestamped directories and never replace accepted reference images.
The preview also accepts `--dark`, `--zh-CN` and `--large`; its bundle identifier
is `com.alkinum.aipass.gpui-parity`. By default it uses synthetic empty state
and does not open or alter the user's vault. Capture and live motion-trace modes reject `--vault`. Fixed animation samples
use `--scenario=unlock-motion` or `--scenario=lock-motion` with `--motion-at=MS`.
`--reduced-motion` exercises the native reduced-motion setting.
`--motion-trace=/new/output.json` records one synthetic live transition; these
are render-loop observations, not compositor presentation timestamps.
Do not use the development Agent connection as the production desktop: the
remaining lifecycle, data-display and recovery workflows are not complete.
