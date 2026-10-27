---
title: Installation
description: Install the desktop app, standalone CLI, and Chromium extension.
navTitle: Installation
order: 3
---

# Installation

Download packages from [GitHub Releases](https://github.com/backrunner/aipass/releases). Each release lists its platform-specific assets and changes; choose an official, beta, or nightly build as described in [Updates](/docs/updates).

## Desktop app

### macOS

Use `AIPass-macOS-<version>.dmg`. The Universal app supports Apple silicon and Intel Macs. Open the DMG, copy AIPass to Applications, and launch it. Published macOS applications and DMGs are signed and notarized.

### Linux

Choose the x86_64 or aarch64 package matching your machine:

- `.deb` for Debian/Ubuntu-based distributions.
- `.rpm` for RPM-based distributions.
- `.AppImage` for a portable app; make it executable before launching it.

Use the package format and permissions appropriate for your distribution. A session without a systemd user manager can use the desktop-session autostart backend.

### Windows

Standalone Windows CLI archives are available. Windows desktop installers are exercised in CI; download only assets actually attached to the release you choose.

On first launch, create a vault and save the one-time recovery key offline. See [Quick start](/docs/quick-start).

## Standalone CLI

Extract `aipass-cli-macos.tar.gz`, `aipass-cli-linux.tar.gz`, or `aipass-cli-windows.zip`. Keep `aipass`, `aipass-agent`, and `aipass-native-host` together in the extracted directory (with `.exe` on Windows), and add that directory to your `PATH`.

```bash
aipass --help
aipass doctor
aipass agent status
```

The CLI starts its sibling Agent when needed. `doctor` checks the vault, Agent, native host, browser registration, and extension allowlist; add `--json` for machine-readable output.

Use `aipass agent install` for login startup, or `start`, `stop`, and `uninstall` to manage the Agent. Print shell completions with `aipass completions <shell>`.

## Browser extension

Download `aipass-edge-extension.zip`, extract it, enable developer mode at `edge://extensions` or `chrome://extensions`, and load the extracted folder. Use the installed extension's ID to register Native Messaging:

```bash
aipass native-host install --extension-id <extension-id>
aipass native-host install --browser edge --extension-id <extension-id>
```

Chrome, Chromium, Edge, and Brave are supported. The desktop app also provides extension status and repair actions. Keep the vault unlocked when filling credentials; see the [extension guide](/docs/extension).

## Sync and another device

Signed macOS desktop builds use CloudKit by default. Folder and WebDAV targets use the same encrypted snapshot engine. The CLI also exposes explicit folder, OneDrive, and legacy iCloud Drive paths; see the [CLI reference](/docs/cli#sync).

Sync and backups do not include device-local subscription login credentials. Sign in to those accounts separately on another device or after a restore. See [Security](/docs/security#sync).
