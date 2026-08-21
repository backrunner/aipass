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

On first launch, AIPass asks you to set a master password. The app then creates the encrypted vault and shows a **recovery key exactly once**. Store it somewhere safe — it is the only way back in if you forget the master password.

## 3. Add a provider credential

Click **Add provider**, pick a provider (for example Anthropic), and fill in the endpoint, auth scheme, and API key. You can attach multiple keys to one provider entry and archive entries you no longer use.

Prefer the terminal? The CLI does the same thing:

```bash
aipass init
aipass add \
  --title 'Anthropic Prod' \
  --provider anthropic \
  --domain console.anthropic.com \
  --endpoint https://api.anthropic.com \
  --interface anthropic-messages \
  --auth x-api-key \
  --api-key "$ANTHROPIC_API_KEY"
```

## 4. Configure an AI tool

From the desktop app or with `aipass configure`, point Codex, Claude Code, or Gemini CLI at a stored credential. AIPass writes the tool configuration, keeps an encrypted backup of the previous state, and `aipass rollback` restores it if anything goes wrong.

```bash
aipass configure codex --entry <entry-id>
```

## 5. Install the browser extension

Install the AIPass extension from the Chrome Web Store, then connect it to the desktop app. The extension only fills keys into provider consoles after you approve a time-limited grant — see [Installation](/docs/installation) for the Native Messaging setup.

You are set. Keys now live in one encrypted vault, and every tool receives them on your terms.
