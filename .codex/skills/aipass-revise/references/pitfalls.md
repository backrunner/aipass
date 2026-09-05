# AIPass Pitfalls Registry

Lessons sedimented from real bug fixes. Read the entry for an area before modifying it.
Newest entries last within each section.

## Sync lifecycle (aipass-agent server / session / sync_watch)

### Sync downloads left the in-memory vault stale
- **Symptom**: entries synced from another device disappeared or were overwritten by older content on the next write.
- **Root cause**: `run_sync_local` / `run_sync_webdav` wrote downloaded objects into the vault directory on disk, but an unlocked session kept serving the `Vault` loaded at open time; the next write used stale lamport clocks and content and clobbered the new files. Original defect at `crates/aipass-agent/src/server.rs` `run_sync_local` (no reload after sync).
- **Fix**: `Vault::reload_from_disk()` in `crates/aipass-vault`, called after any sync with `downloaded > 0` while the session is unlocked, followed by a proxy snapshot rebuild.
- **Guardrail**: any code path that mutates vault files on disk behind the session's back must reload the in-memory vault before further reads/writes. Enforced by `sync_download_reloads_the_unlocked_vault_and_keeps_the_proxy_serving` in `server.rs` tests.
- **Watch points**: conflict-accept flows, vault import, any future sync backend — all must go through the same reload path.

### Agent readiness ignored startup sync
- **Symptom**: on macOS, where iCloud sync is the default, the app became ready and rendered entries before the first sync had run, racing the download of remote objects.
- **Root cause**: ready meant "first `SessionStatus` response" (`client.rs`), and no sync ran at startup at all — sync was manual-only. Default sync mode was `Local` on every platform.
- **Fix**: `initial_sync_pending` flag on `SessionStatus`; agent runs a bounded initial sync thread at startup; client keeps polling while pending (deadline relaxed, capped); macOS defaults to `SyncMode::ICloud` when no settings file exists; desktop only persists sync settings the user explicitly changed.
- **Guardrail**: never gate readiness by blocking the IPC listener — extend `SessionStatus` and let the client poll. Startup gates must always terminate (failure marks the gate done, never blocks ready).
- **Watch points**: `crates/aipass-agent/src/client.rs` ready loop, `handlers.rs` SessionStatus response, `session.rs` `load_sync_settings`, `apps/desktop/src/App.svelte` sync-settings save path.

### No remote-change awareness for folder sync
- **Symptom**: iCloud Drive changes made on another device only appeared after a manual "sync now".
- **Root cause**: no file watching existed; iCloud Drive materializes files via the OS and the app never noticed.
- **Fix**: `sync_watch.rs` uses `notify` (FSEvents) with a 2s debounce to trigger `run_sync_local` plus the reload/restart follow-ups; watcher restarts when sync settings change and exits on shutdown.
- **Guardrail**: realtime triggers must reuse the exact same post-sync path (vault reload + proxy reconcile) as manual sync — never fork a second "sync completed" flow.
- **Watch points**: WebDAV has no watcher by design; new folder-based backends must opt into `folder_sync_dir` resolution.

## Proxy credential snapshot (proxy_service / handlers)

### New or changed credentials invisible to the running proxy
- **Symptom**: a credential added from the extension, imported, archived/restored, or synced from another device did not work through the local proxy until the proxy was manually restarted.
- **Root cause**: the proxy resolves plaintext credentials into an immutable in-memory `RuntimeConfig` at start/restart (`proxy_service.rs` `runtime_config`); invalidation relied on each handler remembering to call `refresh_proxy_provider_credentials`, and many write paths never did (BrowserSaveDetected, ProviderArchive/Restore, CcSwitchImport, all sync paths, VaultImport).
- **Fix**: `ProxyService::reload_if_running()` plus refresh hooks on every missed path; sync downloads trigger a full reconcile; VaultImport stops the proxy before locking so a stale snapshot cannot outlive the vault it came from.
- **Guardrail**: every vault mutation path must either refresh the proxy snapshot or provably not affect proxy-visible data. When adding a new write path, grep for `refresh_proxy_provider_credentials` / `reload_if_running` call sites and add yours.
- **Watch points**: `crates/aipass-agent/src/handlers.rs` (all provider/secret/sync/import branches), `crates/aipass-agent/src/server.rs` `save_detected_secret`, `crates/aipass-agent/src/session.rs` unlock/lock transitions.

## Public model pricing (aipass-agent pricing)

