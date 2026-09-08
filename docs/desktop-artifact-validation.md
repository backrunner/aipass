# Desktop artifact validation

Every macOS branch bundle and release must pass runtime validation before its
artifacts can be uploaded for publication. Compile, unit tests, signatures and
notarization remain required; they cannot establish that the app starts or
finishes installing an update.

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
