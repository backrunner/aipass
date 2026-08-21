---
title: Installation
description: Download and install the AIPass desktop app, the CLI, and the browser extension.
navTitle: Installation
order: 3
---

# Installation

## Desktop app (macOS)

Download the installer from the [download section](/) or directly from [GitHub Releases](https://github.com/backrunner/aipass/releases). A single **universal build** (`universal` in the filename) runs on both Apple silicon and Intel Macs — the download page detects your OS and offers it as one button.

Open the `.dmg` and drag AIPass into Applications. Release builds are signed with a Developer ID certificate and notarized by Apple, so Gatekeeper opens them without extra steps. If you install an unsigned local build instead, macOS will block the first launch — allow it under **System Settings → Privacy & Security**.

Windows support is in preparation and is marked **Coming soon** on the download page.

On first launch the app asks for a master password, creates the vault, and shows the recovery key once. The app also registers the background agent so the vault session survives app restarts.

## CLI

The `aipass` CLI ships with the desktop app and the repository. Verify it with:

```bash
aipass --help
aipass doctor
```

`aipass doctor` checks the vault manifest, agent reachability, the native host binary, installed browser manifests, and the extension allowlist — run it first when anything misbehaves. Add `--json` for a machine-readable report.

Print shell completions with `aipass completions <shell>` (bash, zsh, fish, and more).

The CLI talks to the same background agent as the desktop app. Agent lifecycle commands:

```bash
aipass agent install    # register the agent to start at login (LaunchAgent on macOS)
aipass agent status     # registered/running state plus lock status
aipass agent start
aipass agent stop
aipass agent uninstall
```

You normally do not need these — the CLI starts the agent on demand — but `agent install` keeps the session warm across logins.

## Browser extension (Chrome)

1. Install the AIPass extension from the Chrome Web Store. If the store listing is not available yet, download the extension package from [GitHub Releases](https://github.com/backrunner/aipass/releases) and load it from `chrome://extensions` with developer mode enabled.
2. Connect the extension to the desktop app. The app registers a Chrome Native Messaging host; you can also do it from the CLI:

```bash
aipass native-host install --extension-id <chrome-extension-id>
```

The installer writes the native messaging manifest `dev.aipass.native.json` with the extension allowlist — on macOS under `~/Library/Application Support/Google/Chrome/NativeMessagingHosts/` — and records the allowed extension ids for the native host. Chromium, Edge, and Brave are supported via `--browser`:

```bash
aipass native-host install --browser brave --extension-id <id1>,<id2>
```

Pass `--extension-id` multiple times or comma-separated to allow several extension builds (for example a store build and a dev build). The desktop app also shows native host status and can repair the manifest from its extension settings.

From then on, the extension can look up provider consoles and request fill grants from the unlocked vault. The full flow is covered in the [extension guide](/docs/extension).

## Sync (optional)

To replicate the vault across machines, configure one sync target:

```bash
aipass sync --dir ~/Sync/AIPass                 # any local folder
aipass sync --icloud                            # iCloud Drive (macOS)
aipass sync --onedrive                          # OneDrive folder
aipass sync --webdav-url https://cloud.example/dav \
  --webdav-username "$USER" --webdav-password "$PASS"
```

Only encrypted objects are ever synced — the target never sees plaintext keys. iCloud sync writes to an `AIPass` folder inside iCloud Drive and is macOS-only. Conflicts (the same object changed on two machines) are quarantined and resolved from the desktop app's sync settings.