### Startup-only refresh and encrypted metadata left prices stale
- **Symptom**: a long-running agent never retried a failed price download or refreshed again; downloads while locked left the reported update time stale. Empty upstream tables could replace a good cache with built-in prices.
- **Root cause**: `crates/aipass-agent/src/pricing.rs` `spawn_list_price_refresh` ran once, `refresh_list_prices` saved its timestamp through `with_vault`, and checked for usable rules only after adding built-in fallbacks.
- **Fix**: `pricing/list_prices.rs` schedules daily refreshes and hourly retries, stores public prices and their timestamp atomically, and validates remote rules before adding fallbacks. The pricing config reads the timestamp from the public cache.
- **Guardrail**: keep public catalog refreshes independent of vault unlock and application releases; reject unusable remote data before replacing the last good snapshot. Covered by the refresh schedule, download failure, and locked-vault tests in `pricing/list_prices.rs`.
- **Watch points**: `server.rs` background startup, `pricing.rs` config reads, `handlers.rs` usage summary and timeseries price loading.

## Endpoint inference (three implementations)

### Valid AI endpoints misclassified as custom_http
- **Symptom**: OpenAI-compatible endpoints such as `/api/paas/v4`, `/v1beta`, custom-domain Anthropic relays, and MiniMax `/v1` were saved as `custom_http`, making them non-proxyable.
- **Root cause**: inference lived in three places that had drifted apart: `packages/schemas/src/index.ts` `inferProviderFromEndpoint` (narrow 10-word fallback regex), `apps/extension/src/content/detector.ts` `inferInterfaceFromEndpoint` (wider keyword set, plus a `replicate|cohere|minimax` short-circuit that ran before the OpenAI check), and `crates/aipass-agent/src/server.rs` `infer_interface_from_endpoint`. The schemas fallback was the narrowest and won in save flows.
- **Fix**: shared evidence regexes exported from schemas and consumed by the extension; Rust hand-aligned with a sync comment; minimax registry entry corrected to `openai_compatible`; `custom_http` is now strictly the no-AI-evidence fallback.
- **Guardrail**: endpoint inference changes must land in all three implementations in the same change, with regression tests on both sides. `custom_http` may only be chosen when no AI evidence exists.
- **Watch points**: `packages/schemas/src/index.ts`, `apps/extension/src/content/detector.ts`, `crates/aipass-agent/src/server.rs` `infer_interface_from_endpoint`, `crates/aipass-provider-registry/src/lib.rs` interface lists.

## Update channel resolution (apps/desktop)

### Stored channel preference outlived the build's version family
- **Symptom**: after a beta build updated to a stable release, the app kept polling the beta update feed (and vice versa).
- **Root cause**: every call site used `getStoredUpdateChannel() ?? inferUpdateChannel(version)`, so a manual/stale localStorage choice permanently overrode the channel implied by the running build.
- **Fix**: `resolveUpdateChannel(version)` is the single entry point — version inference wins, a mismatching stored value is cleared; empty version falls back to stored ?? "official".
- **Guardrail**: the update channel follows the build's version family; a stored preference is a hint, never an override across families. All call sites must go through `resolveUpdateChannel`. Enforced by `updates.test.ts`.
- **Watch points**: `apps/desktop/src/App.svelte` update flows, `SettingsPanel.svelte` channel picker.

## Tray vs UI vs agent validation

### Tray could start the proxy with no valid route groups
- **Symptom**: starting the local proxy from the tray succeeded (or failed with a cryptic message) in configurations the UI correctly refused, e.g. zero enabled route groups.
- **Root cause**: `TraySnapshot::can_start_proxy` only checked agent/vault/proxy run-state, while `ServerDetailPane.svelte` disabled Start on `enabledRoutes.length === 0`, and the agent's `start()` did not reject an empty enabled-route set either — three entry points, three validation levels.
- **Fix**: tray `can_start_proxy` requires an active route; agent `start()` rejects configs with no enabled route group (ValidationFailed); tray surfaces the agent's validation error text instead of a generic failure.
- **Guardrail**: every action with multiple entry points (tray, UI, CLI, extension) must share one server-side validation; UI-level gating is a convenience, never the authority. When changing validation, update all three layers together.
- **Watch points**: `apps/desktop/src-tauri/src/tray.rs`, `apps/desktop/src/lib/components/server/ServerDetailPane.svelte`, `crates/aipass-agent/src/proxy_service.rs` `validate_config`/`start`.
