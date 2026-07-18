# AIPass

AIPass is a local-first AI Provider credential manager for desktop, CLI, and browser workflows. It stores AI API credentials in an end-to-end encrypted vault and helps users safely configure tools such as Codex, Claude Code, and Gemini CLI.

## 1.0 Scope

- Monorepo: `pnpm` workspace + Turborepo + Rust workspace.
- Desktop: Tauri + Svelte vault UI with provider CRUD, multi-secret management, search, official/third-party/self-hosted/custom filters, archive, reveal/copy, provider probe, encrypted export/import, auto-lock, clipboard cleanup, device revoke, local/iCloud folder sync, and WebDAV sync.
- CLI: `aipass` commands for `init`, `add`, `update`, `list`, `search`, `get`, `copy`, `secret`, `probe`, `env`, `exec`, `configure`, `rollback`, `sync`, `native-host`, `completions`, and `vault rotate/change-password/devices/revoke-device/export/import`.
- Browser: Chrome MV3 extension with Native Messaging, context lookup, fill grants, detected-key save flow, ignored origins, and self-hosted gateway hints for New API, One API, LiteLLM, and sub2api.
- Providers: OpenAI, Anthropic, Gemini, Azure OpenAI, AWS Bedrock, OpenRouter, DeepSeek, Qwen, Moonshot, Zhipu, Volcengine Ark, Together, Fireworks, Groq, New API, One API, LiteLLM, sub2api, custom OpenAI-compatible, and custom HTTP API.
- Sync: local/iCloud folder sync and WebDAV sync of encrypted object families only.
- License: Apache-2.0.

## Security Model

AIPass encrypts provider records as whole encrypted envelopes. Provider title, domain, endpoint, auth scheme, interface type, quota, notes, headers, and API keys are not written as plaintext vault or sync files.

Core properties:

- Argon2id master-password KDF with per-vault stored parameters; new vaults target 64 MiB memory and 2 rounds for responsive unlocks.
- Random 256-bit vault root key wrapped by the password-derived key and by an emergency recovery key.
- XChaCha20-Poly1305 authenticated encryption with 256-bit symmetric keys. This is the practical post-quantum posture for this local vault; AIPass does not introduce a PQ public-key suite for local password unlock.
- Per-record random DEK wrapped by the current Vault Epoch Key.
- Epoch ratchet with fresh OS CSPRNG material for compromise recovery.
- Recovery keys are shown once at vault creation and after successful recovery. A recovery reset writes a fresh recovery key, invalidates the old one, changes the master password, advances the epoch, and rewraps active objects.
- TTL grants for browser fill and temporary secret access; expired grants are cryptographically erased by removing wrapped key material.
- HMAC fingerprints for API-key search without storing the key in plaintext.
- Encrypted config backups for CLI tool configuration rollback.
- Encrypted vault export/import for backup and migration; exported files do not contain plaintext provider metadata or API keys.

Important boundary: no local-only system can retroactively make an attacker forget an old ciphertext and key that were both copied before rotation. AIPass uses epoch rotation and TTL erasure to stop future exposure and make expired grants unrecoverable when their wrapping material is gone. Vault format v2 is pre-release only; this repository does not include a migration path for earlier local vault formats. See [SECURITY.md](SECURITY.md) and [.agents/07-security-e2ee-model.md](.agents/07-security-e2ee-model.md).

## Development

```bash
pnpm install
pnpm typecheck
pnpm test
pnpm build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
```

Desktop development:

```bash
pnpm --filter @aipass/desktop dev
```

Register and build the development desktop app before using extension-triggered launches:

```bash
pnpm --filter @aipass/desktop dev:register
```

The development app uses the isolated `aipass-dev://` URL scheme; release builds keep `aipass://`.

Extension build:

```bash
pnpm --filter @aipass/extension build
```

Desktop bundle:

```bash
pnpm --filter @aipass/desktop bundle
```

Release artifacts are produced by the `Release` GitHub Actions workflow on stable `vX.Y.Z` tags or manual dispatch with an existing tag. The desktop release path fully supports macOS first: it builds a universal Tauri app, signs and notarizes the `.app`/DMG, creates Tauri updater artifacts, and uploads `latest.json` plus the versioned bundles to GitHub Releases. Publishing the draft GitHub Release makes `https://github.com/<owner>/<repo>/releases/latest/download/latest.json` available to the in-app updater.

Required macOS release secrets:

- `APPLE_CERTIFICATE` and `APPLE_CERTIFICATE_PASSWORD` for the Developer ID Application certificate. `CSC_LINK` and `CSC_KEY_PASSWORD` are accepted as fallbacks for compatibility with the Iconwiz release setup.
- `APPLE_API_KEY_BASE64`, `APPLE_API_KEY_ID`, and `APPLE_API_ISSUER` for Apple notarization.
- `TAURI_SIGNING_PRIVATE_KEY`, optional `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, and `TAURI_SIGNING_PUBLIC_KEY` for updater artifact signatures.

CLI example:

```bash
export AIPASS_MASTER_PASSWORD='correct horse battery staple'
cargo run -p aipass-cli -- --vault ./dev-vault init
# Store the one-time recovery key printed by init before continuing.
cargo run -p aipass-cli -- --vault ./dev-vault add \
  --title 'Anthropic Prod' \
  --provider anthropic \
  --domain console.anthropic.com \
  --endpoint https://api.anthropic.com \
  --interface anthropic-messages \
  --auth x-api-key \
  --api-key "$ANTHROPIC_API_KEY"
cargo run -p aipass-cli -- --vault ./dev-vault secret add <entry-id> \
  --label fallback \
  --api-key "$ANTHROPIC_FALLBACK_API_KEY"
cargo run -p aipass-cli -- --vault ./dev-vault vault export \
  --output ./aipass.aipexport \
  --export-password "$AIPASS_EXPORT_PASSWORD"
```

## Native Host

Install or print a Chrome Native Messaging manifest:

```bash
cargo run -p aipass-cli -- native-host manifest --extension-id <chrome-extension-id>
cargo run -p aipass-cli -- native-host install --extension-id <chrome-extension-id>
```

The installer writes Chrome `allowed_origins` and the native host extension-id allowlist. `AIPASS_ALLOWED_EXTENSION_IDS` can still override that allowlist for managed deployments. The Chrome manifest is the first browser-side boundary; native host extension-id validation is the second boundary.

## Documentation

- [.agents/01-research.md](.agents/01-research.md)
- [.agents/02-requirements.md](.agents/02-requirements.md)
- [.agents/03-ui-design.md](.agents/03-ui-design.md)
- [.agents/04-architecture.md](.agents/04-architecture.md)
- [.agents/05-development-plan.md](.agents/05-development-plan.md)
- [.agents/06-roadmap.md](.agents/06-roadmap.md)
- [.agents/07-security-e2ee-model.md](.agents/07-security-e2ee-model.md)
- [.agents/08-implementation-status.md](.agents/08-implementation-status.md)

## License

Apache-2.0.
