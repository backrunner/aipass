---
title: Updates
description: Auto-update behavior, official vs beta release channels, and how to switch feeds.
navTitle: Updates
order: 8
---

# Updates

AIPass ships on two channels, both distributed through GitHub Releases and consumed with the Tauri updater:

- **Official** — stable releases tagged `vX.Y.Z`. Update feed: `https://github.com/backrunner/aipass/releases/latest/download/latest.json`, which always points at the newest stable release.
- **Beta** — rolling prereleases for early testing, published as a `beta` pre-release on the same repository. Update feed: `https://github.com/backrunner/aipass/releases/download/beta/latest.json`. Beta builds may contain unfinished features.

Each feed is an update manifest published alongside its GitHub Release; the artifacts are signed, and the app verifies the signature before installing anything.

## Which channel am I on?

The default channel follows the installed build: a version number containing a dash (for example `0.9.0-beta.3`) starts on the **beta** channel, and a plain `X.Y.Z` version starts on **official**. Your choice is stored per machine once you change it, so it survives app updates.

## The Updates panel

**Settings → Updates** shows the current version, the selected channel, and a manual **Check now** button. When an update is available you can install it in place; the signature is verified before anything is applied.

## Switching channels

In the desktop app: **Settings → Updates → Update channel** offers **Official** and **Beta**. Switching persists the choice and immediately re-checks for updates on the new channel — if the other channel has a newer build, the install button appears right away.

You can also move channels by installing the desired build over the current app from the [download section](/) or [GitHub Releases](https://github.com/backrunner/aipass/releases). The vault is untouched either way.

## How auto-update behaves

- The app checks shortly after launch (a 3-second delay) and at most once every 24 hours, reading the manifest of your current channel.
- When a newer build exists, an in-app banner offers **Install & restart** — which downloads, verifies, and applies the update in place — or **Later**, which dismisses that version; AIPass will not prompt again for the same version. Manual checks from **Settings → Updates** always run, and surface errors that background checks stay silent about.
- Background check failures (offline, rate-limited) are silent; nothing breaks if a check never completes.

## The download page and channels

The [download section](/) on this site always prefers the newest **official** release that has a macOS package. It only falls back to a beta build when no official release with a package exists yet, and labels the channel next to the version number. Windows builds are marked **Coming soon**.

## For release maintainers

Both feeds are just GitHub Releases assets named `latest.json` — the official feed rides GitHub's `latest` alias, the beta feed lives on the `beta` tag. Publishing a new release (or updating the `beta` pre-release) updates the corresponding feed for every installed app on that channel; there is no separate update server to run.
