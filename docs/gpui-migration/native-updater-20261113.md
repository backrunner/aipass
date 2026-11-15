# Native updater migration checkpoint

The GPUI comparison app now has its own Rust updater; it does not load Tauri.
Production enablement is still pending. The executable remains a migration
preview, and the update fixture refuses `--vault`, capture and motion-trace modes.
No production app, user vault, public feed or release key was changed.

## Implemented behavior

- First-frame startup, a three-second initial check and a persisted daily cadence.
  Failed checks do not persist success or create a zero-delay retry loop.
- Version-family feed selection and strict newer-version checks. Native target
  names are `darwin-native-aarch64` and `darwin-native-x86_64`; existing Tauri
  targets are rejected, preventing an accidental runtime replacement.
- Bounded downloads, HTTPS and redirect validation, mandatory Minisign checking,
  atomic cache writes and exact feed/target/release binding. Cached bytes are
  verified again immediately before extraction.
- Background download, localized install/later controls, persisted dismissal,
  lock-time hiding and cached installation on the next launch. A stale or corrupt
  startup cache cannot authorize silent installation of a newly downloaded release.
- Cross-process operation locking, same-volume staging, bounded archive extraction,
  bundle identity/version/executable and code-signature checks, then atomic macOS
  directory exchange. Symlinks and extra application roots are rejected.
- Keep the old application until the replacement reports a rendered GPUI frame
  and typed Agent readiness when running a managed fixture.
  Failed startup restores the old application; failed rollback retains its backup.
  Removing the update cache must succeed before bundle replacement.
- Drain in-flight Agent operations behind a runtime exclusion gate and recheck
  authorization before replacement. Confirm proxy disconnection, stop the owned
  Agent through authenticated IPC, and reject stale confirmation after lock.
- Hold an inherited process lease across the old/new desktop handoff. A second
  instance cannot enter the profile during replacement or after the old process
  exits. Failed replacement retires a candidate Agent before restoring the old
  sidecar; path checks prevent using another installation's binaries.

## Evidence and limits

Evidence lives in `artifacts/desktop-parity/native-updater-20261113/`.
The Python fixture builder generates a disposable Minisign key, destroys its
private key after signing, and serves only a loopback test feed. Both versions
use the same compiled executable with different signed bundle version metadata.
This exercises replacement and process restart, not a production code-version
upgrade or Apple notarization.

Native computer use recovered during this session. English/light and Chinese/dark
update controls were inspected at 960×640. Native interactions exercised install,
new-window restart, lock-time hiding, failure retry and dismissal. Reports record
bundle/binary/archive/icon hashes, cache removal and observed process identities.
The signature rejection fixture keeps the original app interactive. The signed
early-exit fixture rolls back and leaves the installer alive and retryable.

Unit regressions cover signature/cache corruption, partial cache, cleanup failure,
URL and target restrictions, channel inference, equal/older versions, time reversal,
bounded reads, archive links/multiple roots and atomic exchange/rollback.

### Managed runtime follow-up

`native-runtime-20261113-final/after-manual.json` records the real-window manual
handoff: desktop `78656` → `91099`, Agent `77711` → `91114`, bundle version
`0.2.1`, closed proxy listener and removed package/metadata cache. Native
confirmation, restart and subsequent provider interaction were observed during
that earlier control session. The competitor process exited with code 2.

`after-cached.json` records cached installation with no initial Agent running:
the new bundle reached `0.2.1`, cleared its signed cache, established a new Agent,
and accepted typed CLI unlock/list against the same disposable profile. A
competitor was again rejected. The test launcher stays alive across parent
replacement so the execution tool does not clean up the newly spawned process
tree when the old desktop exits.

`native-runtime-20261113-broken/after-rollback.json` records a signed replacement
that exits before readiness. After explicit proxy shutdown through the typed
fixture CLI, cached installation restored version `0.2.0` and the exact original
binary hash/signature. Agent `78487` exited (unreaped by the fixture builder until
cleanup); restored Agent `27681` accepted unlock/list. The original desktop
remained alive, returned to an idle error state, and retained its instance lease.
The proxy listener remained closed. This follow-up also has no new GUI claim.

The later native-window control session returned `cgWindowNotFound`, including
for the previously working updater fixture. Cached follow-up is process, bundle,
listener and authenticated IPC evidence; it does not add a new GUI acceptance
claim. See the [UI review](ui-review-20261113.md) for that separate limitation.

Full Rust/macOS and Node/macOS checks and the existing Tauri DMG/runtime gate are
recorded separately. The Tauri gate does not accept GPUI production lifecycle.
Remote Linux/Windows CI and release publication have not been run for this diff.

## Remaining production cutover

Promote the isolated singleton, Agent exclusion, shutdown and proxy confirmation
paths into the production launcher. Validate the owned supervisor/tray
suspension/restoration adapters with production registration, and connect owned
clipboard cleanup and login restart mode. Integrate settings/manual
checks and production package metadata, compile in the production trust root,
publish native-target manifests, and validate signed/notarized production bundles.
Linux and Windows native installers are not implemented by this checkpoint.
Crash/power-loss recovery beyond the atomic bundle exchange and failed-launch
rollback needs its own production acceptance. Do not enable the fixture for a
real profile or use its success to remove the Tauri release gate.

## Reproduce

```sh
python3 tests/desktop-parity/native-updater.py NEW_OUTPUT valid
# In another terminal, run the bundle path/config printed by the fixture:
NEW_OUTPUT/installed/AIPass\ GPUI\ Update.app/Contents/MacOS/aipass-desktop-native \
  --parity-preview --update-config=/absolute/NEW_OUTPUT/config.json
```

Use `tampered` to append bytes after signing, or `broken` to sign a replacement
that exits before readiness. Use a new output directory every time. The fixture
builds the `parity-render` binary explicitly, so an intervening default workspace
build cannot accidentally supply an executable without the validation entry point.
Keep the feed running during manual/cached installation because the release is
rechecked. Finish by closing the fixture applications and their loopback servers.

Append `managed` to prepare an isolated vault, typed Agent and proxy fixture. Use
the exact environment and arguments from its `launch.json`; keep the launcher
alive through process replacement. Never point this fixture at a real profile.
