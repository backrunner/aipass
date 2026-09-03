---
title: Desktop app
description: Unlock and lock behavior, the tray, launch at login, integrations, and the local proxy.
navTitle: Desktop app
order: 5
---

# Desktop app

The desktop app is the primary interface to the vault. It is a Tauri app for macOS and talks to the same background agent as the CLI, so an unlock in the app also unlocks `aipass` and the browser extension.

## Vault, unlock, and lock

On first launch the app asks for a master password, creates the vault, and shows the recovery key once — store it offline immediately. On later launches the app shows an unlock screen; entering the master password unlocks the shared agent session.

The vault locks itself according to the lock policy in **Settings → Lock policy**:

- **Auto-lock** after idle: 15 minutes, 30 minutes, 1 hour (default), 2, 4, 8, or 24 hours, or never.
- **Lock on sleep** (default on) — the vault locks when the Mac sleeps.
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

The Integrations section configures AI tools to use a stored credential, with a preview dialog before anything is written. Supported tools:

- **Codex**, **Claude Code**, **Gemini CLI**, **OpenCode** — also available from `aipass configure`.
- **Grok**, **Pi**, **Cursor Agent Local** — desktop-only integrations.

Compatibility is checked per entry: for example Codex requires an OpenAI-compatible endpoint with bearer auth, and Claude Code requires an Anthropic Messages interface. Every apply writes an encrypted backup of the previous configuration and can be rolled back.

## Local proxy (Server)

The **Server** section runs a local HTTP proxy that lets tools share vault credentials without holding real keys. Highlights:

- Binds to `127.0.0.1:8787` by default; the address is configurable.
- Routes define an inbound protocol (OpenAI Responses, OpenAI Chat Completions, or Anthropic Messages). Targets can use any supported provider format: when a target's format differs from the inbound protocol, the proxy converts between protocols automatically (e.g. Claude Code can run on OpenAI-format providers).
- Each route has its own bearer token, a strategy (fallback or round-robin), and weighted targets pointing at vault entries — so a failed provider falls over to the next target automatically.
- Retry policy per route: max attempts, failure threshold, circuit-open seconds, connect / first-byte / stream-idle timeouts.
- Known conversion limits: `/v1/messages/count_tokens` is not converted; `thinking` and `cache_control` fields are dropped across protocols; `anthropic-beta` feature headers are not forwarded to OpenAI upstreams.
- Usage statistics per provider and model — request counts, tokens, estimated cost from your pricing table, success rate, time to first token — stored locally in SQLite.

Because targets reference vault entries, rotating a key in the vault updates the proxy without touching your tools.

## Tray

AIPass lives in the macOS menu bar. The tray menu shows agent and proxy status and offers:

- **Open AIPass** / **Hide Window**, **Refresh Status**, **Quit**.
- Start the agent, **Lock Vault**, and install launch-at-login for the tray.
- Proxy controls: start, stop, refresh, and open the Server page. The tray also shows recent request rates (RPM/TPM).

## Launch at login

Two independent autostart entries exist, both registered as macOS LaunchAgents:

- The **agent** keeps your unlocked session and serves the CLI and browser extension (`aipass agent install`).
- The **tray** app can start at login so the menu bar icon is always present; install it from the tray menu.

## Updates

The app checks for updates in the background and shows a banner when one is available; **Settings → Updates** switches between the official and beta channels. Details are in [Updates](/docs/updates).
