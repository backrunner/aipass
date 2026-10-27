---
title: Updates
description: Official, beta, and nightly channels, signed updates, and restart behavior.
navTitle: Updates
order: 8
---

# Updates

AIPass distributes releases through GitHub Releases. **Settings → Updates** shows your version, selected channel, and **Check now** action. An available update offers installation beside its version status.

## Channels

| Channel  | Tag                         | Update feed                                                                 |
| -------- | --------------------------- | --------------------------------------------------------------------------- |
| Official | `vX.Y.Z`                    | `https://github.com/backrunner/aipass/releases/latest/download/latest.json` |
| Beta     | `vX.Y.Z-beta.N`             | `https://aipass.alkinum.io/api/updates/beta/latest.json`                    |
| Nightly  | `vX.Y.Z-nightly.YYYYMMDD.N` | `https://aipass.alkinum.io/api/updates/nightly/latest.json`                 |

The final nightly daily revision is optional; `vX.Y.Z-nightly.YYYYMMDD` is also accepted. Beta and nightly feeds select published prereleases with the matching tag suffix and do not include drafts or releases from another channel. Each feed returns the release's signed update manifest.

Choose **Official** for stable releases, **Beta** for preview releases, or **Nightly** to follow dated development snapshots. A build's default channel follows its version suffix. The app stores a manual channel choice per machine; installing a build from a different channel can reset the choice to that build's channel.

## Installing an update

The app checks its selected feed in the background and can download an update before you install it. The installer verifies the package signature before applying it.

Use **Install & restart** when ready. A deferred downloaded package is cached and can be installed on a later launch when its selected feed is reachable and the cached package remains valid. Failed downloads or signature checks do not replace the running app; use **Check now** for a visible error and retry.

Automatic updates do not install an older or equal version. Within one base version, prereleases sort as beta, nightly, then stable. Switching feeds can therefore report that you are up to date when the other channel contains an older version.

To change builds manually, download the desired package from [GitHub Releases](https://github.com/backrunner/aipass/releases) and follow the [installation guide](/docs/installation). Read its release notes and vault compatibility requirements first.

## CLI updates

```bash
aipass self-update check --json
aipass self-update install --yes --json
aipass self-update check --channel nightly --target cli --json
aipass self-update install --channel beta --target desktop --yes --json
```

`--target auto` detects a standalone directory containing all three executables, or a desktop companion. Select `cli` or `desktop` to override detection. The default channel follows the CLI build; `--channel official|beta|nightly` selects a channel for this operation without changing desktop preferences. Existing `aipass update <id>` commands still edit credentials.

Checks never install, unlock the vault or prompt for a password. When needed, desktop operations start the installed AIPass companion in tray mode. `install` requires `--yes`, supports redirected stdin and restarts the affected runtime. An Agent restart ends its unlocked session. No newer release is a successful no-op; downgrades are refused.

Desktop updates use the existing signed updater. Success requires a new desktop process at the expected version, the refreshed CLI and a responsive Agent. The restarted desktop stays in tray mode. AppImage installations support unattended updates; DEB/RPM installations must use their system package manager to avoid authorization dialogs. Insufficient permissions fail without requesting elevation.

Standalone updates fetch metadata from the official GitHub repository, require the asset's SHA-256 digest, verify the downloaded bytes and executable architecture, then replace the CLI, Agent and Native Host together. Missing digests, unsupported architectures, malformed packages and version mismatches fail before replacement. Replacement failures restore the previous binaries; recovery files are retained if restoration cannot finish. Keep all three executables together in a writable directory. Other vault Agents using that installation should be stopped before updating.

`--timeout-seconds 300` bounds each network operation and the desktop restart wait (range 1–3600). In JSON mode stdout contains one JSON result. Exit `0` means the check or installation completed, including “up to date”; exit `1` means an operational failure; exit `2` means invalid arguments, including missing `--yes`. Use `available` and `latestVersion` in check results, and `status: "installed"` in successful installation results.

## Extension packages

Releases include `aipass-edge-extension.zip`. Browser manifests use a numeric build version while package metadata keeps the full prerelease version. Nightly packages are distributed as ZIP files rather than submitted as nightly store releases.

Use the extension setup and repair actions for your installed browser. Store-installed extension copies are not replaced by the desktop app's unpacked extension updates.

## Release verification

Publication requires signing, artifact validation, and startup/update checks. Maintainers also verify public downloads and the matching channel feed after publication. See [Development and releases](/docs/development#releases) for the release procedure and contributor credits.
