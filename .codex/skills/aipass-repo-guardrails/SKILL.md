# AIPass Repo Guardrails

Use this skill whenever you change architecture, storage, sync, desktop integration, browser integration, CLI config writing, or IPC in the AIPass repository.

## Repository shape

- `apps/desktop`
  - Svelte UI only.
  - `src-tauri` is the desktop bridge into Rust.
- `crates/aipass-agent`
  - Primary local core service.
  - Owns vault access, sync execution, config writes, and trusted local IPC.
- `crates/aipass-agent-protocol`
  - Structured request/response types for local IPC.
- `crates/aipass-sync`
  - Local folder, iCloud-style folder, OneDrive-style folder, and WebDAV sync logic.
- `crates/aipass-native-host`
  - Browser native messaging boundary.
- `crates/aipass-cli`
  - CLI surface that must call the Rust core service instead of writing final state directly.

Read these repo docs before large changes:

- `.agents/04-architecture.md`
- `.agents/07-security-e2ee-model.md`
- `.agents/08-implementation-status.md`

## Non-negotiable architecture rules

1. Final data access must go through the Rust core service.
2. The desktop frontend must stay a UI layer, not a data or storage authority.
3. Browser extension and native host flows must never become an alternate source of truth.
4. Sync providers must reuse the core sync engine. Frontend code must not manipulate vault or sync objects directly.
5. CLI provider switching or external tool config writing must go through Rust core plans and writers.

## Storage rules

- Persistent storage belongs in Rust crates, primarily `aipass-agent`, `aipass-vault`, `aipass-sync`, and related storage helpers.
- Do not add new frontend-side persistence for vault data, provider credentials, sync state, or tool config state.
- Do not let TypeScript write final secrets, final sync objects, or final provider config files directly.
- If a new setting must persist, prefer a Rust-owned command and file format over browser or frontend local storage.

## IPC and secret-handling rules

- All IPC must use typed protocol messages from `aipass-agent-protocol`.
- Do not add unauthenticated local socket, pipe, or file-based command channels.
- Sensitive fields must use dedicated secret types such as `SensitiveString`, not plain `String`.
- Do not log, clone, or cache master passwords, API keys, recovery keys, or decrypted secrets unless absolutely required for the immediate operation.
- Zeroize sensitive buffers when practical and keep exposure windows short.
- Extension and native host paths must not accept user-entered master passwords in browser-controlled surfaces.
- Desktop-to-Rust requests that carry secrets must be minimal, short-lived, and never persisted in the frontend.

## Sync rules

- Supported sync targets are implemented by the core sync engine.
- Local folder, iCloud, OneDrive, and WebDAV flows must resolve and execute in Rust.
- Cloud folder discovery belongs in Rust. The UI may select a mode, but it must not decide the final filesystem path.
- Sync conflict inspection and resolution must go through the agent, using structured requests.
- Sync payloads remain encrypted objects; no plaintext provider data may be written into sync targets.

## Change checklist

- Does this change keep the frontend as a pure UI surface?
- Does final read/write authority stay in Rust?
- Are IPC messages typed, authenticated, and narrow?
- Are sensitive fields handled with secret-aware types?
- Does sync still operate on encrypted objects only?
- Did you avoid introducing a second code path that bypasses `aipass-agent`?
