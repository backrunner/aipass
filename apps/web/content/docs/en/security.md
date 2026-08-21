---
title: Security architecture
description: How AIPass encrypts the vault — Argon2id, XChaCha20-Poly1305, epoch rotation, recovery, and export.
navTitle: Security
order: 7
---

# Security architecture

AIPass is local-first: the vault is a directory of encrypted files on your machine (`~/Library/Application Support/dev.aipass.desktop/vault` on macOS), and no plaintext credential is ever written to disk, synced, or logged.

## Key hierarchy

```
master password ──Argon2id──> master key ──┐
                                           ├─> wraps ──> vault root key ──> vault epoch key ──> per-record data keys
recovery key ───HKDF-SHA256──> wrap key ───┘
```

- **Master key** — derived from your master password with Argon2id (version 0x13). New vaults use 64 MiB memory, 2 iterations, parallelism 1, and a random 128-bit salt. The KDF parameters are stored in the vault manifest, so they can be strengthened later without breaking old vaults.
- **Vault root key** — a random 256-bit key generated at vault creation. It is stored twice: wrapped by the master key and wrapped by the recovery key. Changing the master password re-wraps the root key; the root key itself does not change.
- **Vault epoch key** — a random 256-bit key that wraps every record's data key. It can be rotated without changing your password.
- **Per-record data keys** — each provider entry, grant, and device record has its own random 256-bit data key, wrapped by the current epoch key.

## Encryption at rest

Every vault object — provider entry, grant, device record — is a single encrypted envelope:

- XChaCha20-Poly1305 authenticated encryption, 256-bit keys, random 192-bit nonce per write.
- The record id is bound as associated data, so envelopes cannot be swapped between records.
- Title, domains, endpoints, auth scheme, notes, and API keys are all inside the envelope. The only plaintext metadata on disk is the vault manifest (KDF parameters, wrapped root keys, current epoch) and object ids.

API-key search works through HMAC-SHA256 fingerprints (truncated to 96 bits, base64) stored inside the envelope — the key itself never appears in any index. On screen and in CLI output, secrets are masked (`sk-ant…xyz9` style) unless explicitly revealed.

## Recovery key

At vault creation you get exactly one recovery key: `AIPASS-` followed by 32 bytes of uppercase hex in dash-separated groups of four. It derives a wrap key via HKDF-SHA256 and unwraps the root key independently of your password. Entry is forgiving — case, dashes, and whitespace are normalized.

Recovery is one-time: using it sets a new master password, generates a **new** recovery key (shown once), and rotates the vault epoch. Keep the current recovery key somewhere offline; AIPass cannot show it again.

## Epoch rotation and devices

Rotating the epoch (`aipass vault rotate`, or **Settings → Rotate keys**) ratchets the epoch key forward with HKDF-SHA256 keyed by fresh randomness and re-wraps every record's data key. Old epoch keys cannot decrypt records written after rotation — this is forward secrecy for the vault.

Each machine that opens the vault is registered as an encrypted device record. Revoking a device (`aipass vault revoke-device <id>`) marks it untrusted **and** rotates the epoch, so a device that lost sync access cannot use stale key material.

## Browser fill grants

Browser fills never reuse a stored key directly. The agent issues a grant — a separate encrypted envelope holding a copy of the key, bound to an origin, with a 120-second expiry. When a grant expires, its wrapped data key is stripped and the file is tombstoned: cryptographic erasure, not just a flag. A consumed or expired grant cannot be replayed. See the [extension guide](/docs/extension) for the flow.

## Locking

Locking drops decrypted key material from the agent's memory. The vault locks on idle timeout (default 60 minutes), system sleep, screen lock, app quit, or agent restart — each configurable where applicable. After locking, every client (desktop, CLI, extension) must re-unlock with the master password.

## Encrypted export and import

`aipass vault export` produces an `aipass-encrypted-vault-export` (version 1) file: the vault's files are packed and encrypted under a **separate export password** with fresh Argon2id parameters — the export is not protected by your master password, so choose a strong export password. `aipass vault import` restores it into a directory that does not already contain a vault, and locks the session afterwards.

The same encrypted-backup approach protects tool configurations: `aipass configure` snapshots previous config files into `.aipbackup` files encrypted with a vault-derived key and bound to the operation id, and `aipass rollback` restores them.

## Sync

Sync replicates only three encrypted object families — `objects/*.aipobj`, `grants/*.aipgrant`, `devices/*.aipdevice` — to a local folder, iCloud Drive, OneDrive, or a WebDAV endpoint. Objects carry Lamport timestamps for ordering; when the same object differs on both sides, the losing version is quarantined and you resolve it (accept or discard) in the desktop app's sync settings. The sync target never holds plaintext: the pipeline is tested to confirm no plaintext secrets land on the remote.
