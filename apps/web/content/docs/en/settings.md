---
title: Settings
description: The desktop settings panel — lock policy, password and key rotation, sync, devices, and the local proxy.
navTitle: Settings
order: 9
---

# Settings

The desktop app's **Settings** panel groups everything operational about the vault. This page walks through each section; auto-update behavior and release channels live in [Updates](/docs/updates).

## Appearance

Theme and display preferences. These are local to the app and do not touch the vault.

## Lock policy

- **Auto-lock** — lock the vault after 15 minutes, 30 minutes, 1 hour (default), 2, 4, 8, or 24 hours of idle time, or never.
- **Lock on sleep** (default on) and **Lock on screen lock** (default on).

Locking drops decrypted keys from the agent's memory; the desktop app, CLI, and browser extension all need the master password again afterwards.

## Master password

Change the master password from the app, or with:

```bash
aipass vault change-password --new-password "$NEW"
```

Changing the password re-wraps the vault root key under a key derived from the new password — records do not need re-encryption, and the operation is quick. The recovery key stays valid.

## Rotate keys

Rotates the vault epoch key and re-wraps every record's data key under it. From the CLI:

```bash
aipass vault rotate
```

Old epoch keys cannot decrypt records written after rotation. Rotation also happens automatically when you revoke a device or recover with the recovery key. See [Security architecture](/docs/security).

## Sync

Choose one sync target:

- **Local folder** — any directory, including ones already synced by other tools.
- **WebDAV** — URL plus username and password.

From the CLI you also get `--icloud` (iCloud Drive, macOS only) and `--onedrive`:

```bash
aipass sync --dir ~/Sync/AIPass
aipass sync --icloud
aipass sync --onedrive
aipass sync --webdav-url https://cloud.example/dav --webdav-username u --webdav-password p
```

Only encrypted objects are synced. When the same object changes on two machines, the conflict is quarantined and listed in the sync settings, where you **accept** (keep the incoming version) or **discard** (keep the current one) per conflict.

## Devices, export, and import

Every machine that opens the vault registers an encrypted device record. From the CLI:

```bash
aipass vault devices                     # list trusted devices
aipass vault revoke-device <device-id>   # revoke and rotate the epoch
aipass vault export --output backup.aipexport --export-password "$PW"
aipass vault import --input backup.aipexport --export-password "$PW"
```

Export files are encrypted under their own export password; import only works into a directory without an existing vault.

## Server (local proxy)

Settings for the built-in proxy: bind address (default `127.0.0.1:8787`), routes with retry policy (max attempts 1–10, failure threshold 1–20, circuit-open seconds, connect / first-byte / stream-idle timeouts), and the model pricing table used for cost estimates. See [Desktop app](/docs/desktop#local-proxy-server).

Use `GET /health` (for example, `http://127.0.0.1:8787/health`) for local liveness checks. The proxy generates this response locally and never forwards it upstream.
