# Native account switching

Quick integration on an account/API detail selects Codex ChatGPT accounts and
Claude Code subscriptions with `official` mode, or an exact API secret ID. The
Rust Agent owns the operation; Tauri and `aipass tool` use the same protocol.
Preview returns a credential-free, keyed configuration binding. Confirmation
rejects a changed account, configuration, native store or binding.

Before switching, the Agent snapshots configuration, native credential stores,
Claude account metadata, local binding and originally absent resources in an
AEAD-encrypted transaction. A backup failure blocks all writes. An unfinished
transaction is recovered after unlock; external modifications stop recovery.
The local journal is outside synced provider records. Vendor homes retain their
normal CLI credential format, protected by file permissions or the same secure
backend as the outgoing store; no new plaintext backup files are created.

Switching away archives the latest CLI-written grant and updates the same entry
and secret IDs to that private vendor home. Switching in points the account at
the active native store and invalidates proxy generation/cache. Internal Codex
account operations and Claude quota renewal use the same account locks; active external CLI sessions can
continue but must be restarted. A later external configuration or account write
is reported as a conflict and is never silently overwritten.

Codex supports `CODEX_HOME`, file and direct OS Keychain `keyring`/`auto` storage
on macOS. Claude supports `CLAUDE_CONFIG_DIR`, directory-specific macOS Keychain,
credential-file fallback and account metadata. An inaccessible Keychain blocks
switching without falling back to a file. Codex ephemeral/encrypted-secrets
backends and non-macOS OS credential stores fail closed as unsupported. File
stores remain available on other platforms; native validation here is macOS.
The storage contracts follow [Codex authentication](https://developers.openai.com/codex/auth/)
and [Claude Code authentication](https://code.claude.com/docs/en/authentication).

Native Codex transactions retain the existing custom provider ID and its unrelated
settings. They do not rewrite JSONL/SQLite histories that a running CLI may own.
Built-in provider IDs cannot override API routing, so they switch to the custom
AIPass provider; original session files are retained under their original provider.
Full configuration and diff previews redact structured values before diffing,
including multiline TOML tokens and nested authentication headers.

The Agent checks Codex through bounded official `account/read` RPC with refresh
and checks Claude through `auth status`, renewing near-expiry grants through its
usage operation. Model catalog or quota failures do not cause Codex sign-in.
On authentication failure the current active credentials remain in place. The
user can start an isolated official login; completion must match the selected
account and Codex workspace, retains entry/secret IDs, checks configuration
again, and continues the switch. Cancel, timeout, vault lock or identity mismatch
stops the continuation. Browser authorization remains user-operated.

Restore uses the latest archived managed account and validates/renews it, rather
than replaying historical refresh tokens. Restoring a never-imported original
login verifies fresh historical grants in a private CLI home using the original
storage backend. Expired/unidentifiable grants are refused and the original
account needs reconnecting; current credentials remain in place. No
shell initialization files are edited. Inherited endpoint/auth environment
variables are reported by name only. A terminal's separately inherited variables
and project/managed settings may still take precedence over the user-level file.

Regression tests cover both account A → account B → API → account A sequences,
latest grants, exact API IDs, encrypted backup contents, backup failures, partial
write rollback, interruption recovery, external writes and duplicate confirmation.
UI tests cover bound confirmation, sign-in-required outcomes and cancellation of
late login starts. Real vendor authorization/refresh remains a separate live
validation step; fixture identities are not proof of real subscription access.

Review regressions also cover canonical archive paths, account renewal locks during
recovery, per-resource external writes during restore, latest API keys on restore,
stale restore IDs, delayed reauthentication previews and cancelled login polling.
Status snapshots and renewal now share the switch coordinator so an overlapping
query cannot combine an old binding with a completed switch. Preview redaction
also covers entire Codex auth-command objects, API tokens and custom AIPass
credential environment assignments.

## Local verification (2026-10-11 review)

- macOS Rust workspace: format, Clippy with warnings denied, build and 867 passing
  tests; four opt-in tests remain ignored in the default workspace run.
- The isolated macOS Keychain transaction test was explicitly run and passed,
  using a unique temporary service and fake grant. It verified encrypted backup,
  restore and absence, then removed the test item. Permission rejection and partial
  write failures are injected regressions, not proof of a real Keychain denial UI.
- Node workspace lint, typecheck, tests and build passed. Desktop: 39 test files,
  310 tests. The repository ownership/source-size gate also passed.
- Both tools have Rust A → B → API → A grant/identity regressions. A separate
  fixture CLI/Agent Codex sequence returned four `applied` outcomes; its official
  CLI RPC was simulated. Real installed CLI capability probes passed separately.
- An isolated Tauri app passed startup, frontend/Agent readiness and the 960×640
  window gate. Browser component views checked English/Chinese, light/dark,
  confirmation and login dialogs at 960×640. Native screenshot attachment was
  unavailable (`cgWindowNotFound`), so browser views are not native visual proof.
- This review also passed the desktop Tauri Rust check and refreshed browser
  component views at 960×640 in both languages/themes, including cancelled login
  feedback, readonly subscription identity and expanded authentication diagnostics.
  No horizontal overflow was found; the temporary review page/server was removed.
- Real vendor account authorization, renewal and live account switching have not
  been exercised. No DMG/updater publication gate or remote CI is claimed.
- Temporary fixture credentials, Vault, vendor homes, Agent autostart registration,
  UI page and processes were removed. Curated local evidence is retained under
  `runtime-check-reports/native-switching/` (ignored by Git).
