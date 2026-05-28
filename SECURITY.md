# Security Policy

AIPass stores AI Provider API credentials. Do not include real API keys in public issues, logs, pull requests, screenshots, test fixtures, extension traces, sync directories, or crash reports.

## Reporting

Until a dedicated security address is published, open a private security advisory or contact the maintainers out of band. Please include the affected version, platform, reproduction steps, and whether any real credential may have been exposed.

## Supported Versions

Pre-1.0 builds are experimental. The 1.0 release line defines the first stable support policy.

## Security Invariant

Copying the local vault, sync folder, backups, indexes, native-host files, extension storage, or logs must not be enough to recover AI secrets or sensitive provider configuration.

This invariant covers:

- API keys and tokens.
- Provider titles, domains, endpoints, headers, quota notes, auth scheme, interface type, and free-form notes inside provider records.
- CLI configuration backups created by AIPass.
- iCloud/WebDAV/local sync payloads.
- Browser fill grants and detected-key drafts.

## Implemented Controls

- Master password is processed through Argon2id with stored KDF parameters; new vaults target 64 MiB memory and 2 rounds for responsive unlocks.
- Each vault has a random 256-bit root key. The manifest stores a password-wrapped root key and a recovery-wrapped root key, but never the master password or recovery key.
- Provider records are encrypted as whole record envelopes with XChaCha20-Poly1305.
- The root key wraps the active epoch key and index key. Record plaintext remains protected by per-record DEKs wrapped by the current epoch key.
- Each record gets a random data encryption key wrapped by the current Vault Epoch Key.
- Envelope AAD binds vault id, object id, object type, schema version, crypto version, device id, lamport, and update timestamp.
- API-key search uses HMAC fingerprints and masked display values instead of plaintext secret indexing.
- Multiple API keys for one provider are stored as encrypted secret refs with independent labels and fingerprints.
- Epoch rotation creates fresh key material and rewraps active encrypted objects.
- TTL grants can be cryptographically erased by removing wrapped DEK material.
- CLI config backups are encrypted with vault-derived backup keys.
- Vault export/import files are encrypted with a separate export password and must not contain plaintext provider metadata or API keys.
- The Chrome extension does not persist API keys; ignored-origin preferences are persisted through the native host/agent settings path, and API keys pass through Native Messaging only for save/fill flows.
- Native Messaging validates a native-host extension-id allowlist in addition to Chrome manifest `allowed_origins`; managed deployments can override the allowlist with `AIPASS_ALLOWED_EXTENSION_IDS`.

## Master Password And Recovery

Daily unlock uses only the master password. The emergency recovery key is displayed once during vault creation and once again after a successful recovery reset. Recovery with the old recovery key fails after reset because a new recovery key is generated and the manifest is rewrapped.

Password change, recovery reset, and device revoke all advance the vault epoch and rewrap active encrypted objects. AIPass uses 256-bit symmetric cryptography for practical quantum resistance in this local vault model; it does not use a PQ public-key unlock scheme.

Vault format v2 is pre-release only. Because the application has not shipped, older local vault formats are intentionally not migrated.

## Forward Security Boundary

AIPass supports compromise recovery for future writes after epoch rotation, and cryptographic erasure for TTL-scoped grants whose wrapped key material is deleted. This is the practical boundary for local encrypted files.

AIPass cannot make an attacker forget data they already copied if they had both old ciphertext and the old key material at the same time. After suspected exposure, rotate the vault epoch, revoke affected devices, rotate provider API keys at the provider, and resync.

## Sync

iCloud/WebDAV/local-folder sync transfers encrypted object families only. The sync server is not a trust boundary for secrecy. It may observe object ids, object type names, lamport/update metadata, byte sizes, and conflict files, but it must not observe provider content or API secrets.

WebDAV ETag and file timestamps are used for concurrency control only. They are not authentication or encryption boundaries.

## CLI And Tool Configs

The default CLI configuration writers avoid writing plaintext API keys into third-party tool configuration. They write helper references, env-key names, base URLs when needed, and encrypted backups for rollback.

Commands that intentionally reveal secrets, such as `get --reveal`, `copy`, `env`, `exec`, and `probe`, should be treated as high-sensitivity operations by the caller. Shell history, terminal scrollback, process environments, outbound provider probes, and child processes are outside the vault boundary once a secret is intentionally revealed.

## Verification Gates

Before a 1.0 release candidate, run:

```bash
pnpm typecheck
pnpm test
pnpm build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
```

The Rust test suite includes stolen-vault scans, tamper failures, epoch-ratchet checks, TTL-erasure checks, recovery-key reset/leak checks, compromise recovery checks, multi-secret checks, encrypted export/import checks, encrypted config backup checks, WebDAV sync visibility checks, and Native Messaging grant tests.
