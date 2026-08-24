---
title: Browser extension
description: Install the Chrome extension, pair it with the desktop app, and use the fill grant flow.
navTitle: Extension
order: 6
---

# Browser extension

The Chromium extension (Chrome and Microsoft Edge) fills API keys into AI provider consoles without ever storing them in the browser. It talks to the AIPass agent through a Native Messaging host, and every fill is authorized by a short-lived grant from the unlocked vault.

## Install and pair

1. Install the extension from the Chrome Web Store or Microsoft Edge Add-ons, or load the package from [GitHub Releases](https://github.com/backrunner/aipass/releases) via `chrome://extensions` or `edge://extensions` with developer mode.
2. Register the native messaging host so the browser allows the pairing:

```bash
aipass native-host install --browser edge --extension-id <edge-extension-id>
```

The host name is `dev.aipass.native`. The installer writes the browser-specific manifest (on macOS, Edge uses `~/Library/Application Support/Microsoft Edge/NativeMessagingHosts/dev.aipass.native.json`) with `allowed_origins` restricted to your extension ids, and saves the allowlist the native host enforces at runtime. Chromium, Edge, and Brave work with `--browser`. The desktop app shows the native host status and can repair the manifest from its extension settings; `aipass doctor` reports the same checks.

If the extension cannot reach the vault, check in order: the desktop app is installed, the agent is running (`aipass agent status`), the vault is unlocked, and the manifest exists (`aipass doctor`).

## The fill flow

When you open a known provider console — matched against the domains and console URLs on your entries — the flow is:

1. The extension sends a context lookup with the page origin to the native host.
2. If the vault is **unlocked**, the agent returns up to 5 matching entries and issues a grant per stored key, each bound to that origin and valid for **120 seconds**.
3. The popup lists the matching entries. Pick one and click fill.
4. At fill time the extension requests a fresh grant, the agent consumes it once, and the key is filled into the page (or copied through the clipboard bridge).

A grant is single-purpose (`chrome.fill`), origin-bound, and expires after 120 seconds; expired grants are cryptographically erased — the wrapped key material is stripped from the grant file. If a grant expires before you click, the popup asks the agent for a new one. When the vault is locked, the popup can trigger an unlock instead.

If a console page shows no entries, check that the entry's domains or console URLs cover the page origin, or use the popup search to find any entry and fill it manually.

## Detecting and saving new keys

The extension scans provider console pages for API keys you create or view there. When it spots one, the popup offers a prefilled draft — title, endpoint, interface, auth scheme, tags — that you review and save straight into the vault. Nothing is sent anywhere until you confirm, and the key lands in the same encrypted envelope format as entries added from the desktop app or CLI.

## Ignored origins

From the popup you can ignore the current origin. Ignored sites are remembered in the vault and no longer trigger lookups or detection prompts — useful for pages that happen to match a provider domain but where you never want fill.

## What the extension cannot do

- It never sees your master password; unlock always happens in the desktop app's native UI or the CLI.
- It cannot list entries, reveal keys, or change the vault while the vault is locked.
- It can only talk to Chrome through the allowlisted extension ids you installed with.
