# Native desktop cutover — 2026-11-15

The production application is `apps/desktop-native` (`aipass-desktop` executable).
`apps/desktop`, including `src-tauri`, is removed from the workspace and tree.
The Cargo and pnpm dependency graphs no longer contain the retired framework,
its plugins, runtime, WebView, bundler or JavaScript APIs. Repository checks
reject their reintroduction through manifests or lockfiles.

## Retained responsibilities

- Agent owns vaults, subscriptions, proxy, sync and final tool configuration.
  GPUI uses typed, authenticated Agent requests through a shared runtime gate.
- `shell/` owns native tray adapters, CloudKit ciphertext transport, singleton
  activation, URL forwarding, geometry, logging and macOS self-installation.
- macOS retains the SwiftUI tray and CloudKit bridge, with its signed application
  identity, provisioning profile and production entitlements. Windows uses a
  native tray menu; Linux runs AppIndicator on its GTK event thread.
- `updates/` owns HTTPS feeds, bounded downloads, minisign verification, cache,
  schedules, UI/CLI exclusion and restart. macOS swaps application directories
  atomically and restores the prior app when readiness fails. Linux keeps
  AppImage replacement and polkit-owned Deb/RPM authorization. Windows stages a
  separately verified NSIS payload, waits for the old process in an independent
  native helper, then restarts the installed executable.
- `scripts/package-desktop.mjs` owns packaging. Platform modules create macOS
  app/DMG/updater archives, Deb/RPM/AppImage and Windows NSIS packages. Release
  signing retains the existing public trust root. `AIPASS_SIGNING_*` variables
  replace framework-specific command inputs; GitHub accepts the existing secret
  names as compatibility aliases so no credential rotation is required.
- The Svelte LAN panel, extension, shared locales and shared UI remain. The panel
  logo now comes from native assets, and its tracked embedded output is rebuilt.

## Validation evidence

Validation was recorded before committing the implementation. It is not a
release, a merge, or remote CI evidence. The user waived Keychain-dependent
regression and animation/performance sampling; prior unaffected UI evidence is
retained in the linked migration records.

The local native bundle uses a disposable updater key and ad hoc macOS code
signing. It does not prove production Developer ID notarization or CloudKit
account access. Linux/Windows runtime checks remain configured in GitHub Actions;
local Linux execution is explicitly prohibited by the repository policy.

The local gate uses Homebrew Rust 1.99.1 and Node 26.12.1 on macOS:

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed. |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed without warnings. |
| Workspace Rust regression | 1,443 passed, 4 pre-existing ignored; 12 Keychain skip filters were explicitly applied as authorized. |
| `cargo test -p aipass-desktop-native --lib --bin aipass-desktop` | 140 passed after the final restart/launch changes. |
| `cargo build --workspace` | Passed. |
| Native `parity-render` all-target Clippy | Passed. |
| Frozen pnpm install, license audit | Passed; 349 package licenses audited. |
| `pnpm lint`, `pnpm typecheck`, `pnpm test`, `pnpm build` | Passed. Unchanged task inputs reuse their recorded/Turbo results. |
| `pnpm repo:check` | Passed; 841 source files, 13 documented no-growth exceptions. |
| Vendored source provenance | All 6 pinned source inventories passed. |
| Migration dependency/manifest checks | Passed, including direct and renamed Cargo dependencies. |
| Frozen browser reference build | Passed without installing the retired framework. Retained historical native captures remain the visual evidence. |
| Shared control panel rebuild | Byte-for-byte identical generated files on the final rebuild. |
| Repository skill validators and workflow YAML parsing | Passed for all edited skills and both workflows. |

The native package builds and signs its executable, Agent, CLI and Native Host,
then generates a signed updater archive and DMG. The finished-artifact runtime
suite checks main-window and tray startup, automatic signed download, rejection
of tampered bytes, manual install/restart, authenticated CLI install/restart and
cached install/restart. Each scenario verifies 960×640, the GPUI render/event
loop, typed Agent IPC, isolated proxy configuration/history, and replacement PID
where applicable. This is startup/update evidence, not an animation or physical
presentation-timing measurement.

Windows/Linux code and packaging are migrated, but their execution remains for
configured remote CI; no local Linux environment was created. Wayland uses
compositor minimization where GPUI does not expose a window-unmap operation;
X11 and Windows use native hide/show. Wayland tray-only presentation is not
claimed as accepted by these macOS results.

The previous failing cached-restart report is retained under
`artifacts/native-cutover-runtime-20261115`. The fixed run passed all 7 cases
under `artifacts/native-cutover-runtime-final-20261115`. Final delivery artifact
hashes and the rebuilt-artifact rerun are recorded below.


## Final artifact results

`node scripts/verify-macos-dmg.mjs target/release/bundle/dmg` passed for the
final image: checksum, read-only mount, metadata and executable payloads.
The image check ran sequentially after runtime validation to avoid concurrent
mount contention.

All 7 scenarios passed again for the final rebuilt artifacts in
`artifacts/native-cutover-runtime-delivery-20261115` (`delivery.json`).

| Artifact | SHA-256 |
| --- | --- |
| `AIPass_0.2.0_arm64.dmg` | `60c7352b01ba40e06d22d25c3cf724b429a38dfbf26f84a659a6c6e837d4e76a` |
| `AIPass.app.tar.gz` | `438f87d1ddfd543f72da5ac7a273a59c7b80663704bc39e235c2937524310bbe` |

These are local arm64 validation artifacts, signed with a disposable updater
key and ad hoc application signing. They are not production release artifacts.
The validated implementation is now recorded in commit `572313bc8ee32e649a3c4c1c44484d8a2406d7f9`.
The documentation commit does not change runtime or build inputs. Existing
passing checks are retained; strict workspace Clippy was also rerun successfully
before committing. Delivery status and remote CI are recorded in PR #6.
