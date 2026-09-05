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
  <a href="#what-aipass-does">Features</a> &middot;
  <a href="#quick-start">Quick start</a> &middot;
  <a href="#security-model">Security</a> &middot;
  <a href="#development">Development</a> &middot;
  <a href="#documentation">Documentation</a>
</p>

AIPass keeps API credentials for OpenAI, Anthropic, Gemini, self-hosted gateways, and other AI providers in an encrypted local vault. The desktop app, CLI, and browser extension work together so credentials can be found, injected, and configured without turning a cloud service or browser storage into the source of truth.

> [!NOTE]
> AIPass is pre-release software. Vault format v2 does not include a migration path for earlier development vault formats yet.

## What AIPass Does

| Surface     | Capabilities                                                                                                                                                          |
| ----------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Desktop** | Manage providers and multiple secrets, search and filter entries, probe credentials, import/export encrypted vaults, configure sync, rotate keys, and revoke devices. |
| **CLI**     | Read and copy secrets, inject environment variables, run commands with temporary credentials, configure AI tools, manage the vault, and run sync.                     |
| **Browser** | Detect provider pages, look up matching credentials, fill approved secrets, and save newly created keys through Chromium Native Messaging.                            |
| **Sync**    | Sync encrypted object families through a local folder, iCloud-style folder, or WebDAV endpoint.                                                                       |

### Provider Coverage

AIPass includes definitions for OpenAI, Anthropic, Gemini, Azure OpenAI, AWS Bedrock, OpenRouter, DeepSeek, Qwen, Moonshot, Zhipu, Volcengine Ark, Together, Fireworks, Groq, New API, One API, LiteLLM, sub2api, custom OpenAI-compatible services, and custom HTTP APIs.

It can configure Codex, Claude Code, Gemini CLI, and OpenCode while keeping configuration backups encrypted for rollback.

### Local Proxy WebSocket Support

