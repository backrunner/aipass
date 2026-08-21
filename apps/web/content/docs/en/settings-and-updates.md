---
title: Settings and updates
description: Release channels, auto-update behavior, and keeping AIPass current.
navTitle: Settings & updates
order: 4
---

# Settings and updates

## Release channels

AIPass ships on two channels, both distributed through GitHub Releases:

- **Official** — stable releases tagged `vX.Y.Z`, published from the release workflow as signed and notarized macOS builds.
- **Beta** — prereleases published on the same repository for early testing. Beta builds may contain unfinished features.

The [download section](/) on this site always prefers the newest **official** release that has a macOS package. It only falls back to a beta build when no official release with a package exists yet, and labels the channel next to the version number.

You can also switch channels inside the app: **Settings → Updates → Update channel** offers **Official** and **Beta**, and switching re-checks for updates immediately on the new channel. The default channel follows the installed build — a beta build starts on the beta channel, a stable build on official. To move channels without waiting for an update, install the desired build over the current app from the download section or [GitHub Releases](https://github.com/backrunner/aipass/releases).

## Auto-update

The desktop app checks for updates automatically — shortly after launch, and at most once every 24 hours — by reading the update manifest published alongside each GitHub Release. When a newer build is available, an in-app prompt lets you:

- **Install** — download and apply the update in place.
- **Dismiss** — skip that version; AIPass will not prompt again for the same version.

Update artifacts are signed, and the app verifies the signature before installing.

## Vault and sync settings

From the desktop app you can also:

- Change the master password or rotate the vault epoch (`vault rotate`, `vault change-password` from the CLI).
- Manage trusted devices and revoke access for a device you no longer use.
- Export an encrypted vault backup (`vault export`) and import it on another machine.
- Configure sync over a local or iCloud folder, or a WebDAV endpoint. Only encrypted objects are ever synced.

See the [README](https://github.com/backrunner/aipass) for the full CLI surface.
