# Tauri removal audit, 2026-11-14

**Current removal gate (user instruction, 2026-11-14): complete UI acceptance.**

After all UI surfaces, states, controls and appearance are accepted, remove Tauri.
The user's subsequent instruction waives foreground smoothness sampling; it is
no longer a cleanup prerequisite and must not be recorded as a performance pass.
Functional regression and production behavior may be
reviewed afterward. The Keychain-dependent workspace test, production lifecycle
and updater functional checks below are deferred work, not prerequisites for
this UI-first cleanup. Do not remove resources or shared code still needed by
GPUI, the Agent or the LAN control panel, and keep the resulting tree buildable.
The mandatory workspace Clippy and structural repository checks still apply.

UI acceptance is not complete: local-proxy/settings pages and integrated native
interaction remain open. The [onboarding and Agent checkpoint](native-onboarding-agents-20261114.md) adds the previously missing authentication setup/recovery/loading branches and Agent management page with bounded comparison evidence.
The [native interaction checkpoint](native-acceptance-20261114.md) records partial
real-window acceptance and visibility diagnostics; it does not close the integrated
input or appearance gate. Its visible-window follow-up closes the subscription
proxy Tab/input gap and verifies cancellation, selectors and form scrolling;
system appearance round-trip presentation remains interrupted.
Keep Tauri for these UI gaps only.
The original production audit below is historical; it does not override this
new removal criterion.

The reviewed implementation was delivered as four signed commits ending at
`2003fc3aea36a1160894d309fdd780c37473282a` on `refactor/gpui-desktop`.
The remote branch SHA was checked after the push. This audit follows that delivery;
it does not mark the migration accepted or authorize a release.

## Implementation follow-up

The [clipboard, activity and trash checkpoint](native-actions-20261114.md) implements
three gaps identified below. The original audit is retained as a point-in-time
inventory. Its statements that native secure copy, permanent deletion and activity
dispatch are absent are superseded by that checkpoint; other production and UI
blockers remain open. The new checkpoint does not remove Tauri or accept cutover.

The subsequent [native pricing checkpoint](native-pricing-20261114.md) adds
credential price assignments, shared rule/version dialogs and public endpoint
copy. It supersedes their absence in this original inventory. Font-width,
real-window and remaining production/page gaps are still open.

The [UI consistency follow-up](ui-consistency-20261114.md) corrects fractional
text layout and pricing badge paint, restores edit-page runtime/integration
cards, and adds previously missing edit-page mappings. It records the remaining
edit/advanced/integration discrepancies explicitly; cutover is still blocked.

The [UI control checkpoint](ui-control-close-20261114.md) closes the measured
edit/integration geometry gaps, corrects stepper/resize/font/glass details, and
records the new UI-first removal criterion. Its current-source readiness review
incorporates the sampling waiver and confirms the remaining visible gaps. It also
corrects the historical recovery-export claim: the active reference requires
copy/copied feedback; its optional download callback is not supplied by the app.

The [onboarding and Agent checkpoint](native-onboarding-agents-20261114.md)
subsequently restores import/cloud/recovery/loading and Agent management, account
detail and manual guides. Its evidence does not close the remaining settings,
local-proxy or integrated-input acceptance requirements.

## Coverage boundary

The immutable [inventory](inventory.json) contains 98 reference surfaces. Its 45
Rust/Swift lifecycle paths still exactly match the current tracked Tauri lifecycle
sources, excluding the comparison example. The table below accounts for every
surface group. It records implementation scope, not 98 successful UI checks.
Existing comparison reports remain evidence for their specific cases only.

