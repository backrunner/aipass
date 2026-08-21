---
title: Introduction
description: What AIPass is, how the pieces fit together, and how the vault protects your AI credentials.
navTitle: Introduction
order: 1
---

# Introduction

AIPass is a local-first credential manager for AI workflows. It stores AI provider API keys in an end-to-end encrypted vault on your machine and helps you safely configure tools such as Codex, Claude Code, and Gemini CLI.

The product has three parts that share one vault:

- **Desktop app** — a Tauri app for macOS where you add, search, probe, and archive provider credentials, manage multiple keys per provider, and export or import encrypted backups.
- **CLI** — the `aipass` command line for scripting vault operations, injecting credentials into tool environments, and configuring AI CLI tools with rollback.
- **Browser extension** — a Chrome extension that detects AI provider consoles and fills keys only with time-limited grants from the desktop app over Native Messaging.

## Supported providers

OpenAI, Anthropic, Gemini, Azure OpenAI, AWS Bedrock, OpenRouter, DeepSeek, Qwen, Moonshot, Zhipu, Volcengine Ark, Together, Fireworks, Groq, New API, One API, LiteLLM, sub2api, plus custom OpenAI-compatible endpoints and custom HTTP APIs.

## How the vault protects keys

Every provider record is stored as a whole encrypted envelope — title, domain, endpoint, auth scheme, and API keys are never written as plaintext vault or sync files.

- Argon2id master-password KDF (new vaults target 64 MiB memory and 2 rounds).
- XChaCha20-Poly1305 authenticated encryption with 256-bit keys.
- A random 256-bit vault root key wrapped by your password-derived key and by an emergency recovery key, shown once at vault creation.
- Per-record random data keys wrapped by a rotating vault epoch key.
- Time-limited grants for browser fill; expired grants are cryptographically erased.
- HMAC fingerprints enable API-key search without storing keys in plaintext.

Sync (local/iCloud folder or WebDAV) replicates encrypted objects only.

## Where to go next

- [Quick start](/docs/quick-start) — install the app and store your first key.
- [Installation](/docs/installation) — desktop and browser extension setup details.
- [Settings and updates](/docs/settings-and-updates) — release channels and auto-update behavior.
