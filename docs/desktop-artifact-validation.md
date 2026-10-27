# Desktop artifact validation

Every macOS branch bundle and release must pass runtime validation before its
artifacts can be uploaded for publication. Compile, unit tests, signatures and
notarization remain required; they cannot establish that the app starts or
finishes installing an update.

Routine source commits and pushes use the proportionate local checks in
[`agents.md`](../agents.md#proportionate-local-validation). A simple UI or
documentation fix does not require producing a macOS bundle. Changes requiring
the complete local gate and artifacts intended for publication still require
the finished-DMG and updater checks described here.

## DMG image integrity

Per the 2026-10-27 user instruction, `scripts/verify-macos-dmg.mjs` runs only
non-GUI checks on the finished image: `hdiutil verify`, a read-only mount,
valid app metadata, and readable, nonempty executable payloads. It does not
open Finder or inspect window geometry, background art, icon positions or
Retina representations. The existing packaging appearance is still generated.

Run `node --test scripts/verify-macos-dmg.test.mjs` on macOS to exercise a
valid image without layout assets, a truncated image and a missing helper.
Production signing/notarization checks and the application runtime gate below
remain required.

## Application runtime

`scripts/verify-macos-runtime.mjs` validates the finished DMG and updater archive:

- Copy the DMG's application into a disposable directory, open the main window
  at 960×640, wait for frontend completion and check WebView JavaScript,
  native event-loop and authenticated Agent responsiveness for ten seconds.
- Open the updater archive's application in tray mode and repeat the observation.
- Download the supplied archive from a loopback fixture, verify its original
  updater signature, install it through `install_update`, and require a new
  process to finish startup with the main window visible.
- Preload the normal pending-update cache and start in tray mode. Require
  `install_pending_update` to install, clear its cache and restart successfully.

The update scenarios reinstall the same signed version into a disposable copy.
This tests the new installer's runtime with the actual archive, without requiring
an unpublished feed or changing release versions/signatures. Semver ordering and
channel policy retain their separate unit tests. An older published installer
that cannot complete an update may still need a manual DMG upgrade; a passing
new installer cannot repair code already running in an old release.

The application's explicit `--verify-runtime /tmp/aipass-runtime-*` option is
accepted only when the running executable is the fixture's own `AIPass.app`.
The fixture uses an ephemeral WebView, isolated preferences/cache/vault/socket/log
paths, local-only sync, and direct Agent startup. It does not register browser
extensions or login services. A real, separately isolated LaunchAgent regression
test covers supervisor suspension, stale kill-child flags and desktop-child
survival. Runtime lifecycle tests also require in-flight startup to finish before
installation and reject watchdog restarts until the update transaction ends.
Neither check substitutes for signed CloudKit account integration or
longer interactive testing of vault, proxy and sync operations.

Release validation keeps the artifact's production updater public key and
signature. Branch CI seals the complete test app with ad hoc code signing and
generates a disposable updater key; the key is deleted after the job. The verifier
never disables signature verification.

Run locally on macOS with Homebrew Rust, Node 26 and the repository's pnpm:

```sh
cargo test -p aipass-desktop --lib
cargo test -p aipass-agent autostart::
node scripts/verify-macos-runtime.mjs release-artifacts runtime-check-reports
```

For a local/branch build without Developer ID, generate a disposable signing setup:

```sh
runtime_signing_dir="$(mktemp -d)/signing"
trap 'rm -rf "$runtime_signing_dir"' EXIT
node scripts/prepare-runtime-check-build.mjs "$runtime_signing_dir"
TAURI_SIGNING_PRIVATE_KEY="$runtime_signing_dir/updater.key" \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD='' \
TAURI_BUNDLER_DMG_IGNORE_CI=true \
  pnpm --dir apps/desktop tauri build --ci --bundles app,dmg \
  --config "$runtime_signing_dir/tauri.runtime-check.json"
node scripts/verify-macos-dmg.mjs target/release/bundle/dmg
node scripts/verify-macos-runtime.mjs target/release/bundle runtime-check-reports
rm -rf "$runtime_signing_dir"
```

An `.app` path can be supplied for startup-only diagnostics. CI and release jobs
must supply an artifact directory so both update scenarios are mandatory.

The verifier fails on early process exit, startup errors, unresponsive IPC/event
loops, missing restart, unexpected versions, leftover update caches or altered
executables/signatures. `runtime-check-reports` contains per-scenario stdout,
stderr, component logs, a frontend snapshot on startup timeout when the WebView
still responds, matching macOS crash reports when available, results,
and input artifact SHA-256 hashes. Both workflows
upload these diagnostics even on failure. Runtime failures block publication;
do not use `continue-on-error` or accept a log marker as a substitute for a live,
responsive restarted process. Local validation runs only on macOS; GitHub Actions
is responsible for its configured Linux jobs.

## Linux artifacts

Linux release jobs build Deb, RPM and AppImage for aarch64 and x86_64. Verify all
three detached signatures against the configured updater public key with
`scripts/verify-linux-signatures.mjs`. The release job uses the production key;
pass the disposable build configuration as its second argument for local tests.

`scripts/verify-linux-runtime.mjs <deb-or-rpm-directory> <reports>` extracts the
finished package and validates main/tray startup, native event-loop/WebView/Agent
responsiveness, package version, CLI operations and persistent helper hashes.
`scripts/verify-linux-updater.mjs <AppImage> <version> <reports> [older-AppImage]`
requires signed manual/cached install-and-restart, clean process exit, cleared
cache and exact installed bytes. It also tests read-only installation/cache,
tampered-cache recovery, a complete cached package in a read-only cache,
bad signatures, a signed Deb supplied under an AppImage
target and a failed feed.
The matching signed Deb must be alongside the AppImage directory in `../deb`.
All permission checks run as an ordinary user. Failure must preserve the original
executable and the live Agent; never grant test writes permission merely to pass.

These gates use isolated HOME/vault/runtime data and keep signature verification
enabled. The container's extract-and-run transport does not establish FUSE or
physical GPU behavior. Successful privileged Deb/RPM installation and graphical
Polkit approval/cancellation require a full Linux login environment; cached distro
packages intentionally wait for explicit installation rather than prompting on
startup. See [Linux validation](linux-validation.md) for the task-specific local
desktop authorization and actual coverage. The default local policy remains
macOS validation with remote Linux jobs.
