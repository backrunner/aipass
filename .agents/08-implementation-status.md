# AIPass 1.0 Implementation Status

更新时间：2026-05-28

本文记录当前仓库中已经落地并通过验证的 1.0 能力。需求细分仍以 [06-roadmap.md](./06-roadmap.md) 为准；本文件用于对照实现状态、验收命令和剩余发布风险。

## 总体状态

| Area | Status | Evidence |
|---|---|---|
| Monorepo | Implemented | `pnpm-workspace.yaml`, `turbo.json`, Rust workspace |
| E2EE vault core | Implemented | `aipass-crypto`, `aipass-vault`, `cargo test --workspace` |
| Provider registry | Implemented | 官方、第三方、自托管、自定义分类统一于 Rust/TS registry |
| Desktop | Implemented | Tauri + Svelte UI, CRUD/search/provider/env/tag/recent/quota filters/multi-secret/copy/reveal/snippets/probe/export/import/settings/sync/device revoke/native-host repair |
| CLI | Implemented | add/update/list/search/get/copy/secret/probe/env/exec/inject/configure/rollback/sync/native-host/doctor/vault commands |
| Chrome extension | Implemented | MV3 popup, dynamic content scan, Native Messaging, fill grant, save detected key, ignored origins, least-privilege host permissions |
| Sync | Implemented | local/iCloud folder sync and WebDAV sync for encrypted object families |
| Security tests | Implemented | stolen vault, tamper, TTL erasure, epoch ratchet, compromise recovery, sync visibility |
| Release automation | Implemented | CI runs build gates; release workflow builds desktop, CLI/native-host, and Chrome extension artifacts |
| Docs | Implemented | README, SECURITY, `.agents` research/requirements/design/architecture/roadmap/status |

## 1.0 Release Gate Mapping

| Gate | Status | Verification |
|---|---|---|
| `fake_key_leak_scan` / stolen vault scan | Passed | `stolen_vault_scan_does_not_find_provider_plaintext`, CLI smoke grep |
| Tamper test | Passed | `tamper_test_fails_decrypt` |
| Epoch ratchet test | Passed | `epoch_ratchet_blocks_old_key_from_new_data`, `compromise_recovery_test_old_epoch_cannot_decrypt_new_writes` |
| TTL erasure test | Passed | `ttl_erasure_test_keeps_active_record_but_erases_grant` |
| Desktop path | Passed | `pnpm typecheck`, `pnpm build`; UI includes CRUD/search/multi-secret/probe/export/import/settings/sync |
| CLI path | Passed | CLI smoke: init/add/update/search/secret/export/import/probe/archive/restore/configure/sync/leak-scan/delete |
| Extension path | Passed | `pnpm --filter @aipass/extension test`, `pnpm build` |
| Sync path | Passed | local sync tests, WebDAV tests, CLI sync smoke |
| Default tool config avoids plaintext keys | Passed | config-writer tests and CLI smoke |
| Apache-2.0 | Passed | `LICENSE`, package metadata |

## Implemented Desktop Details

- Vault create, unlock, lock.
- Three-pane 1Password-like workbench.
- Provider add/edit with domain inference, favicon URL, multiple domains, API endpoints, console URLs, interface, auth, default model, model aliases, headers, quota, tags, environment, and notes.
- Search by title, provider id, domain, endpoint, console URL, model/default model aliases, quota fields, environment, tag, header name, masked secret, fingerprint, and full API key through HMAC fingerprint matching.
- Filters for Recent, Official, Third-party, Self-hosted, Custom, environment, tag, low quota, and expiring entries.
- Multi-secret management for primary/fallback/admin/read-only style keys.
- Copy/reveal with reveal timeout and best-effort clipboard cleanup.
- Provider probe from the detail pane for OpenAI-compatible, Anthropic, Gemini, and Azure OpenAI interfaces.
- Archive, restore, permanent delete.
- Settings drawer with auto-lock, clipboard clear seconds, master password change, epoch rotation, encrypted vault export/import, sync, device list/revoke, safe tool config modes, and native-host status/repair.
- Local/iCloud folder sync and WebDAV sync UI.

