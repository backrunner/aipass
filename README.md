<p align="center">
  <img src="apps/desktop/public/aipass-logo.png" width="144" alt="AIPass logo">
</p>

<h1 align="center">AIPass</h1>

<p align="center">
  A local-first, end-to-end encrypted credential manager for AI providers.
</p>

<p align="center">
  <a href="https://github.com/backrunner/aipass/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/backrunner/aipass/ci.yml?branch=main&style=flat-square&label=CI" alt="CI status"></a>
  <a href="https://github.com/backrunner/aipass/releases"><img src="https://img.shields.io/github/v/release/backrunner/aipass?include_prereleases&style=flat-square&label=release" alt="Latest release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-0f6fff?style=flat-square" alt="Apache 2.0 license"></a>
</p>

<p align="center">
  <a href="https://aipass.alkinum.io/docs/">Documentation</a> ·
  <a href="https://github.com/backrunner/aipass/releases">Download</a> ·
  <a href="https://aipass.alkinum.io/docs/quick-start/">Quick start</a>
</p>

AIPass brings AI provider API keys and supported subscription accounts into one local workspace. A Rust agent owns credential access; the desktop app, CLI, and browser extension share the same encrypted vault.

## Features

- Manage providers, multiple keys, subscription accounts, models, and quotas.
- Configure installed AI tools with a preview and encrypted backups for rollback.
- Route requests through a local proxy with protocol conversion, Responses WebSocket support, and per-key usage estimates.
- Sync and back up encrypted vault data. Subscription login credentials stay encrypted on each device.
- Fill credentials on provider consoles through the Chromium browser extension.

## Get started

1. [Download a release](https://github.com/backrunner/aipass/releases) and follow the [installation guide](https://aipass.alkinum.io/docs/installation/).
2. Create your vault and keep the one-time recovery key offline.
3. Add a credential or connect a subscription, then configure an installed AI tool.

Desktop releases are available for macOS (Universal) and Linux (x86_64 / aarch64). Standalone CLI archives are available for macOS, Linux, and Windows.

AIPass is pre-release software. Read the release notes before upgrading.

## Documentation

- [Quick start](https://aipass.alkinum.io/docs/quick-start/)
- [Tool integrations and subscriptions](https://aipass.alkinum.io/docs/integrations/)
- [Local proxy and WebSocket support](https://aipass.alkinum.io/docs/local-proxy/)
- [CLI reference](https://aipass.alkinum.io/docs/cli/)
- [Security](https://aipass.alkinum.io/docs/security/) and [updates](https://aipass.alkinum.io/docs/updates/)
- [中文文档](https://aipass.alkinum.io/docs/zh/)

## Development

Use Node.js 26, pnpm 12.3.4, and stable Rust (Homebrew on macOS). See the [development guide](https://aipass.alkinum.io/docs/development/) for platform prerequisites, architecture, checks, and release procedures.

```bash
pnpm install --frozen-lockfile
pnpm --filter @aipass/desktop dev
```

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) and report security issues through [SECURITY.md](SECURITY.md).

## License

[Apache License 2.0](LICENSE). Third-party notices are in [NOTICE](NOTICE).
