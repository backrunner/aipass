---
title: Updates
description: Official, beta, and nightly channels, signed updates, and restart behavior.
navTitle: Updates
order: 8
---

# Updates

AIPass distributes releases through GitHub Releases. **Settings → Updates** shows your version, selected channel, and **Check now** action. An available update offers installation beside its version status.

## Channels

| Channel | Tag | Update feed |
| --- | --- | --- |
| Official | `vX.Y.Z` | `https://github.com/backrunner/aipass/releases/latest/download/latest.json` |
| Beta | `vX.Y.Z-beta.N` | `https://aipass.alkinum.io/api/updates/beta/latest.json` |
| Nightly | `vX.Y.Z-nightly.YYYYMMDD.N` | `https://aipass.alkinum.io/api/updates/nightly/latest.json` |

The final nightly daily revision is optional; `vX.Y.Z-nightly.YYYYMMDD` is also accepted. Beta and nightly feeds select published prereleases with the matching tag suffix and do not include drafts or releases from another channel. Each feed returns the release's signed update manifest.

Choose **Official** for stable releases, **Beta** for preview releases, or **Nightly** to follow dated development snapshots. A build's default channel follows its version suffix. The app stores a manual channel choice per machine; installing a build from a different channel can reset the choice to that build's channel.

## Installing an update

The app checks its selected feed in the background and can download an update before you install it. The installer verifies the package signature before applying it.

Use **Install & restart** when ready. A deferred downloaded package is cached and can be installed on a later launch when its selected feed is reachable and the cached package remains valid. Failed downloads or signature checks do not replace the running app; use **Check now** for a visible error and retry.

Automatic updates do not install an older or equal version. Within one base version, prereleases sort as beta, nightly, then stable. Switching feeds can therefore report that you are up to date when the other channel contains an older version.

To change builds manually, download the desired package from [GitHub Releases](https://github.com/backrunner/aipass/releases) and follow the [installation guide](/docs/installation). Read its release notes and vault compatibility requirements first.

## Extension packages

Releases include `aipass-edge-extension.zip`. Browser manifests use a numeric build version while package metadata keeps the full prerelease version. Nightly packages are distributed as ZIP files rather than submitted as nightly store releases.

Use the extension setup and repair actions for your installed browser. Store-installed extension copies are not replaced by the desktop app's unpacked extension updates.

## Release verification

Publication requires signing, artifact validation, and startup/update checks. Maintainers also verify public downloads and the matching channel feed after publication. See [Development and releases](/docs/development#releases) for the release procedure and contributor credits.
