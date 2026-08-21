---
title: Quick start
description: Install AIPass, create your vault, store a key, and configure your first AI tool.
navTitle: Quick start
order: 2
---

# Quick start

## 1. Install the desktop app

Download AIPass for macOS from the [home page](/) — the buttons link directly to the newest GitHub Release. Open the `.dmg` and move AIPass to Applications.

## 2. Create your vault

On first launch, AIPass asks you to set a master password. The app then creates the encrypted vault and shows a **recovery key exactly once** — a string like `AIPASS-XXXX-XXXX-…`. Store it somewhere safe offline; it is the only way back in if you forget the master password, and it cannot be displayed again.

From the terminal, the equivalent is:

```bash
aipass init
```

`aipass init` prints the recovery key once and expects a password via `--password` or the `AIPASS_MASTER_PASSWORD` environment variable. The vault lives at `~/Library/Application Support/dev.aipass.desktop/vault` unless you override it with `--vault` or `AIPASS_VAULT_DIR`.

## 3. Add a provider credential

Click **Add provider**, pick a provider (for example Anthropic), and fill in the endpoint, auth scheme, and API key. You can attach multiple keys to one provider entry and archive entries you no longer use.

Prefer the terminal? The CLI does the same thing:

```bash
aipass add \
  --title 'Anthropic Prod' \
  --provider anthropic \
  --domain console.anthropic.com \
  --endpoint https://api.anthropic.com \
  --interface anthropic-messages \
  --auth x-api-key \
  --api-key "$ANTHROPIC_API_KEY"
```

The command prints the new entry's UUID. If the vault is locked, the CLI prompts for the master password (or reads `--password` / `AIPASS_MASTER_PASSWORD`).

Verify the key works against the real endpoint:

```bash
aipass probe <entry-id>
```

## 4. Configure an AI tool

From the desktop app's Integrations section or with `aipass configure`, point Codex, Claude Code, Gemini CLI, or OpenCode at a stored credential. Without `--yes` the command only prints a preview of the changes; with `--yes` it applies them and keeps an encrypted backup of the previous state:

```bash
# preview
aipass configure claude-code <entry-id>

# apply
aipass configure claude-code <entry-id> --yes
```

The apply output includes an operation id. If anything goes wrong, restore the previous configuration:

```bash
aipass rollback <operation-id>
```

Helper mode (the default for Claude Code) never writes your key to disk — it points the tool at `aipass get` so the key is fetched from the vault at runtime. See the [CLI reference](/docs/cli#configuring-ai-tools) for modes and per-tool details.

## 5. Install the browser extension

Install the AIPass extension from the Chrome Web Store, then pair it with the desktop app:

```bash
aipass native-host install --extension-id <chrome-extension-id>
```

On a provider console page, open the extension popup, pick a matching entry, and click fill. The vault must be unlocked; every fill uses a short-lived, origin-bound grant. See the [extension guide](/docs/extension).

You are set. Keys now live in one encrypted vault, and every tool receives them on your terms.
