# Component Map

## Ownership

| Component | Owns | Must not own |
| --- | --- | --- |
| `apps/desktop/src` | Svelte presentation, interaction state, invoking Tauri commands | Vault files, provider credentials, sync objects, final tool config |
| `apps/desktop/src-tauri` | Desktop lifecycle, Tauri IPC bridge, windows, tray, singleton, updater, native-host installation | A second vault/session implementation |
| `crates/aipass-agent` | Local service, authenticated IPC, vault session, CRUD, sync orchestration, proxy and pricing state, config writes | Browser or Svelte presentation |
| `crates/aipass-agent-protocol` | Typed requests/responses, framed IPC contract, error codes, protocol version, secret-aware DTOs | Business persistence or UI behavior |
| `crates/aipass-vault` | Encrypted records, vault lifecycle, audit and grants | Desktop or browser integration |
| `crates/aipass-crypto` | KDF, AEAD, key envelopes, epoch and zeroization primitives | Product workflows |
| `crates/aipass-sync` | Encrypted local/cloud/WebDAV object exchange and conflicts | Plaintext provider semantics in remote storage |
| `crates/aipass-proxy` | Proxy runtime, routing, retry, usage storage and aggregates | Vault decryption authority |
| `crates/aipass-proxy-conversion` | Provider protocol conversion | Route persistence or UI |
| `crates/aipass-config-writers` | Plans, applies, backs up, and rolls back tool configuration | Selecting secrets without agent authorization |
| `crates/aipass-native-host` | Chromium Native Messaging validation and translation to agent requests | Direct vault reads or browser-controlled unlock state |
| `crates/aipass-cli` | Command parsing, output contract, agent client workflows | Final state writes that bypass the agent |
| `apps/extension` | Detection, user confirmation, browser UX, native messaging client | Secret persistence or trusted origin decisions alone |
| `crates/aipass-provider-registry` | Provider definitions, endpoints, auth/interface metadata | User credentials |
| `packages/ui`, `packages/schemas` | Shared UI and TypeScript contracts | Rust core authority |

The actual Rust workspace is listed in `Cargo.toml`; the JavaScript workspace and task graph are in `pnpm-workspace.yaml` and `turbo.json`.

## Primary Data Paths

```text
Svelte UI -> Tauri command -> AgentClient -> authenticated local socket -> agent -> vault/sync/proxy/writer
CLI ------------------------> AgentClient -> authenticated local socket -> agent -> core owner
Web page -> extension -> Chromium Native Messaging -> native host -> AgentClient -> agent
agent -> aipass-sync -> encrypted folder, iCloud/OneDrive-style folder, or WebDAV remote
agent -> config writer -> planned/validated tool configuration and encrypted backup
```

The per-vault socket and auth token are derived from a canonical vault path and namespace. A caller must not invent its own socket path, token store, framing, or authentication.

## Change Routing

- New agent operation: add protocol DTOs in `aipass-agent-protocol`, implement the handler in `aipass-agent`, then add thin surface adapters.
- Desktop feature: keep view state in Svelte and durable behavior in a Tauri command or, preferably, an agent request when it touches core state.
- Browser feature: validate browser input in the native host, then use an agent request. Do not return master passwords to browser-controlled UI.
- Sync feature: resolve provider paths and execute in Rust through the agent and `aipass-sync`.
- Tool integration: generate/apply through `aipass-config-writers` behind an agent plan; plaintext modes require explicit user choice.
- Proxy feature: put routing/runtime/usage behavior in `aipass-proxy` or `aipass-agent::proxy_service`; expose typed agent requests to desktop or CLI.

## Trust Boundaries

- Web page DOM -> extension content script: untrusted.
- Extension -> native host: validate extension ID, message type, protocol version, origin, and capability.
- Native host/desktop/CLI -> agent: authenticated, versioned local IPC.
- WebView -> Tauri: narrow commands; secrets should have minimal frontend lifetime.
- Remote sync/WebDAV -> sync engine: untrusted encrypted data; authenticate and validate before merge.
- Tool config files -> writers: external mutable state; use plans, backups, and rollback.

For encryption envelopes, allowed plaintext, indexes, backups, and TTL/epoch rules, read [the E2EE security model](../../../../.agents/07-security-e2ee-model.md) rather than duplicating those details here.