## Implemented CLI Details

- Vault lifecycle: `init`, `login`, `lock`, `vault status`, `vault rotate`, `vault change-password`, `vault devices`, `vault revoke-device`.
- Provider lifecycle: `add`, `update`, `list`, `search`, `get`, `copy`, `archive`, `restore`, `delete`.
- Secret lifecycle: `secret list`, `secret add`, `secret remove`, and `get/copy --field secret:<label>`.
- Provider probe: `probe <entry-id>`.
- Encrypted backup/migration: `vault export`, `vault import`.
- Runtime usage: `env`, `exec`, `inject`.
- Tool integration: `configure codex`, `configure claude-code`, `configure gemini-cli`, `configure opencode`, `rollback`.
- Sync: `sync --dir`, `sync --webdav-url`.
- Native host: `native-host manifest`, `native-host install`, persistent extension-id allowlist.
- Shell completions: `completions`.

## Implemented Extension Details

- Native host request wrapper includes `chrome.runtime.id`.
- Native host supports extension-id allowlist validation from persistent settings or managed env override.
- Popup supports ping, lookup, no-match search, fill, save detected key, refresh, and ignore site.
- Content detector supports first-class providers and self-hosted New API, One API, LiteLLM, sub2api hints, including representative key-page fixtures from those open-source dashboards.
- Open-source source audit covered New API, One API, LiteLLM, and sub2api key creation/copy flows; official and closed hosted consoles are covered through provider registry domains, host permissions, public key-page conventions, and detector fixtures rather than source review.
- Replicate is registered as a third-party provider with account token-page detection and `REPLICATE_API_TOKEN` env/snippet support.
- Ignored origins are persisted through the native host/agent settings path; API keys are not persisted in extension storage.
- Save detected key uses Native Messaging and vault add flow; repeated saves for the same platform/origin create separate lookup entries, queued browser drafts preserve multiple newly detected keys, and desktop/CLI also support multiple secrets on one provider record.
- Popup is built with Svelte + SCSS through Vite/Sass.

## Security Notes

- Active provider records do not auto-expire; user data loss is avoided by default.
- TTL cryptographic erasure is used for grants and temporary access artifacts.
- Epoch rotation protects future writes after compromise; it does not revoke old ciphertext already copied together with old key material.
- Native Messaging deployments should set Chrome `allowed_origins` and the native-host extension-id allowlist; managed environments may use `AIPASS_ALLOWED_EXTENSION_IDS` as an override.

## Verification Commands Run

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
pnpm typecheck
pnpm test
pnpm build
pnpm --filter @aipass/extension build
pnpm --filter @aipass/desktop tauri --version
pnpm --filter @aipass/desktop tauri build --no-bundle --ci
```

Additional CLI smoke covered:

- temporary vault init
- Anthropic provider add/update
- full secret search without JSON leakage
- multi-secret add/list/reveal/remove
- encrypted vault export/import and export leak scan
- provider probe missing-endpoint guard
- archive/restore/delete
- Claude Code and Codex configuration planning/apply
- epoch rotation
- local sync
- vault/sync plaintext grep for fake secret, title, endpoint, and note

## Remaining Release Engineering Work

The repository now has CI and release workflow coverage for the automatable release artifacts:

- Required CI runs Rust fmt/clippy/test/build and Node license audit/lint/typecheck/test/build.
- The `Release` workflow builds desktop bundles on macOS/Linux/Windows.
- The `Release` workflow packages standalone CLI and native-host binaries on macOS/Linux/Windows.
- The `Release` workflow produces `aipass-chrome-extension.zip` for Chrome Web Store submission.

Credential-bound release tasks still require production account setup outside the repository:

- Provide macOS signing/notarization secrets and verify signed install/upgrade/uninstall.
- Provide Windows signing certificate secrets and verify signed install/upgrade/uninstall.
- Publish with the final Chrome Web Store extension id and install/update the native-host extension allowlist.
- Run final signed installer smoke tests for native-host repair on macOS and Windows.