The local proxy accepts OpenAI [Responses WebSocket mode](https://developers.openai.com/api/docs/guides/websocket-mode) at `ws://127.0.0.1:8787/v1/responses` (replace the address if configured differently). Use the Responses route's local token in `Authorization: Bearer <route-token>`; AIPass injects the selected provider credential upstream. Keep the provider base URL as `https://...` or `http://...`, just as for HTTP requests.

With route conversion disabled, the proxy relays to a native Responses WebSocket endpoint. With conversion enabled, each `response.create` uses the shared HTTP/SSE forwarding pipeline: Responses targets receive a Responses HTTP request, and Anthropic Messages targets receive a translated Messages request. SSE events are returned as Responses WebSocket events, including text, function calls, and tool results in subsequent requests. Usage is recorded per response. Outbound system, environment, and custom proxy settings apply to both modes.

Native WS mode selects and retries targets before the connection opens and never replays or switches upstreams after opening. Converted mode applies the route's HTTP retry policy separately to each response, with no replay after emitting its events. It supports ordered `stream_id` lanes, parallel lanes and forks, and `previous_response_id` through an in-memory cache of the latest response in each lane (up to 64 MiB of cached context per connection). Full input context is replayed to HTTP targets for continuations. Converted `generate: false` warms only the local context cache; it does not call or prewarm an upstream model. Responses are not persisted by this local cache, even with `store: true`; uncached IDs require resending full input. Background generation, server-managed conversations and compaction, and mid-turn steering require native Responses mode.

Credential/configuration refreshes close existing connections; clients must reconnect and recover their conversation state. The response idle timeout applies while responses are pending, allowing idle connections between tool calls. OpenAI Chat Completions WebSocket conversion, browser subprotocol authentication, and the separate Realtime audio API are not supported.

## Quick Start

### Prerequisites

- Node.js 24 and `pnpm` 11.11.0
- Stable Rust
- The [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your operating system

Install the workspace dependencies:

```bash
pnpm install --frozen-lockfile
```

Start the desktop app in development mode:

```bash
pnpm --filter @aipass/desktop dev
```

The development app uses the isolated `aipass-dev://` URL scheme; release builds use `aipass://`. To register a development build for extension-triggered launches, run:

```bash
pnpm --filter @aipass/desktop dev:register
```

Build the browser extension:

```bash
pnpm --filter @aipass/extension build
```

The verified Manifest V3 package is written to `apps/extension/build/aipass-extension.zip`.

### CLI Example

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
```

Run `cargo run -p aipass-cli -- --help` to see the complete command surface.

## Security Model

AIPass encrypts each provider record as a complete envelope. Titles, domains, endpoints, authentication schemes, interface types, quotas, notes, headers, and API keys are not written as plaintext vault or sync files.

Core properties:

- Argon2id master-password derivation with parameters stored per vault. New vaults target 64 MiB of memory and two rounds for responsive unlocks.
- A random 256-bit vault root key wrapped by both the password-derived key and an emergency recovery key.
- XChaCha20-Poly1305 authenticated encryption with 256-bit symmetric keys.
- A random data-encryption key per record, wrapped by the current Vault Epoch Key.
- Epoch rotation using fresh OS CSPRNG material for compromise recovery and future writes.
- One-time recovery keys, plus recovery reset that invalidates the old key, changes the master password, advances the epoch, and rewraps active objects.
- TTL grants for browser fill and temporary access. Expired grants are cryptographically erased by removing their wrapped key material.
- HMAC fingerprints for API-key search without plaintext key indexes.
- Encrypted configuration backups and encrypted vault export/import.

No local-only system can make an attacker forget an old ciphertext and key that were both copied before rotation. Epoch rotation protects future writes, while TTL erasure makes expired grants unrecoverable after their wrapping material is removed. Read [SECURITY.md](SECURITY.md) and the [E2EE security model](.agents/07-security-e2ee-model.md) for the full threat model and disclosure policy.

## Architecture

```text
Desktop UI ─────────┐
CLI ────────────────┼──> AIPass Agent ───> Encrypted local vault
Browser extension ──┘          │
                               ├──> Encrypted folder / iCloud / WebDAV sync
                               └──> AI tool configuration writers
```

The Rust agent owns final vault access, sync, and configuration writes. The Svelte desktop app remains a UI layer, while the browser extension reaches the vault only through the authenticated Native Messaging boundary.

| Path                        | Purpose                                            |
| --------------------------- | -------------------------------------------------- |
| `apps/desktop`              | Tauri desktop shell and Svelte UI                  |
| `apps/extension`            | Chromium Manifest V3 extension                     |
| `crates/aipass-agent`       | Trusted local core service                         |
| `crates/aipass-cli`         | Command-line interface                             |
| `crates/aipass-vault`       | Encrypted vault model and operations               |
| `crates/aipass-sync`        | Local folder, iCloud-style folder, and WebDAV sync |
| `crates/aipass-native-host` | Browser Native Messaging boundary                  |
| `packages/ui`               | Shared Svelte UI components                        |

## Development

Run the repository checks from the workspace root:

```bash
pnpm licenses:audit
pnpm lint
pnpm typecheck
pnpm test
pnpm build

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
```

Create a desktop bundle with:

```bash
pnpm --filter @aipass/desktop bundle
```

See [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request. Never commit real API keys or unencrypted vault exports.

## Native Host

Install or print a Chrome Native Messaging manifest:

```bash
cargo run -p aipass-cli -- native-host manifest --extension-id <chrome-extension-id>
cargo run -p aipass-cli -- native-host install --extension-id <chrome-extension-id>
```

The installer writes Chrome `allowed_origins` and the native-host extension ID allowlist. `AIPASS_ALLOWED_EXTENSION_IDS` can override that allowlist in managed deployments. The Chrome manifest is the first browser-side boundary; native-host extension ID validation is the second.

## Release Notes for Maintainers

Release artifacts are produced by the `Release` GitHub Actions workflow on `vX.Y.Z` official tags, `vX.Y.Z-beta.N` beta tags, or manual dispatch with an existing tag. The current desktop release path supports macOS first: it stamps the tag version into workspace manifests, builds a universal Tauri app, signs and notarizes the app and DMG, creates updater artifacts, and publishes them to GitHub Releases.

The desktop updater reads `latest.json` from these feeds:

```text
Official: https://github.com/backrunner/aipass/releases/latest/download/latest.json
Beta:     https://aipass.alkinum.io/api/updates/beta/latest.json
```

Official releases become GitHub's latest release. Beta releases are marked as prereleases and carry their own normalized `latest.json` asset; the beta feed is resolved by the website Worker, which returns the manifest of the newest published prerelease — no rolling `beta` tag or separate update server is involved. Users pick the channel in Settings → Updates; builds whose version contains `-` default to Beta. The desktop checks its selected feed every 24 hours, downloads and verifies new packages in the background, then offers an immediate restart; a deferred package is rechecked and installed automatically on the next launch when the selected feed is reachable.

<details>
<summary>Required macOS release secrets</summary>

- `APPLE_CERTIFICATE` and `APPLE_CERTIFICATE_PASSWORD` for the Developer ID Application certificate. `CSC_LINK` and `CSC_KEY_PASSWORD` are accepted as fallbacks.
- `APPLE_SIGNING_IDENTITY` for the Developer ID Application identity used to sign the app.
- `APPLE_TEAM_ID` for the Apple Developer team ID; the signed app's TeamIdentifier is verified against it.
- App Store Connect notarization credentials (preferred): `APPLE_API_KEY_ID`, `APPLE_API_ISSUER`, and `APPLE_API_KEY_BASE64`, all provided together.
- Apple ID notarization credentials (fallback): `APPLE_ID` and `APPLE_PASSWORD`, both provided together.
- `TAURI_SIGNING_PRIVATE_KEY` and optional `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` for updater artifact signatures. The matching public key is committed in `apps/desktop/src-tauri/tauri.conf.json`; `TAURI_SIGNING_PUBLIC_KEY` can override it for a build.

</details>

<details>
<summary>Release procedure</summary>

1. Ensure CI is green on `main` and the versions in `package.json`, `apps/desktop/package.json`, `apps/desktop/src-tauri/tauri.conf.json`, and `Cargo.toml` agree.
2. Create and push an official or beta version tag, or manually run the workflow with an existing tag and channel.
3. The workflow validates the tag, builds and notarizes the macOS desktop app, packages the CLI, native host, and Chromium extension, and assembles a draft GitHub Release.
4. The publish step rewrites `latest.json`, verifies both macOS architectures and their signatures, and publishes the release.
5. For a beta, the rolling feed is refreshed only when the new version is semver-newer than the current beta feed.

The workflow refuses to overwrite an already published tag. Only rerun it while the release is still a draft.

</details>

Microsoft Edge Add-ons submission material lives in [`apps/extension/store`](apps/extension/store). Release builds publish the verified extension package as `aipass-edge-extension.zip`.

## Documentation

- [Local logs and troubleshooting](docs/local-logging.md)
- [Product research](.agents/01-research.md)
- [Requirements](.agents/02-requirements.md)
- [UI design](.agents/03-ui-design.md)
- [Architecture](.agents/04-architecture.md)
- [Development plan](.agents/05-development-plan.md)
- [Roadmap](.agents/06-roadmap.md)
- [E2EE security model](.agents/07-security-e2ee-model.md)
- [Implementation status](.agents/08-implementation-status.md)

## License

Licensed under the [Apache License 2.0](LICENSE).
