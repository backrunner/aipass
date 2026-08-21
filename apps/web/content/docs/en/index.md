---
title: Introduction
description: What AIPass is, how the pieces fit together, and how the vault protects your AI credentials.
navTitle: Introduction
order: 1
---

# Introduction

AIPass is a local-first credential manager for AI workflows. It stores AI provider API keys in an end-to-end encrypted vault on your machine and helps you safely configure tools such as Codex, Claude Code, and Gemini CLI.

The product has three parts that share one vault:

- **Desktop app** — a Tauri app for macOS where you add, search, probe, and archive provider credentials, manage multiple keys per provider, route traffic through a local proxy, and export or import encrypted backups.
- **CLI** — the `aipass` command line for scripting vault operations, injecting credentials into tool environments, and configuring AI CLI tools with rollback. See the [CLI reference](/docs/cli).
- **Browser extension** — a Chrome extension that detects AI provider consoles and fills keys through short-lived, origin-bound grants over Native Messaging. See the [extension guide](/docs/extension).

A background **agent** process holds the unlocked vault session. The desktop app, the CLI, and the browser extension's native host all talk to the same agent over a local socket, so you unlock once and every surface shares the session until it locks.

## Supported providers

The built-in registry knows OpenAI, Anthropic, Gemini, Azure OpenAI, AWS Bedrock, OpenRouter, DeepSeek, Moonshot, Qwen, Zhipu, Volcengine Ark, Together, SiliconFlow, xAI, Mistral, Cohere, Perplexity, Cerebras, NVIDIA, Novita, MiniMax, Hugging Face, Fireworks, Groq, Replicate, New API, One API, LiteLLM, sub2api, Veloera, OmniRoute, and Metapi — plus custom OpenAI-compatible endpoints and custom HTTP APIs. Entries can also carry no provider id at all; the registry is used for icons, console detection, and sensible defaults, not as a hard requirement.

## How the vault protects keys

Every provider record is stored as a whole encrypted envelope — title, domain, endpoint, auth scheme, and API keys are never written as plaintext vault or sync files.

- Argon2id master-password KDF (new vaults use 64 MiB memory, 2 iterations, parallelism 1).
- XChaCha20-Poly1305 authenticated encryption with 256-bit keys and random 192-bit nonces.
- A random 256-bit vault root key wrapped by your password-derived key and by an emergency recovery key, shown once at vault creation.
- Per-record random data keys wrapped by a rotating vault epoch key.
- Browser fill uses grants that expire after 120 seconds; expired grants are cryptographically erased.
- HMAC-SHA256 fingerprints enable API-key search without storing keys in plaintext.

Sync (local folder, iCloud Drive, OneDrive, or WebDAV) replicates encrypted objects only. See [Security architecture](/docs/security) for the full model.

## Where to go next

- [Quick start](/docs/quick-start) — install the app and store your first key.
- [Installation](/docs/installation) — desktop, CLI, and browser extension setup details.
- [CLI reference](/docs/cli) — every `aipass` command with flags and examples.
- [Desktop app](/docs/desktop) — unlock, tray, autostart, integrations, and the local proxy.
- [Browser extension](/docs/extension) — pairing, the fill grant flow, and key detection.
- [Security architecture](/docs/security) — encryption, recovery key, rotation, devices, export/import.
- [Updates](/docs/updates) — official vs beta feeds, auto-update behavior, and switching channels.
- [Settings](/docs/settings) — the settings panel: lock policy, rotation, sync, devices, proxy.
