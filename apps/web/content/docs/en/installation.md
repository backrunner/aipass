---
title: Installation
description: Download and install the AIPass desktop app and the Chrome extension.
navTitle: Installation
order: 3
---

# Installation

## Desktop app (macOS)

Download the installer from the [download section](/) or directly from [GitHub Releases](https://github.com/backrunner/aipass/releases). Two builds are published:

- **Apple silicon** — assets with `aarch64` or `arm64` in the filename.
- **Intel** — assets with `x64` or `x86_64` in the filename.

Open the `.dmg` and drag AIPass into Applications. Release builds are signed with a Developer ID certificate and notarized by Apple, so Gatekeeper opens them without extra steps. If you install an unsigned local build instead, macOS will block the first launch — allow it under **System Settings → Privacy & Security**.

Windows support is in preparation and is marked **Coming soon** on the download page.

## Browser extension (Chrome)

1. Install the AIPass extension from the Chrome Web Store. If the store listing is not available yet, download the extension package from [GitHub Releases](https://github.com/backrunner/aipass/releases) and load it from `chrome://extensions` with developer mode enabled.
2. Connect the extension to the desktop app. The app registers a Chrome Native Messaging host; you can also do it from the CLI:

```bash
aipass native-host install --extension-id <chrome-extension-id>
```

The installer writes the Chrome manifest with the extension allowlist. From then on, the extension can look up provider consoles and request fill grants from the unlocked vault.

## CLI

The `aipass` CLI ships with the desktop app and the repository. Verify it with:

```bash
aipass --help
```

Key commands: `init`, `add`, `list`, `get`, `copy`, `secret`, `probe`, `env`, `exec`, `configure`, `rollback`, `sync`, `native-host`, and the `vault` family for rotation, recovery, devices, and encrypted export/import.
