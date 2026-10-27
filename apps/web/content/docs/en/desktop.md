---
title: Desktop app
description: Unlock and lock behavior, the tray, launch at login, integrations, and the local proxy.
navTitle: Desktop app
order: 5
---

# Desktop app

The desktop app is the primary interface to the vault. It is a Tauri app and talks to the same background agent as the CLI, so an unlock in the app also unlocks `aipass` and the browser extension.

## Vault, unlock, and lock

On first launch the app asks for a master password, creates the vault, and shows the recovery key once — store it offline immediately. On later launches the app shows an unlock screen; entering the master password unlocks the shared agent session.

The vault locks itself according to the lock policy in **Settings → Lock policy**:

- **Auto-lock** after idle: 15 minutes, 30 minutes, 1 hour (default), 2, 4, 8, or 24 hours, or never.
- **Lock on sleep** (default on) — the vault locks when the computer sleeps.
- **Lock on screen lock** (default on) — the vault locks when you lock the screen.

Quitting the app or restarting the agent also locks the session. Locking is cryptographic: the agent drops the decrypted keys from memory, so nothing — including the browser extension — can read secrets until you unlock again.

If you forget the master password, use the recovery key from the unlock screen. Recovery sets a new master password, shows a **new** recovery key, and rotates the vault epoch — the old recovery key stops working.

## Main window

The sidebar organizes your entries:

- **Vault** — All items, Favorites, Recent.
- **Providers** — entries grouped by kind: Official, Third-party, Self-hosted, Custom.
- **Storage** — Server (the local proxy), Archive, Trash.

The provider detail pane shows endpoints, console URLs, masked keys, model aliases, quota, tags, and notes, and offers copy, reveal, probe, configure, archive, and delete actions. Each entry can hold multiple labeled keys.

External tools can open the provider form with the AIPass-native deep link scheme:

`aipass-provider://v1/add?title=Relay&providerId=custom_http&domain=relay.example.com&endpoint=https%3A%2F%2Frelay.example.com%2Fv1&interfaceType=openai_compatible&authScheme=bearer&apiKey=...`

The `v1/add` path is versioned. Repeat `domain`, `endpoint`, `consoleEndpoint`, and `tag` for multiple values; encode `modelAliases`, `headers`, and `quota` as JSON query values. The link opens the normal add form, and the Rust agent remains the only component that persists the provider record.

## Integrations

Use **Quick integration** to configure an installed AI tool from a credential or proxy route. Review the plan before applying it; previous configurations are backed up with encryption for rollback. Supported CLI tools include Codex, Claude Code, Gemini CLI, OpenCode, Grok, Pi, and Cursor Agent Local.

See [Tool integrations](/docs/integrations) for key selection, subscription accounts, and CC Switch imports.

## Local proxy

The local proxy uses route tokens while provider credentials stay in AIPass. Configure routes, target selection, protocol conversion, WebSocket preference, and per-key usage pricing in **Local proxy**.

See [Local proxy](/docs/local-proxy) for session behavior, conversion limits, safe fallback, and the LAN control panel.

## Tray

AIPass uses the system tray or macOS menu bar. The tray menu shows agent and proxy status and offers:

- **Open AIPass** / **Hide Window**, **Refresh Status**, **Quit**.
- Start the agent, **Lock Vault**, and install launch-at-login for the tray.
- Proxy controls: start, stop, refresh, and open the Server page. The tray also shows recent request rates (RPM/TPM).

## Launch at login

Agent and tray startup are configured independently. macOS uses LaunchAgents, Windows uses per-user startup registration, and Linux uses its supported session backend (including a desktop-session fallback when no systemd user manager is available):

- The **agent** keeps your unlocked session and serves the CLI and browser extension (`aipass agent install`).
- The **tray** app can start at login so the menu bar icon is always present; install it from the tray menu.

## Updates

The app checks for updates in the background and shows a banner when one is available; **Settings → Updates** switches between the official, beta, and nightly channels. Details are in [Updates](/docs/updates).
