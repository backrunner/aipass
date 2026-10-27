---
title: Development and releases
description: Build AIPass, understand component ownership, run checks, and prepare releases.
navTitle: Development
order: 10
---

# Development and releases

The repository's [contribution guide](https://github.com/backrunner/aipass/blob/main/CONTRIBUTING.md) and [agent instructions](https://github.com/backrunner/aipass/blob/main/agents.md) define the current validation and commit rules.

## Prerequisites

- Node.js 26 (`.nvmrc`) and pnpm 12.3.4 (`package.json`).
- Stable Rust; on macOS use Homebrew's latest `rust` formula.
- The [Tauri platform prerequisites](https://v2.tauri.app/start/prerequisites/).

```bash
nvm install
nvm use
npm install --global pnpm@12.3.4
pnpm install --frozen-lockfile
pnpm --filter @aipass/desktop dev
```

Development builds use the isolated `aipass-dev://` scheme. Register a development build for extension-triggered launches with `pnpm --filter @aipass/desktop dev:register`. Release builds use `aipass://`.

Build the extension with `pnpm --filter @aipass/extension build`; the package is written to `apps/extension/build/aipass-extension.zip`.

### macOS toolchain

Check `command -v cargo rustc` and `rustc --version` to confirm Homebrew Rust. The macOS 27 `mis-aligned LINKEDIT string pool` issue was traced to LLVM 22's `rust-objcopy`. The documented local repair points only Rust's `rust-objcopy` symlink at Homebrew LLVM 23's `llvm-objcopy`, preserving Rust's own LLVM library dependency. Recheck that symlink after a Rust upgrade or reinstall; see the [build-toolchain guardrails](https://github.com/backrunner/aipass/blob/main/.codex/skills/aipass-revise/references/pitfalls.md).

## Component ownership

```text
Desktop UI ─────────┐
CLI ────────────────┼──> Rust Agent ───> Encrypted vault
Browser extension ──┘          ├──> Encrypted sync
                               └──> Tool configuration writers
```

- `apps/desktop`: Svelte UI and Tauri bridge.
- `crates/aipass-agent`: trusted local service and final credential access.
- `crates/aipass-vault`, `aipass-sync`: vault storage and encrypted sync.
- `crates/aipass-cli`, `aipass-native-host`: CLI and browser boundaries.
- `apps/extension`: Chromium extension.
- `apps/web`: this documentation site and update-feed Worker.
- `packages/ui`: shared UI components and locales.

The frontend is a UI layer. Storage, credential writes, sync, and final configuration changes belong to Rust. Keep subscription adapters in this repository rather than loading third-party provider engines at runtime.

## Validation

Use checks proportional to the intended change, as defined in `agents.md`. Before every commit, push, or release, the complete workspace Clippy command must pass:

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

Security, storage, sync, IPC, startup/update, dependency, build, CI, and release changes also require the full local gate:

```bash
pnpm install --frozen-lockfile
pnpm licenses:audit
pnpm lint
pnpm typecheck
pnpm test
pnpm build
git diff --exit-code -- apps/control-panel/embedded
cargo fmt --all -- --check
cargo test --workspace
cargo build --workspace
```

The gate includes a finished macOS DMG/updater bundle and all four runtime scenarios. Follow the [repository guardrails](https://github.com/backrunner/aipass/blob/main/.codex/skills/aipass-repo-guardrails/SKILL.md) for the complete commands. Local validation runs on macOS; Linux and Windows jobs run in GitHub Actions. Desktop UI changes must be reviewed at the Tauri minimum of 960×640.

Never commit real API keys, updater private keys, or unencrypted vault exports.

## Releases

The [Release workflow](https://github.com/backrunner/aipass/blob/main/.github/workflows/release.yml) accepts signed tags `vX.Y.Z`, `vX.Y.Z-beta.N`, and `vX.Y.Z-nightly.YYYYMMDD.N`. It stamps versions from the tag, builds desktop/CLI/extension artifacts, checks signatures and runtime behavior, and publishes the release and its update manifest.

1. Validate the exact source commit and required remote checks.
2. Confirm signing, notarization, provisioning-profile, and updater credentials are configured in GitHub Actions secrets.
3. Push the signed tag and monitor every release job through completion.
4. Credit merged pull requests and contributors in the release notes.
5. Download the published artifacts and verify hashes, signatures, versions, notarization, runtime behavior, and the matching channel feed.

Published tags must not be overwritten. See [desktop artifact validation](https://github.com/backrunner/aipass/blob/main/docs/desktop-artifact-validation.md), [CloudKit release setup](https://github.com/backrunner/aipass/blob/main/docs/cloudkit-release.md), and [update channels](/docs/updates).

The website has its own [Cloudflare deployment procedure](https://github.com/backrunner/aipass/blob/main/apps/web/DEPLOYMENT.md). Updating documentation does not require publishing a new desktop version.