| Reference group | Surfaces | Original native coverage, superseded by the follow-ups above |
| --- | ---: | --- |
| App root | 1 | Preview startup and provider workspace exist; production startup and page routing are incomplete. |
| Authentication | 8 | Auth controls, transitions and recovery display exist. Vault import, cloud discovery UI and recovery export still need working flows and acceptance. |
| Integration | 9 | Provider tool detection, preview/apply/restore and login exist. Agent management/account detail and manual setup flows remain incomplete. |
| Pricing | 2 | Credential pricing assignment and pricing group/version dialogs have no native replacement. Credential billing fields do not replace these dialogs. |
| Providers | 31 | Lists, forms, credentials, subscription options and maintenance actions have partial parity evidence. Secure copy, permanent trash deletion and the remaining reference branches still need coverage. |
| Server | 7 | Native proxy management, logs, search settings, channels and usage views are missing. A provider's upstream proxy setting is a separate feature. |
| Settings | 6 | Global settings and their security, sync, control-panel, import and model-pricing sections are missing. |
| Desktop shared | 7 | Window chrome, notices, modal controls and isolated updater controls exist. Sync status and complete state/interaction coverage remain open. |
| Shared UI package | 27 | Native primitives cover portions of provider/auth controls. Proxy controls and complete control/state mapping remain open. The Svelte package must remain for the LAN control panel. |

The existing three-round acceptance contract remains in force. A component's
presence, an offscreen image or a passing Rust event test does not accept physical
input, animation presentation, every state or production lifecycle.

## Original production blockers (historical)

1. **Production entry point.** [main.rs](../../apps/desktop-native/src/main.rs)
   exits without `--parity-preview`. A normal native launch does not start the
   shipped desktop. Theme/language preview flags also do not replace persisted
   preferences, system appearance updates or settings UI.
2. **Missing page dispatch.** [desktop.rs](../../apps/desktop-native/src/desktop.rs)
   leaves `WorkspaceEvent::Settings` in the wildcard no-op branch.
   [workspace.rs](../../apps/desktop-native/src/workspace.rs) still renders the
   provider layout for Agent/server navigation, while
   [Catalog::entries](../../apps/desktop-native/src/catalog.rs) returns an empty
   list for both sections. The sidebar's presence does not implement those pages.
3. **Import and recovery.** `AuthEvent::Import` maps to `None` in `desktop.rs`.
   [Recovery presentation](../../apps/desktop-native/src/desktop/view.rs) displays
   and acknowledges a key, but does not implement the reference's complete export
   workflow. Subscription account import is implemented separately and does not
   satisfy vault import.
4. **Provider actions.** Native credentials can edit/reveal keys, but have no
   secure-copy action or pricing-dialog flow. `catalog::Action` supports trash and
   restore, but has no permanent deletion operation or corresponding confirmation.
   These are visible actions in the reference provider detail, beyond the recently
   completed Reset/Probe/Usage dialogs.
5. **Security and user activity.** Native code has no `SessionTouch` dispatch for
   user activity. The reference throttles activity reports in `App.svelte`.
   Preserve the Agent's existing inactivity and power-event authority: macOS
   sleep/screen-lock observers already live in
   [Agent power handling](../../crates/aipass-agent/src/power/macos.rs), and must
   not be duplicated in GPUI. Native settings, activity reporting, lock-time UI
   cleanup and real sleep/lock acceptance remain necessary. The owned secret
   clipboard service still resides in Tauri and must retain confirmation,
   expiry and ownership-aware cleanup across lock, quit and update.
6. **Platform services.** CloudKit ciphertext transport and entitlements, Swift
   tray, desktop activation/singleton requests, deep links, browser/native-host
   installation, CLI installation and self-install remain tied to the Tauri
   shell. Preserve Agent ownership and move platform adapters into real modules
   before removing their current entry points.
7. **Updater production wiring.** [update_validation.rs](../../apps/desktop-native/src/update_validation.rs)
   accepts only isolated fixtures. Its managed runtime passes `supervise: false`
   and no tray socket. The runtime service adapter contains supervisor support,
   but this is not production supervisor/tray acceptance. Production trust root,
   native-target feeds, package metadata, launch modes and native installer gates
   still need integration. Linux/Windows native replacement installers and
   crash/power-loss recovery remain open. See the
   [native updater checkpoint](native-updater-20261113.md).
