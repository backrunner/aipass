# Native onboarding and Agent management checkpoint, 2026-11-14

This checkpoint fills previously absent onboarding and Agent management UI.
It does not accept the complete desktop or remove Tauri. Global settings,
local-proxy pages and integrated native input acceptance remain open.
Foreground smoothness sampling remains waived, not passed. Production lifecycle,
Keychain-dependent functional checks and release artifacts remain deferred under
the user's UI-first instruction.

## Implementation

- Restore vault import from encrypted backup, vault folder, local sync, iCloud
  and WebDAV. Conditional fields, password reveal controls, file/folder selection,
  source changes and Tab destinations use native controls. Submitted passwords
  are immediately removed from the editors and their undo state. Existing typed
  Agent import requests remain the only vault authority.
- Restore cloud discovery, empty/failed feedback, retry and conditional local-only
  creation. A sync-settings read failure no longer discards a successful status
  read or leaves the root permanently loading.
- Extract recovery-kit presentation into its own entity. Match the modal, key
  typography and shared acknowledgment checkbox; add confirm-before-copy,
  generation/cancellation checks, clipboard TTL and copied feedback. The active
  reference supplies no download callback, so no download action is invented.
- Add initial status/sync loading with the shared background, brand and spinner.
  Failed status requests have an error and retry action.
- Route Agent navigation to an installed-tool list and detail pane. Scan and
  reconcile tool selections; show compatible API/native subscription credentials,
  current account, quota, subscription usage and seven-day local usage. Include
  empty/loading/error/unavailable/conflict states, restore, environment overrides
  and Cursor runtime eligibility. Preserve selections during theme changes.
- Reuse the existing configuration diff preview with the managed-switch label;
  dispatch `ToolConfigManagedApply`, not the provider quick-integration write.
  Installed manual-only tools open a configuration guide. Copying an API key
  uses the existing scoped credential service and clipboard permission lifecycle;
  plaintext never enters the guide view.
- Retire stale previews and reads when the catalog changes; clear all management
  state on lock. Refresh on window activation without invalidating an active
  manual-setup/clipboard confirmation. Add correct manual-tool icons, pending
  switch feedback, contextual accessibility labels and zero-count trash behavior.
- Keep onboarding, recovery, managed-tool choice projection, transport, page,
  manual guide and fixtures in separate Rust modules. All new owned production
  files satisfy the 1,000-line limit; no third-party provider implementation was
  imported. The `time` local-offset feature supplies the usage query's local-day
  boundary and adds `num_threads` to the lockfile.

## Comparison evidence

The reference remains the frozen Svelte baseline recorded in `inventory.json`.
Capture reports retain source hashes, binary hashes, paired images, measured
bounds, missing-command traces and errors. Geometry tolerance remains 0.5 logical
pixels. Full matrices cover 960×640 and 1280×820, English and Simplified Chinese,
light and dark, at 2× scale. Static captures use reduced motion; they are not
foreground timing or physical-input evidence.

| Artifact directory | Scope and result |
| --- | --- |
| `onboarding-matrix-20261114` | 11 states × 8 variants: 88 cases, 272 geometry checks pass; no capture errors or unsupported commands. |
| `onboarding-final-20261114` | 11 minimum-size English/light replacement probes after import-reset and shared-picker changes: 34 checks pass. |
| `agents-matrix2-20261114` | 11 states × 8 variants: 88 cases, 680 geometry checks pass; no capture errors or unsupported commands. |
| `agents-controls-20261114` | 16 credential-menu/manual-guide cases, including trigger and popup bounds: 168 checks pass after correcting the manual guide's generic monospace font and selected-credential fixture. |
| `agents-pending-20261114` | 16 ready/pending cases after adding the switch spinner and contextual picker label; all mapped geometry checks pass. |

Earlier failed probes are retained. They exposed a duplicate pane border,
incorrect toolbar button size, accumulated text-line metrics, manual-guide line
height, missing tool icons and an error retry button stretched across the card.
The manual guide uses Menlo for the reference's generic `monospace`, while the
configuration preview and recovery key retain their distinct system monospace
stacks. Finite reference transitions settle before capture; only infinite
background motion is frozen. This prevents capturing the sidebar's previous
active colors as if they were its settled style.

The first onboarding matrix predates the final import-reset interaction change;
its geometry is reused only for unchanged layouts. The final minimum-size probe
and focused auth regression cover the reset/shared-picker changes. Agent's full
matrix predates the manual-guide font/fixture correction and pending feedback;
the control and pending matrices replace those affected observations. No passing
result is promoted to whole-image, complete animation or native-input acceptance.
The manual guide follows the declared 4px backdrop blur; the headless WebKit
reference does not paint that blur in its screenshots, so identical backdrop
rasterization is not claimed.

## Local validation

Homebrew Rust on macOS:

- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- `cargo clippy -p aipass-desktop-native --features parity-render --all-targets -- -D warnings`: pass.
- Native library/UI regression: 94 library and 145 UI tests pass. This includes
  import validation and secret retirement, metadata-failure loading recovery,
  preview invalidation, managed credential modes, selection reconciliation,
  lock retirement and manual-guide lifecycle. Final styling changes are checked
  by the targeted captures and focused Agent tests.
- The disposable `provider_agent_roundtrip` integration passes. Its own output
  explicitly limits apply/rollback evidence: the disposable environment has no
  usable Keychain; it verifies the real IPC conflict/fail-closed path, while
  successful apply/rollback remains covered by existing Agent handler tests.
- `pnpm --filter @aipass/desktop typecheck`: zero errors and warnings reported.
- `pnpm repo:check`: 946 owned source files checked, 19 pre-existing oversized
  files remain tracked; no new source-size exception.
- Formatting and whitespace checks pass. No commit, push, remote CI, production
  bundle or workspace functional acceptance is claimed by this checkpoint.

## Native interaction follow-up

The [native acceptance checkpoint](native-acceptance-20261114.md) records actual
Agent picker/manual-guide interaction, recovery confirmation and two corrected
accessibility labels. Other presented-input checks remain open because the test
windows reported Hidden; the complete desktop is not accepted.

## Still required before Tauri cleanup

1. Global settings drawer and all general/security/sync/server/backup/about
   controls, including sync status, control-panel and model-pricing settings.
2. Local-proxy management/detail, channels, logs, search settings, usage chart,
   breakdown and range controls.
3. Integrated native focus/Tab, popup/disclosure, resize and interactive state
   acceptance across the migrated surfaces. The matrices above provide bounded
   layout evidence and do not replace that acceptance.

Shared Svelte/control-panel resources, Agent services and pinned licensed
renderer sources remain intact. No Tauri source was removed.
