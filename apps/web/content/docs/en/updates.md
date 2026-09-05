---
title: Updates
description: Auto-update behavior, official vs beta vs nightly release channels, and how to switch feeds.
navTitle: Updates
order: 8
---

# Updates

AIPass ships on three channels, all distributed through GitHub Releases and consumed with the Tauri updater:

- **Official** — stable releases tagged `vX.Y.Z`. Update feed: `https://github.com/backrunner/aipass/releases/latest/download/latest.json`, which always points at the newest stable release.
- **Beta** — rolling prereleases for early testing, tagged `vX.Y.Z-beta.N` and published as prereleases on the same repository. Update feed: `https://aipass.alkinum.io/api/updates/beta/latest.json`, which resolves to the update manifest of the newest published `-beta` prerelease. Beta builds may contain unfinished features.
- **Nightly** — dated snapshot builds off `main`, tagged `vX.Y.Z-nightly.YYYYMMDD` (for example `v0.3.0-nightly.20260905`) and also published as prereleases. Update feed: `https://aipass.alkinum.io/api/updates/nightly/latest.json`, which resolves to the newest published `-nightly` prerelease. Nightly builds are the least tested of the three channels.

Each feed is an update manifest published alongside its GitHub Release; the artifacts are signed, and the app verifies the signature before installing anything. The feeds are isolated by tag suffix, so a release on one channel never appears on another channel's feed.

## The browser extension on the nightly channel

Nightly builds ship the browser extension **only as a zip package** — nightly extensions are never submitted to the Edge Add-ons store. When the desktop app starts, it silently checks whether the extension is already installed in your browser's extension directory (Chrome, Edge, and other Chromium-based browsers are scanned). If the app bundles a newer build, the new files are extracted directly into the browser's extension directory as a new version — no manual reinstall needed — and the browser picks the update up on its next launch.

Inside the nightly zip the extension carries a monotonically increasing four-part numeric version (`<base>.<build number>`, for example `0.3.0.1045`), because browsers reject non-numeric manifest versions; the full nightly semver remains what the desktop app displays. Files are only ever released when the bundled version is strictly newer than the installed one, and store-installed copies are never modified.

## Which channel am I on?

The default channel follows the installed build: a version number containing `nightly` (for example `0.3.0-nightly.20260905`) starts on the **nightly** channel, any other dashed version (for example `0.9.0-beta.3`) starts on **beta**, and a plain `X.Y.Z` version starts on **official**. Your choice is stored per machine once you change it, so it survives app updates — but installing a build from a different channel resets the choice to that build's channel, so a nightly install always lands on the nightly feed.

## The Updates panel

**Settings → Updates** shows the current version, the selected channel, and a manual **Check now** button. When an update is available you can install it in place; the signature is verified before anything is applied.

## Switching channels

In the desktop app: **Settings → Updates → Update channel** offers **Official**, **Beta**, and **Nightly**. Switching persists the choice and immediately re-checks for updates on the new channel — if the other channel has a newer build, the install button appears right away.

AIPass never installs an older or equal version over the current one. Within one base version, semver orders prerelease identifiers lexically (beta < nightly < stable), and every candidate is checked against the running version before anything is applied — so switching from a newer channel build to an older one simply reports "up to date" instead of downgrading. This protects the vault, which has no downgrade migration path.

You can also move channels by installing the desired build over the current app from the [download section](/) or [GitHub Releases](https://github.com/backrunner/aipass/releases). The vault is untouched either way.

## How auto-update behaves

- The app checks shortly after launch (a 3-second delay) and at most once every 24 hours, reading the manifest of your current channel.
- When a newer build exists, an in-app banner offers **Install & restart** — which downloads, verifies, and applies the update in place — or **Later**, which dismisses that version; AIPass will not prompt again for the same version. Manual checks from **Settings → Updates** always run, and surface errors that background checks stay silent about.
- Background check failures (offline, rate-limited) are silent; nothing breaks if a check never completes.

## The download page and channels

The [download section](/) on this site always prefers the newest **official** release that has a macOS package. It only falls back to a beta build when no official release with a package exists yet, and labels the channel next to the version number. Windows builds are marked **Coming soon**.

## For release maintainers

All three channels are driven by update manifests named `latest.json` published as GitHub Releases assets — the official feed rides GitHub's `latest` alias, while the beta and nightly feeds are served by this site's Worker, which reads the newest published prerelease **matching the channel's tag suffix** (`-beta` / `-nightly.`) and returns its manifest, so the two prerelease feeds never leak into each other. Publishing a release updates the corresponding feed for every installed app on that channel; there is no separate update server to run and no versionless channel tag to maintain.

Nightly releases are tagged `v<BASE>-nightly.<YYYYMMDD>` (semver, so the existing `v*.*.*` tag trigger and release validation apply; the release workflow rejects nightly tags without the dated suffix). Always choose a `BASE` version **above the latest stable release** (for example `0.3.0` while stable is on `0.2.x`): semver ordering then guarantees nightly > stable for nightly users, official users never see nightly builds, and a nightly user switching back to official or beta is held in place by the "never install an older or equal version" check instead of being downgraded. The macOS bundle build number comes from the workflow run number, which increases monotonically across all channels.