8. **Build and resource dependencies.** Native
   [assets.rs](../../apps/desktop-native/src/assets.rs) and
   [logo.rs](../../apps/desktop-native/src/logo.rs) still embed files from
   `apps/desktop/public`. Native fixture packaging also reads the Tauri ICNS.
   Cargo membership, desktop package scripts, version stamping, signing,
   CloudKit packaging, all desktop CI bundles and release jobs still target
   Tauri. Agent desktop discovery still expects the shipped executable names.
   Deleting the old directory now would break builds and runtime integration.
9. **Real-window acceptance.** The latest provider dialog/menu inspection still
   has inconsistent accessibility/input/screenshot observations. The new popup
   presentation sample has not passed. Historical auth frame traces do not
   certify these newer controls. Recheck the entire integrated app at 960×640
   and 1280×820, both locales/themes, with the required states and motion settings.

## Delivery validation

Local evidence: `artifacts/desktop-parity/push-gate-20261114/` (ignored local
artifacts; not uploaded by the Git push). `source.json` records the exact commit
and workflow hashes; command logs and per-job JSON retain exit codes.

- **rust (macOS): passed.** Homebrew Rust 1.99.1; `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`
  and `cargo build --workspace`. The existing four opt-in ignored tests remain
  ignored; no new exclusion or suppression was added. Native `parity-render`
  tests and its additional strict Clippy check also passed. The isolated proxy
  uninstall regression passed. Native-host's 24 tests completed this time.
- **node (macOS): passed.** Frozen install, license audit, lint/repository size
  checks, typecheck, tests and build; embedded control-panel output unchanged.
  The desktop suite passed 456 tests. Existing build chunk-size and test-only
  Svelte warnings remain visible in logs. Fourteen parity-audit/provenance tests,
  the edited skill validator and owned-document link/diff checks also passed.
- **macOS desktop bundle: passed for the current Tauri implementation.** Fresh
  disposable updater signing key, ad hoc App signature, required helper payloads,
  non-GUI DMG checks and seven runtime scenarios: main/tray startup, valid and
  tampered automatic download, manual/CLI/cached install-and-restart. Signatures
  stayed enabled. These results do not accept GPUI production lifecycle, Apple
  notarization, a code-version upgrade or a published release.
- **Remote CI:** the connector returned no statuses or PR-triggered workflow
  runs for the pushed SHA. CI is configured for PRs and pushes to `main`; no
  Linux/Windows result is claimed and no local Linux environment was created.

Artifact SHA-256 values:

| Artifact | SHA-256 |
| --- | --- |
| `AIPass_0.2.0_aarch64.dmg` | `06bd02a6eb23061207b152c4c109b5f913c1f04081092504c622876aaccfd453` |
| `AIPass.app.tar.gz` | `d64c36a698cdea645ddab525c9c3db74e5a7cce51adc0efadce440e78b24850c` |
| `AIPass.app.tar.gz.sig` | `ac3bd6f8e066e693d060ddcdb2318f33611162dca76c5408e8c12e0ab60577d1` |

## Removal sequence

Complete the missing native UI surfaces and their visual/input/motion acceptance
first. Then preserve required platform adapters and assets, remove the Tauri
shell and its exclusive dependencies, and update build/package references so the
remaining native tree builds. Keep shared Svelte controls/locales for the control
panel, required Swift adapters and third-party notices. Refresh lockfiles and
run the structural/source checks and mandatory workspace Clippy on the cleanup.
Functional and production-artifact acceptance remains deferred until the later
functional/release phase; UI acceptance alone does not certify a release.

No Tauri source, dependency or platform service was removed in this checkpoint.

Later native edit work and its still-open acceptance items are recorded in
[native provider editing follow-up](native-edit-20261114.md). Its local checks
do not complete the remaining UI gate. Functional/Keychain/production acceptance
is deferred under the instruction above.
