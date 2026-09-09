# AIPass Pitfalls Registry

Lessons sedimented from real bug fixes. Read the entry for an area before modifying it.
Newest entries last within each section.

## Sync lifecycle (aipass-agent server / session / sync_watch)

### Agent integration fixtures inherited the host cloud directory
- **Symptom**: native-host tests on macOS stalled in initial sync while reading the host's iCloud files.
- **Root cause**: `crates/aipass-native-host/src/lib.rs` `RunningAgent::start` created a vault at the temporary root and omitted sync settings; sibling agent settings were shared and macOS selected its real iCloud default.
- **Fix**: place each fixture vault under its own temporary root and persist a local sync folder before starting the agent.
- **Guardrail**: every agent integration fixture must isolate the vault, sibling settings and sync destination before startup; never rely on the host's platform defaults. Keep native-host and agent server fixtures aligned.
- **Watch points**: native-host `RunningAgent`, agent server `RunningAgent`, `session::sync_settings_path` and `load_sync_settings`.

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

### Agent startup paid fixed polling delays
- **Symptom**: first requests after an update were delayed by seconds even after the agent had bound its socket.
- **Root cause**: the nonblocking listener slept 200ms after every empty accept, and supervisors checked child exit only every 10 seconds; desktop setup also risked synchronously waiting on agent repair.
- **Fix**: wake the listener with a Unix socket poll timeout, reduce supervisor checks to 1 second, and keep desktop agent warmup asynchronous.
- **Guardrail**: do not add fixed waits to the agent readiness path; keep launch repair and desktop setup off the Tauri setup critical path.
- **Watch points**: `crates/aipass-agent/src/server.rs`, `src/ipc.rs`, `src/autostart.rs`, and `apps/desktop/src-tauri/src/lib.rs`.

### No remote-change awareness for folder sync
- **Symptom**: iCloud Drive changes made on another device only appeared after a manual "sync now".
- **Root cause**: no file watching existed; iCloud Drive materializes files via the OS and the app never noticed.
- **Fix**: `sync_watch.rs` uses `notify` (FSEvents) with a 350ms debounce and a bounded 2s maximum to trigger `run_sync_local` plus the reload/restart follow-ups; watcher restarts when sync settings change and exits on shutdown.
- **Guardrail**: realtime triggers must reuse the exact same post-sync path (vault reload + proxy reconcile) as manual sync — never fork a second "sync completed" flow.
- **Watch points**: all backends watch local writes; WebDAV polls remotely every 5 seconds with error backoff while transport credentials are available. CloudKit uses native push wake plus 60-second recovery polls. Folder backends also use `folder_sync_dir` and notifications, with polling for missed events and late mounts.

### Updated desktop reused an older resident Agent
- **Symptom**: WS opt-out reverted after saving; Codex preview still failed after installing a fixed desktop build.
- **Root cause**: `crates/aipass-agent/src/client.rs:207` accepted any successful SessionStatus as readiness. A resident started before the update kept protocol v2 while replacement binaries existed at the same paths; older schemas silently discarded `supportsWebsockets`.
- **Fix**: require protocol v3 for WS persistence and configuration-only preview; validate response versions, retire older residents through authenticated legacy shutdown, and wait for their socket to stop before replacement. Never retire a newer resident for an older client.
- **Guardrail**: raise the required Agent protocol when new client behavior depends on semantics that older residents silently ignore; never treat an incompatible success response as readiness or replay mutations with a legacy protocol. Cover authenticated retirement and mismatch readiness in `client::tests`, WS edit/save/reopen in `App.test.ts`, and vault reload in `websocket_opt_out_survives_updates_and_vault_reopen`.
- **Watch points**: desktop/CLI/native-host startup, direct and supervised agents, protocol response decoding, and packaged sidecar builds.

### Record-only sync could not recover a fresh installation
- **Symptom**: first-run iCloud skipped sync without a manifest; downloaded records lacked the wrapped keys needed to open a vault. WebDAV and local edits did not automatically sync.
- **Root cause**: `server.rs::run_initial_sync` returned early for an absent manifest; `aipass-sync/src/local.rs::SYNC_DIRS` excluded the manifest; `sync_watch.rs::restart_sync_watcher` watched only an existing remote folder.
- **Fix**: authenticated whole-vault snapshots with immutable parent histories, local three-way merge, crash replay, durable outbox, explicit first-run import and a create-time cloud recheck. See `docs/vault-sync.md` and `vault_sync::tests`.
- **Guardrail**: transfer wrapped keys and records in one authenticated unit; never overwrite an existing locked vault with unauthenticated downloads. Validate snapshots before merge, keep divergent edits recoverable, quarantine tampered inputs, and test restore on an absent vault. Journal legacy migration, preserve the incoming branch through the conflict API (records live in nested directories), and keep unresolved legacy conflicts visible across polls. Do not hold the session lock across network work or refresh session activity from background polling.
- **Watch points**: initial sync, create/unlock/recovery, file and WebDAV imports, reset caches, conflict resolution, proxy reconciliation, `SessionStatus`/protocol v5, and frontend revision refresh. Native fixtures must persist isolated sync settings before calling `session::create_vault`.


### CloudKit identity and sync refresh must stay outside session lifecycle
- **Symptom**: raw Agent sidecars cannot use app-scoped CloudKit identity; unconditional proxy refresh cancelled unrelated streams; IO failures locked the vault and stopped its running proxy.
- **Root cause**: CloudKit requires a provisioned signed application process; `ProxyHandle::update_config` broadcast every refresh globally; `vault_sync::run` coupled pending-journal recovery to session/proxy shutdown.
- **Fix**: signed native desktop ciphertext worker over authenticated protocol v5; profile/schema release checks; native timeout and replay-safe task IDs; route generations and unchanged-config no-op; rollback journals with vault-access gating only when rollback also fails. Lock stages established vault changes before key disposal, with a separate zeroizing WebDAV transport credential cache.
- **Guardrail**: keep CloudKit off raw sidecars and the WebView; embed and verify the matching production profile. Do not advance an unauthenticated imported checkpoint by pre-queuing it. Test wrong/late native completions, account changes, immediate edit/lock/upload, failed apply rollback, and unrelated WS continuity alongside actual credential revocation. Preserve activity timestamps and listening sockets on normal sync/network errors. Test the Swift FFI entitlement guard through the macOS desktop Rust test harness; it works with Command Line Tools without XCTest.
- **Watch points**: `cloudkit.rs` in Agent and desktop, `CloudKitTransport.swift`, signing scripts/release workflow, initial/create/import flows, `sync_watch.rs`, `session.rs`, proxy HTTP usage streams, native/adapted WS and idle upstream pools. See `docs/cloudkit-release.md` and `docs/vault-sync.md`.

### Applied snapshots must refresh readers even when checkpoint IO fails
- **Symptom**: sync or conflict acceptance returned an IO error after installing new data, leaving the proxy with stale credentials; conflict failure also stopped the proxy and locked the session.
- **Root cause**: `vault_sync.rs::run_inner` refreshed only on an overall successful report; `resolve` refreshed after fallible checkpoint writes and retained the old forced lock path.
- **Fix**: recover local journals before network work, track successful apply/bootstrap/journal recovery separately from later failures, refresh readers while preserving session activity and sockets, and join identical authenticated heads after failed checkpoint persistence.
- **Guardrail**: refresh successfully applied data even if later bookkeeping fails, but never read a vault with an unresolved apply journal into the proxy. Test download, conflict acceptance, recovery, updated outbound credentials, unchanged activity and eventual convergence in `applied_snapshots_refresh_proxy_even_when_followup_io_fails`.
- **Watch points**: `vault_sync::run_inner`, `synchronize`, `resolve`, vault journal replay, proxy credential reconciliation.

### Proxy reconciliation must not create synchronized mutations
- **Symptom**: starting or refreshing the proxy rewrote provider last-used timestamps and audit records, creating extra snapshots and conflicts with another device's real edits.
- **Root cause**: `proxy_service.rs::runtime_config_inner` used user-facing `reveal_secret_field` and `reveal_provider_headers`, which write synced records.
- **Fix**: a Rust-only, non-serializable, zeroizing `runtime_provider_credentials` reader supplies the proxy without writes and rejects archived/deleted entries. Runtime target exclusions log only target IDs and error codes.
- **Guardrail**: use pure reads when reconciling runtime state. Assert repeated refreshes preserve the vault revision and do not publish new snapshots; retain audited reveal behavior for explicit user actions.
- **Watch points**: proxy start/reload, all sync transports, `aipass-vault` runtime vs user reveal APIs, `applied_snapshots_refresh_proxy_even_when_followup_io_fails`.

### Late CloudKit completions must retain their account boundary
- **Symptom**: a timed-out native operation could resume after an account change and mutate cursor/subscription state; the Agent accepted a successful write reply without checking its account.
- **Root cause**: Swift actors are reentrant across CloudKit awaits, and task account validation covered dispatch but not the returned acknowledgement.
- **Fix**: synchronously invalidate account generations on notification, check cancellation/generation after awaits, verify the account before committing results, and reject missing or mismatched account acknowledgements in the Agent.
- **Guardrail**: check account and cancellation before every shared cursor/ID/subscription mutation; only remove outbox data after an acknowledgement for the pinned account. Regression: `bridge_rejects_missing_or_changed_account_acknowledgements`.
- **Watch points**: `CloudKitTransport.swift`, Agent `cloudkit::CloudKitBridge::request`, native timeout and notification paths.

### Import settings and configured sync must share the vault transaction boundary
- **Symptom**: a failed import settings write could leave an imported vault using the previous vault's remote; a waiting background sync could resume using settings read before an import or target change.
- **Root cause**: `vault_sync::import_file` and `import_sync` installed sibling settings separately from the vault directory, while `server::run_sync_configured` resolved its destination before taking `sync_lock`.
- **Fix**: stage an encrypted-settings journal inside the incoming vault, prefer it until the sibling write completes, clear transport credentials on installation, and select configured targets under the sync lock. Creation rejects old remote records without recovery keys.
- **Guardrail**: serialize target selection with settings changes/import/reset; install vault identity and its sync settings together. Test failed settings persistence, recovery with stale sibling settings, queued sync target changes, and record-only first installation in `vault_sync::tests`.
- **Watch points**: configured startup/watcher/manual sync, file/backup/WebDAV/CloudKit import, `load_sync_settings`, `save_sync_settings`, and create-time discovery.

### Automatic CloudKit upgrades must commit only after authenticated read-back
- **Symptom**: existing local/WebDAV/folder settings never entered CloudKit; Drive fallback skipped late edits once another device populated CloudKit.
- **Root cause**: settings had no upgrade marker and `vault_sync::run_cloudkit_inner` consulted Drive only when CloudKit was empty. Reusing this fallback for fresh CloudKit restores also misclassified them as legacy installations.
- **Fix**: protocol v8, a persisted one-time migration marker, source backup/reconciliation, destination authentication before uploads, and remote read-back before switching settings. Fresh restore/create and explicit settings changes record their current choice. Background revision refresh reloads settings without overwriting a UI draft.
- **Guardrail**: enable migration for existing local-only vaults, serialize it with settings/import/reset, keep network IO outside the session lock, and verify current local contents after read-back. Preserve original encrypted copies and settings on failure. Test retries after reopen/settings IO failure, concurrent edits, foreign and conflicting vaults, late Drive edits, explicit opt-outs and fresh CloudKit restores in `cloudkit_migration::tests` and `session::tests`.
- **Watch points**: initial/configured/manual CloudKit sync, lock-time queues, unlock watcher, bootstrap-before-create, import settings, protocol retirement and App sync-revision refresh.

### CloudKit deployment scope and normalized grants need explicit handling
- **Symptom**: a Development deployment required Production access; Production validation returned `endpoint not applicable`; a successful import was reported as failed after Apple normalized creator permissions into `GRANT READ, WRITE`.
- **Root cause**: `scripts/deploy-cloudkit-schema.mjs` assumed both environments supported the same deployment endpoints and parsed only one permission per grant.
- **Fix**: local CLI accepts Development only, with Production promotion documented through CloudKit Console; expand combined grants before validating creator access and idempotency. Developer ID profile service wildcards are accepted while the application still requests only the explicit CloudKit container/service.
- **Guardrail**: reject Production/both before tool calls, validate Development before import, and compare a fresh post-import export. Test combined creator/non-creator grants and deployment without Production access. Do not mistake an iCloud service wildcard for authorization of an unlisted container.
- **Watch points**: deployment script/tests, `infra/cloudkit/schema.ckdb`, `docs/cloudkit-release.md`. Keep schema deployment separate from signed-app provisioning.

### Epoch rotation must share the authenticated snapshot transaction
- **Symptom**: a damaged later record or failed write left earlier records encrypted with a lost epoch key; failed password/recovery/revocation operations could partially take effect.
- **Root cause**: `crates/aipass-vault/src/lib.rs:883` rewrote objects before persisting the new manifest, and callers mutated password wrappers/device records before rotation completed.
- **Fix**: prepare the next header, encrypted records, revocation and audit in memory; commit them through the existing authenticated snapshot journal with rollback and unlock replay.
- **Guardrail**: never replace epoch-encrypted records before journaling the matching wrapped key. Test preparation failures, partial IO, manifest failure, interrupted unlock replay and retry after a failed password change (`sync_snapshot::tests`).
- **Watch points**: explicit rotation, password changes, recovery, device revocation, snapshot apply/recovery and read gating.

## Tool configuration writes (aipass-agent / config-writers)

### Local diagnostics stopped at rotation limits and runtime boundaries
- **Symptom**: component logs stopped after reaching the daily size cap; proxy failures disappeared after stopping/restarting; failed probes and sync could look successful at the IPC layer.
- **Root cause**: `logging.rs` returned on overflow, `proxy::RuntimeStats` retained only in-memory errors, and request logging relied on transport success and serialized requests to discover their event names.
- **Fix**: shared component writer with size rotation and process locking; static exhaustive request event names, UUID correlation and semantic outcomes; bounded persistent proxy diagnostics linked to upstream attempts. See `docs/local-logging.md`.
- **Guardrail**: never serialize requests for diagnostics; log only allowlisted metadata, retain operation pairs across repetition/rotation, correlate attempts across HTTP/SSE/WebSocket, and test persistence after stop/restart and business failures within successful IPC responses.
- **Watch points**: agent `operation_log.rs`, shared `logging.rs`, client/envelope/server correlation, desktop logger, proxy `diagnostics.rs` and `persist_attempt`. Regression tests cover provider lifecycle, semantic errors/unwinding, concurrent rotation, restart retention and successful fallback.

### Live log viewers must scope refresh and scrolling to an opening
- **Symptom**: the proxy log dialog opened at the oldest entry and stayed on a one-time snapshot.
- **Root cause**: `ServerDetailPane` loaded logs before opening the dialog; the portalled log body had no mount/update scroll handling or refresh lifecycle.
- **Fix**: `ProxyLogsDialog` loads immediately and schedules the next refresh two seconds after completion, follows the bottom unless the user scrolls up, and invalidates pending work on close/unmount. Virtualize and highlight only visible rows with measured wrapping heights and content/occurrence keys to preserve reading anchors as history is trimmed. Capture tail-follow intent before layout adjustments; show loading during imports and requests, retaining the last good snapshot on transient failures and skipping unchanged DOM updates.
- **Guardrail**: bind scrolling to the actual mounted log body, prevent overlapping refreshes, and reject responses from previous openings. Verify bounded rendering/highlighting, duplicate entries, trimmed history, loading, opening/reopening, manual scrolling, failures, close and unmount in `ProxyLogsDialog.test.ts`; check wrapped rows and tail following at 960×640.
- **Watch points**: dialog portal mounting, `ServerDetailPane` lazy loading and `onLoadProxyLogs`, typed `server_logs` IPC.

### Unsupported Responses tools must not disappear during conversion
- **Symptom**: Responses-to-Anthropic conversion returned success after removing `custom` execution tools and their call history.
- **Root cause**: `request.rs` filtered tools to `function` and ignored unsupported call/result items.
- **Fix**: reject unsupported definitions and call/result history with an explicit conversion error, preserving native Responses passthrough. Regression: `responses_conversion_rejects_unsupported_tools_and_history_without_silently_dropping_them`.
- **Guardrail**: implement complete reversible tool mappings or reject the request; never silently drop executable capabilities or invocation history.
- **Watch points**: `crates/aipass-proxy-conversion/src/request.rs`, native Responses routing, and converted WS requests.

### Protocol failures need comparable metadata across transports
- **Symptom**: Codex reported missing terminal tools while proxy status alone could not distinguish dropped tool definitions, malformed Responses events, and WS transport failures.
- **Root cause**: request diagnostics omitted tool shape and event lifecycle; WS connector errors were reduced to HTTP status, and mixed WS requests logged a UUID before replacing it for usage accounting.
- **Fix**: shared allowlisted tool/event summaries in `crates/aipass-proxy/src/diagnostics/protocol.rs`, fixed transport reason categories and OS codes, and one generated request ID across mixed WS attempts and usage records.
- **Guardrail**: observe native WS and HTTP/SSE with the same bounded counters; compare tool summaries across conversion without logging names or payloads; assign correlation IDs before logging. Keep diagnostics observational. Covered by `diagnostics::protocol::tests` and `websocket::tests::diagnostics`.
- **Watch points**: native `websocket.rs`, mixed `websocket/upstream.rs`, bridge context restoration, HTTP `forward_request` / `observe_sse_usage_traced`, and `docs/local-logging.md`.

### Configuration failures lacked an operation trail
- **Symptom**: a failed config write surfaced only a transport error such as `failed to fill whole buffer`, with no way to identify which write was interrupted.
- **Root cause**: tool config preview/apply/rollback handlers had no lifecycle logs, while writes can wait on vault or Codex SQLite state migration.
- **Fix**: handlers now log start, completion, rejection, and write/rollback failures with operation or request IDs, target metadata, and elapsed time; config requests use the long response timeout.
- **Guardrail**: log every configuration operation's lifecycle and correlate apply/rollback failures by operation ID without logging config contents or secrets.
- **Watch points**: `crates/aipass-agent/src/handlers.rs`, `crates/aipass-agent-protocol/src/lib.rs`, and `crates/aipass-config-writers/src/backup.rs`.

### Codex session migration overflowed the IPC response
- **Symptom**: changing a Codex provider failed with `failed to fill whole buffer`, while other tool configuration writes succeeded.
- **Root cause**: Codex provider migration scanned session JSONL files and put each transformed file into `ConfigPlan.extra_writes`; large histories made the preview response exceed the 16 MiB agent frame limit or exhausted the agent before it could answer.
- **Fix**: preview and apply use separate planning entry points. Preview returns configuration diffs without discovering or reading JSONL/SQLite history; apply retains migration paths/counts and performs per-file backup plus streaming rewrite in Rust.
- **Guardrail**: never scan session/history files during preview, including direct, official and local-proxy modes. Never place history contents in an IPC frame. Keep apply-time migration and encrypted backups; test preview against unreadable history (`codex_preview_never_reads_session_history_in_any_auth_mode`).
- **Watch points**: `crates/aipass-config-writers/src/plan.rs`, `crates/aipass-config-writers/src/backup.rs`, and `crates/aipass-agent/src/handlers.rs`.

### Compact configuration diffs are not literal source
- **Symptom**: preview marked unchanged settings as replacements, lost JSON indentation in full-file view, and could not show real file line numbers.
- **Root cause**: `crates/aipass-config-writers/src/utils.rs:155` emits a compact replacement block with omitted prefix lines; `apps/desktop/src/lib/utils/highlight.ts:25` interpreted source indentation as diff markers even in full-file mode.
- **Fix**: retain hunk coordinates in the agent diff, align unchanged lines in the desktop viewer, and highlight literal source separately from diff parsing.
- **Guardrail**: never derive file line numbers from snippet indexes or feed literal content back through diff-prefix stripping. Preserve hunk metadata during redaction; test per-file counts and full-file indentation.
- **Watch points**: `utils::diff_tests`, `config-diff.test.ts`, `highlight.test.ts`, and `IntegrationPreviewDialog.test.ts`; direct and local-proxy integrations share the viewer.

### Codex provider writes omitted WebSocket support
- **Symptom**: generated Codex configurations did not advertise Responses WebSocket support, including when pointing at the WS-capable local proxy.
- **Root cause**: `crates/aipass-config-writers/src/plan.rs:930` managed `wire_api` but omitted the provider-level `supports_websockets` flag.
- **Fix**: default `supports_websockets` to true in the shared provider updater for every auth mode; honor an explicit provider opt-out on direct integrations while local proxy integrations always advertise their own WS capability.
- **Guardrail**: keep transport flags in the shared Codex provider updater; verify new configs, provider migration, direct opt-outs and local proxy WS support while keeping HTTP base URLs. Covered by the Codex writer idempotence/migration tests and `codex_local_proxy_writer_enables_websocket_transport`.
- **Watch points**: `plan_codex`, `plan_codex_official`, `plan_codex_plaintext_with_mode`, and agent `build_tool_config_proxy_plan`.

### Configuration confirmation must retain its preview context
- **Symptom**: a delayed preview for one provider opened after selecting another; confirmation applied the current provider or Codex mode instead of the displayed selection.
- **Root cause**: `IntegrationCard.svelte:64` reset visible state without invalidating requests, while parent apply callbacks rebuilt requests from live selection.
- **Fix**: invalidate request generations on context change/unmount and return a captured apply closure with each preview; show mode-specific credential access text in the dialog and correct website claims.
- **Guardrail**: bind confirmation to the request that produced its preview. Include provider/route identity and write mode in invalidation; ignore late success and failure. Cover provider changes, mode changes and repeated confirmation in integration tests.
- **Watch points**: provider and proxy route integrations, Codex mode selection, direct configuration versus local proxy tokens, and English/Chinese security copy.

## Proxy credential snapshot (proxy_service / handlers)

### Codex local tokens must keep their conversational protocol scope
- **Symptom**: a Codex-configured local proxy token could be reused against another inbound API route such as Chat Completions.
- **Root cause**: local authentication is token based, so protocol scoping depends on the route selection predicate remaining aligned with Codex's `wire_api = "responses"` configuration.
- **Fix**: keep Codex integration validation and runtime route selection tied to `OpenAiResponses`, with an explicit protocol-scope helper and regression test.
- **Guardrail**: preserve one-to-one Codex token and Responses conversation routing; reject Chat Completions and Anthropic paths before forwarding. Per the 2026-09-07 user request, OpenAI route tokens also authorize the two standalone Images endpoints, restricted to their own OpenAI targets; this exception must not broaden conversational protocol access.
- **Watch points**: `crates/aipass-agent/src/server.rs` `ensure_proxy_tool_protocol`, `crates/aipass-proxy/src/lib.rs` route selection, and `apps/desktop/src/lib/utils/integrations.ts`.

### New or changed credentials invisible to the running proxy
- **Symptom**: a credential added from the extension, imported, archived/restored, or synced from another device did not work through the local proxy until the proxy was manually restarted.
- **Root cause**: the proxy resolves plaintext credentials into an immutable in-memory `RuntimeConfig` at start/restart (`proxy_service.rs` `runtime_config`); invalidation relied on each handler remembering to call `refresh_proxy_provider_credentials`, and many write paths never did (BrowserSaveDetected, ProviderArchive/Restore, CcSwitchImport, all sync paths, VaultImport).
- **Fix**: `ProxyService::reload_if_running()` plus refresh hooks on every missed path; sync downloads trigger a full reconcile; VaultImport stops the proxy before locking so a stale snapshot cannot outlive the vault it came from.
- **Guardrail**: every vault mutation path must either refresh the proxy snapshot or provably not affect proxy-visible data. When adding a new write path, grep for `refresh_proxy_provider_credentials` / `reload_if_running` call sites and add yours.
- **Watch points**: `crates/aipass-agent/src/handlers.rs` (all provider/secret/sync/import branches), `crates/aipass-agent/src/server.rs` `save_detected_secret`, `crates/aipass-agent/src/session.rs` unlock/lock transitions.

### Unavailable siblings must not stop live provider reconciliation
- **Symptom**: archiving a referenced provider stopped the whole proxy; a retained unavailable target also prevented another provider's WS capability warning from being persisted.
- **Root cause**: `proxy_service.rs::refresh_provider_credentials` called strict `restart`, and `persist_ws_capabilities` used strict `runtime_config`, while sync refresh already skipped unavailable targets.
- **Fix**: use the existing tolerant live reload for local credential refresh and the same usable-target resolution for capability persistence. Retain stored references so restoring the provider makes it available again.
- **Guardrail**: reconcile local provider changes, synced changes, and capability observations against the same usable target set. Test archive, sibling refresh, warning persistence, and restore without stopping the listener in `inactive_targets_do_not_interrupt_live_refresh_or_capability_persistence`.
- **Watch points**: local archive/restore and provider edits, sync reload, capability persistence, strict validation for explicit proxy setup.

### WebSocket transport must share proxy configuration and invalidation
- **Symptom**: Responses WebSocket clients could not connect to the local proxy; a separate direct WS connector would also bypass configured outbound proxies and leave authenticated sessions alive after credential changes.
- **Root cause**: `crates/aipass-proxy/src/lib.rs` used `serve_connection` without upgrades, and `build_upstream_headers` intentionally removes HTTP hop headers. Runtime refresh originally only replaced the request-time credential snapshot.
- **Fix**: enable Hyper upgrades, negotiate WS through the shared reqwest proxy configuration, and notify upgraded sessions from `ProxyHandle::update_config`. Track usage per response and retry only before the upgrade is committed.
- **Guardrail**: route every new transport through the existing token selection, credential injection and outbound proxy settings; terminate authenticated long-lived sessions when their runtime snapshot changes, including while blocked writing to a slow client. Never replay a committed Responses WS session on another target. When a configuration change cancels bridged HTTP work, recheck invalidation before sending its result and retain the WS restart close reason; a biased outer select alone does not prevent a same-poll error race.
- **Watch points**: `lib.rs` `upstream_client_for_transport` / `update_config`, `websocket.rs` handshake / relay, agent `proxy_service.rs` credential refresh. Regression coverage lives in `websocket/tests.rs` (custom proxy, config reload, and no replay after disconnect).

### Self-hosted routes must honor the selected wire format
- **Symptom**: a self-hosted endpoint that accepts both OpenAI and Anthropic requests was rejected at proxy startup or received an inferred format instead of the route's selected format.
- **Root cause**: `proxy_service.rs` and `RouteGroupDialog.svelte` derived each target's protocol from provider metadata and automatically enabled conversion, even though self-hosted endpoints can support multiple wire formats.
- **Fix**: route configuration now supplies the upstream protocol; target metadata is not used to infer conversion, while explicitly persisted target protocols remain supported.
- **Guardrail**: send every target the route's configured protocol unless a target has an explicit protocol override; preserve that override through editor save round trips. Never probe or infer a self-hosted target's format during proxy startup.
- **Watch points**: `crates/aipass-agent/src/proxy_service.rs` runtime config, `apps/desktop/src/lib/components/server/RouteGroupDialog.svelte`, and route protocol tests.

### Converted WebSocket sessions need request-scoped state
- **Symptom**: an Anthropic-only route previously rejected Responses WS clients even though the existing SSE converter supported the protocol pair.
- **Root cause**: the WS path only accepted native Responses upgrades and did not adapt `response.create` into HTTP/SSE requests or retain the conversation associated with `previous_response_id`.
- **Fix**: `websocket/bridge.rs` adapts each Responses WS request through the existing Responses -> Anthropic request converter and Anthropic -> Responses `StreamConverter`, restores `stream_id`, and keeps response-chain context in memory for the connection lifetime.
- **Guardrail**: converted WS routes must retain complete text/tool context (including empty-argument tool calls), emit argument completion events, release the active lane on every error, and account each response exactly once, ignoring duplicate terminal notifications. Treat `response.incomplete` as a terminal generation result, not an upstream transport failure. Preserve system/developer message roles when converting requests. Evict failed same-lane parents without evicting a failed fork's source lane. Do not treat the initial `response.created` event as successful completion.
- **Watch points**: `websocket/bridge.rs` request preparation, SSE completion/error handling, `SessionUsage::server_event`; coverage in `websocket_conversion_preserves_tool_calls_results_and_forked_context`, `websocket_conversion_orders_lanes_and_recovers_after_upstream_failure`, and `websocket_conversion_warmup_and_error_cache_eviction_are_connection_local`.

### Successful fallback hid failing proxy targets
- **Symptom**: the proxy showed a healthy status after a backup completed the request, and users could not identify the failing service in a route group.
- **Root cause**: `crates/aipass-proxy/src/lib.rs` `ProxyHandle::status` only used final request failures; target circuit health never crossed the status boundary into `RouteListPane` / `RouteGroupDialog`.
- **Fix**: expose enabled target IDs with unresolved failures or an open circuit through `ProxyStatus`; include them in degradation and display badges per group and credential while running.
- **Guardrail**: derive target degradation from shared runtime health, including successful fallback, recovery, expiration, and config reload. Keep target IDs distinct across credentials. Enforced by `degraded_targets_follow_recent_failures_circuits_and_recovery`, `proxy_authenticates_fails_over_and_records_usage`, and desktop route component tests.
- **Watch points**: HTTP/model discovery/stream/WebSocket `mark_failure` and `mark_success`, agent stopped status, desktop status polling, route list and editor.

### Immediate success cleared a recovering target
- **Symptom**: a provider briefly showed as degraded and then returned to healthy after one successful fallback or a late in-flight response.
- **Root cause**: `crates/aipass-proxy/src/lib.rs` `mark_success` removed target health on the first success, even when the target had unresolved failures or had just left its circuit-open period.
- **Fix**: require two distinct, consecutive successful generations started after the latest failure before clearing degradation; keep recent flapping history for ten minutes. Count non-stream completion once, and ignore model discovery and older in-flight successes.
- **Guardrail**: require `RECOVERY_SUCCESS_THRESHOLD` new successful generations before clearing degradation. Do not erase recent failure history, count one response twice, heal from model discovery, or let old completions heal newer failures. Keep HTTP/SSE/native WS/adapted WS aligned; cover stale parallel lanes and recovery thresholds.
- **Watch points**: `mark_failure`, `mark_success`, `circuit_open`, streaming completion, converted WebSocket lanes, and degraded status projection.

### Empty successful bodies must not commit
- **Symptom**: an upstream could return HTTP 200 with an empty body, leaving the model client with an empty response instead of trying another target.
- **Root cause**: non-stream forwarding treated any 2xx response as successful before checking whether a body was present.
- **Fix**: empty buffered success bodies now mark the target failed and enter the normal fallback chain.
- **Guardrail**: before committing a non-stream upstream response, reject empty bodies and structured error payloads as target failures.
- **Watch points**: `crates/aipass-proxy/src/lib.rs` `forward_request`, silent retry buffering, and usage attempt accounting.

### Validate provider payloads before success accounting
- **Symptom**: a malformed 200 response on a converted route returned 502 without fallback and was counted as successful; model discovery also returned empty/error 200 bodies as healthy results.
- **Root cause**: `crates/aipass-proxy/src/lib.rs` `forward_request` started usage tracking before non-stream conversion; `handle_models_request` skipped the error-body guard used by generation.
- **Fix**: validate non-stream conversion before starting success/usage tracking, and reject empty or structured-error model-list bodies before returning them; model discovery never heals generation health.
- **Guardrail**: finish response validation before success accounting or committing a response; on provider response conversion failure, continue fallback and record only the failed attempt. Enforced by `invalid_converted_response_fails_over_without_recording_success` and `model_discovery_empty_and_error_success_bodies_fail_over`.
- **Watch points**: `forward_request`, `handle_models_request`, `track_usage_stream`, and converted WebSocket calls to the shared forwarding path.

### Slow generation is not a confirmed provider failure
- **Symptom**: a slow response header, first SSE event, body chunk or WS generation triggered a hard timeout and sometimes submitted the same generation to another provider.
- **Root cause**: generation waits shared first-byte, idle and hold deadlines; HTTP replay protection was limited to WebSocket callers.
- **Fix**: per the 2026-09-07 user instruction, remove response deadlines from HTTP/SSE/native WS/adapted WS. Bound only connection/handshake establishment, socket writes, and retry backoff. After submission, permit another provider only on an explicit rejected status/error event/error payload or a fully received invalid response; never replay ambiguous write/read failures or truncated responses. Disable reqwest automatic retries. Cancel pending work on downstream close/config invalidation without classifying cancellation as provider failure.
- **Guardrail**: do not use first-byte, idle or hold budgets to terminate a submitted generation. Check hold limits only before another attempt and during backoff/handshake. Verify slow headers/bodies/SSE/WS remain active beyond legacy budgets, confirmed errors still fail over, and ambiguous failures never touch a backup.
- **Watch points**: `forward_request`, `prefetch_sse_event`, `collect_upstream_body`, `track_usage_stream`, `websocket.rs`, `websocket/upstream.rs`, `websocket/bridge.rs`; regressions in `tests/runtime_status.rs` and WebSocket tests.
- **Test guardrail**: allow CI startup/parsing/SQLite work before upstream submission, then wait beyond the configured budgets after the upstream acknowledges receipt. A 30 ms hold budget can expire before submission and falsely fail a slow-response test; race arrival against early request completion for actionable failures. Local development tools can send unauthenticated `GET /` probes to IPv4 and IPv6 test listeners: LocalStack traffic caused a false backup-connection failure on macOS. Ignore only those root probes in HTTP and WebSocket fixtures (including multimodal failover and handshake attempt counters), reject API/authenticated traffic at the backup, and independently verify that no diagnostic references the backup target.

### Live channel telemetry must follow generation lifetimes
- **Symptom**: live concurrency and available channels displayed zero with an old resident Agent; WS connections were counted while idle, recovering targets were subtracted as unavailable, and usage rows moved on every refresh.
- **Root cause**: missing IPC fields silently deserialized to zero, activity was tied to the HTTP upgrade lifetime, availability reused degradation, and the UI reused usage-volume sorting.
- **Fix**: require the current Agent protocol at startup; expose per-target request activity/circuit cooldown from Rust. Count each WS generation, release guards on completion or cancellation, keep recovering channels available, and refresh status independently of usage queries. Order credential rows by first appearance in configured groups, including zero-usage members; aggregate indicators using provider plus secret identity while showing the specific group in tooltips.
- **Guardrail**: test HTTP/native WS/adapted WS activity through slow response, completion and cancellation. Do not infer live activity from historical usage or treat degradation as an open circuit. Keep configured ordering stable across usage refreshes and keep removed historical credentials deterministic.
- **Watch points**: `ProxyStatus`, Agent stopped status, Tauri tray fixtures, App status polling, `UsageBreakdown` / `ChannelIndicator`, protocol readiness replacement.

### Circuit-open weights distorted round robin
- **Symptom**: after a high-weight target opened its circuit, one healthy fallback received its weight while another equally weighted target received no requests.
- **Root cause**: `crates/aipass-proxy/src/lib.rs` `select_route_targets` rotated by all enabled targets' weights before removing circuit-open targets.
- **Fix**: remove unavailable targets before calculating weighted rotation.
- **Guardrail**: calculate round-robin weights only over eligible peers in the best stability tier. All retry modes must respect open circuits and recovery reservations. Covered by `round_robin_redistributes_weight_among_available_targets`.
- **Watch points**: shared target selection used by HTTP, model discovery, native WS, and converted WS.

### Session cache affinity was lost during target selection
- **Symptom**: requests from one conversation alternated between providers, reducing provider prompt-cache hits even while every target was healthy.
- **Root cause**: round-robin selection had no short-lived association between a client session key and the target that completed its previous request; HTTP, model discovery, and WebSocket paths all started from the route-wide rotation.
- **Fix**: keep bounded in-memory affinity keyed by route and `prompt_cache_key`/session headers, prefer the healthy remembered target, and rebind after fallback; failures clear mappings for the affected target.
- **Guardrail**: apply session affinity after health filtering in every transport path; only validated generation success can bind it. Keep successful fallback bindings against late old completions and primary recovery, retain response-ID origins, and use connection/lane affinity for adapted WS without a client key. Bound/expire keys and invalidate changed credentials or failed targets; preserve unaffected bindings on metadata/config reorder. Covered by `session_affinity_prefers_last_successful_target_across_round_robin` and `session_affinity_is_cleared_when_a_target_fails`.
- **Watch points**: `crates/aipass-proxy/src/lib.rs` forwarding/model discovery and `crates/aipass-proxy/src/websocket.rs` handshake selection.

### Held retries and eager recovery churned stable fallback sessions
- **Symptom**: a failed high-priority provider was retried during its ban or regained session traffic on cooldown expiry, reducing prompt-cache reuse on a reliable backup.
- **Root cause**: `lib.rs` passed `hold_round > 0` as a circuit bypass; target sorting ignored unresolved health; cooldown was fixed and model queries/config reload/late completions replaced session state.
- **Fix**: prioritize established sessions, then stable targets, recent recovery, and unresolved degradation. Default three failures open a 30-second ban; failed half-open recovery immediately doubles it up to 15 minutes, allowing one generation at a time. Keep ten minutes of flapping history after recovery and never generate background recovery requests. Preserve unaffected routing state while still invalidating authenticated transport snapshots on config reload.
- **Guardrail**: never bypass a target ban in HTTP or native WS hold/silent retries. Acquire recovery slots at submission and release on completion/cancellation. Do not move a working session merely to test priority restoration. Retain explicit-failure-only replay rules and provider-bound opaque context. Test real multi-turn HTTP/SSE/adapted WS, blacklist budgets/escalation, model discovery, stale completion, and credential vs metadata reload in `tests/stability.rs` and `websocket/tests/adaptive.rs`.
- **Watch points**: `routing.rs`, shared selection/forwarding/usage completion, native WS `response.create`, adapted WS lane identity, and channel degradation projection.

### Provider WS capability must not become a route-wide transport choice
- **Symptom**: one HTTP-only provider forced every target in a mixed route to SSE, while a rejecting WS endpoint could repeatedly fail despite working over SSE.
- **Root cause**: `crates/aipass-proxy/src/websocket.rs` selected the bridge once for the whole route; native handshakes shared the ordinary HTTP target circuit, and pre-output buffering could replay a submitted generation.
- **Fix**: prefer transparent relay for the actual selected native target, select upstream WS per provider only inside fallback/converted sessions, keep temporary WS fallback session-scoped and require two 404/405/501 refusals plus successful Responses HTTP generation before disabling the provider preference, and use the shared connector for non-generating Responses warmup probes.
- **Guardrail**: never infer missing WS support from transport errors, model discovery, or a 101 handshake alone. Do not replay a WS client's generation after submission or an ambiguous disconnect, including before output and with silent retry enabled. Keep probe auth/quota/timeouts inconclusive, invalidate evidence only for changed effective transport configuration, and let concurrent valid WS completion invalidate pending rejection evidence. Regression coverage: `websocket/tests/adaptive.rs`.
- **Watch points**: `websocket.rs` native relay, `websocket/upstream.rs` mixed-route WS, `websocket/bridge.rs`, `lib.rs` forwarding/WS health, agent `provider.probe`, and desktop probe rendering.

### WS preference must cover HTTP entry points and preserve upstream connections
- **Symptom**: mixed routes opened a new upstream socket for each generation, HTTP Responses clients never tried WS, and idle upstream sockets had no proactive heartbeat.
- **Root cause**: `websocket/upstream.rs` owned a socket only for the response lifetime; `lib.rs` gated it on the inbound `websocket` flag; native relay only replied to peer pings.
- **Fix**: prefer WS for native Responses HTTP and WS requests, retain successful sockets only inside fallback WS sessions, for 120 seconds, with exclusive leases and handshake/session isolation, and add hop-local ping/pong liveness checks.
- **Guardrail**: never share a leased socket between active generations or client connections. A Codex close ends every upstream lease and idle worker; never reuse across client connections or retain HTTP-request sockets. Keep native sessions transparent even on mixed routes. Revalidate fallback sockets before generation, drop them on credential/interface refresh, cancellation or error, preserve buffered HTTP response shape, and never count control frames as response progress. Reconstruct complete tool history before provider changes; reject orphan results and pin opaque provider state to its source target. Cover reuse, idle heartbeats, isolation, refresh, safe reconnect and no replay in `websocket/tests/adaptive.rs`.
- **Watch points**: `lib.rs` HTTP entry, `websocket.rs` native relay, `websocket/bridge.rs` session ownership, `websocket/upstream.rs`, `websocket/pool.rs`, and `websocket/keepalive.rs`.

### Auxiliary WS notifications must not break Responses adaptation or probing
- **Symptom**: Replica native WS completed successfully, but HTTP-to-WS streaming broke immediately and the WS capability probe stayed inconclusive.
- **Root cause**: `websocket/upstream.rs` `ResponseStream::next_event` rejected non-`response.*` events; `websocket/probe.rs` also rejected the normal `codex.rate_limits` notification before warmup completion.
- **Fix**: preserve well-formed typed auxiliary events through streaming adaptation, ignore them while buffering a response, and let probes continue past them to a validated empty-output warmup completion.
- **Guardrail**: never treat an auxiliary notification as generation completion, first token, or transport recovery. Keep malformed events and incomplete/disconnected warmups unsuccessful, and never replay a submitted generation. Cover HTTP streaming/buffering, native/adapted WS and probe notification ordering in `websocket/tests/adaptive.rs`.
- **Watch points**: `websocket/upstream.rs`, `websocket/probe.rs`, `websocket/bridge.rs`, and `SessionUsage::server_event` in `websocket.rs`.

### Provider API bases and client identity must survive every transport
- **Symptom**: automatic namespace inference treated `/v3`, `/v1beta` and `/openai` as if the user had supplied `/v1`; configured identity headers could overwrite Codex, and configured local metadata escaped upstream filtering.
- **Root cause**: `crates/aipass-proxy/src/lib.rs` `upstream_url_with_query` generalized version detection beyond the user's explicit `/v1` input; `build_upstream_headers` filtered only two inbound local header names and let configured headers replace client identity.
- **Fix**: per the 2026-09-07 user correction, omit the appended `/v1` only when the user base contains an exact `/v1` path segment, retaining the official Codex OAuth backend and Azure resource-path contracts. Prefer inbound User-Agent/originator and filter AIPass identity/local metadata from both incoming and configured headers. Generated bridge response IDs use a neutral `resp_` prefix.
- **Guardrail**: use the shared URL/header builders for HTTP, model discovery, native WS and adapted WS. Check `/v1` as a path segment, never in the hostname/query or as a prefix of `/v10` or `/v1beta`; do not infer it from another namespace. Never inject an AIPass identity or replace an existing client identity with provider defaults. Preserve native request/response bytes and test wire-level auth replacement, opaque tools, session metadata and provider base/query handling. Coverage: `tests/transparency.rs` and native/converted WebSocket tests.
- **Watch points**: `lib.rs` `upstream_url_with_query` / `build_upstream_headers`, `websocket.rs` `upstream_headers`, `websocket/bridge.rs`, and the agent's provider model probe URL builder.

### Upstream failures lost provider explanations
- **Symptom**: local proxy logs showed a failed attempt without an actionable provider error.
- **Root cause**: `crates/aipass-proxy/src/lib.rs` discarded non-success bodies before failover; WS and SSE had separate error paths.
- **Fix**: shared bounded error extraction for HTTP/model discovery, WS handshake and WS/SSE error events; retain request/target/provider IDs and resolve display names from the unlocked desktop list.
- **Guardrail**: persist only sanitized error fields/excerpts, never whole wire payloads or titles. Bound error reads and stream event buffers, redact active credentials and headers, and keep failed-attempt detail even if fallback succeeds. Tests cover 403 failover, WS rejection, split SSE errors and unrelated-payload redaction.
- **Watch points**: `diagnostics/upstream.rs`, HTTP/model discovery, WS connector/session trackers, and `proxyLogs.ts`.

### WS capability persistence and recovery must share agent authority
- **Symptom**: intermittent WS failures disabled unrelated sessions, stale edit drafts could undo automatic closure, and preference refresh could cut off a working fallback response.
- **Root cause**: `crates/aipass-proxy/src/lib.rs` combined provider transport failure counts with preference, while `apps/desktop/src/App.svelte` always sent the draft switch and runtime invalidation treated preference as credential configuration.
- **Fix**: typed handshake/fallback outcomes; one pre-submission reconnect; bounded session fallback; versioned joint evidence and an agent-consumed event; atomic Vault preference/warning update; non-generating recovery validation outside the vault lock; touched-only UI updates. Preserve automatic closure explanations until recovery succeeds.
- **Guardrail**: keep all three WS entry paths on the shared capability ledger. Never replay an ambiguous submission. Reject stale configuration epochs, duplicate events and candidates invalidated by WS success. Keep unsaved events through lock/write failure/listener stop, revalidate before persistence, and clear only after refresh. Retain refresh retries separately from live evidence: a record can commit before auditing fails, and later WS success must not erase that committed change's refresh/revision. A preference-only update must not revoke live transports. Require valid empty WS completion before false-to-true saves and compare configuration again after probing. Cover these in `websocket/tests/adaptive.rs`, `proxy_service::tests::websocket_capability_survives_lock_stop_retry_and_vault_reopen`, `websocket_recovery_tests.rs`, Vault durability tests, and `App.test.ts`.
- **Watch points**: proxy capability/upstream/native/bridge/pool, agent background worker/provider update/probe, Vault summary and narrow update, Tauri DTO, schemas/UI types, provider details/form/i18n, and protocol version.

### Standalone image calls need separate capability and replay rules
- **Symptom**: Images paths were rejected locally; chat health, a tool declaration or a partial image could be mistaken for usable image generation support.
- **Root cause**: `crates/aipass-proxy/src/lib.rs` recognized only conversational protocols, while Images JSON/multipart requests and image SSE events have a separate wire contract.
- **Fix**: `images.rs` forwards generation/edit requests through existing authenticated route targets and shared URL/header/body helpers, with bounded request-driven capability evidence and byte-preserving image streams.
- **Guardrail**: distinguish operation, model, streaming mode and effective credential configuration; filter unsupported/non-OpenAI targets before limiting attempts. Never learn unsupported from auth/quota/parameter errors or from missing output. Never use a chat protocol converter, change conversation affinity, or replay an ambiguous submitted image request. Preserve slow generation and config cancellation. Keep capabilities ephemeral and payloads out of diagnostics.
- **Watch points**: Images entry auth, multipart metadata, `images::candidates`, structured error classification, large split image SSE events, and `tests/image_api.rs`. Native Responses image tools are a separate capability.

### Provider concurrency admission must cover every forwarding path
- **Symptom**: channel activity statistics alone cannot enforce a provider limit, and independent channels/keys can bypass a per-target cap.
- **Root cause**: `TargetActivityGuard` counted by target; HTTP generations, Images, model discovery and native WS submission had separate admission paths.
- **Fix**: provider-wide atomic permits shared across routes, including unlimited occupancy for live limit changes. Limited WS routes use the existing per-generation bridge; storage and IPC carry the provider-owned setting with protocol v7.
- **Guardrail**: acquire before submission and release after completion/cancellation; never truncate the candidate list before skipping full providers. Preserve occupancy across config reloads and keep busy skips out of circuit health. Run `provider_concurrency` regressions, vault durability and desktop edit/save/reopen tests.
- **Watch points**: `concurrency.rs`, HTTP/model dispatch, `images.rs`, native WS and bridge, Agent runtime snapshots, Vault update omission semantics, Tauri/provider form mappings. Never bypass provider-bound continuation or no-replay protections to find a free slot.
- **Form guardrail**: update a numeric draft value and its touched flag in one input handler. A separate `bind:value` plus touched mutation can let a parent reactive refresh restore the old value before the binding reads it. Verify changing a nonempty limit in a real 960×640 browser as well as the App regression.

## Public model pricing (aipass-agent pricing)

### Startup-only refresh and encrypted metadata left prices stale
- **Symptom**: a long-running agent never retried a failed price download or refreshed again; downloads while locked left the reported update time stale. Empty upstream tables could replace a good cache with built-in prices.
- **Root cause**: `crates/aipass-agent/src/pricing.rs` `spawn_list_price_refresh` ran once, `refresh_list_prices` saved its timestamp through `with_vault`, and checked for usable rules only after adding built-in fallbacks.
- **Fix**: `pricing/list_prices.rs` schedules daily refreshes and hourly retries, stores public prices and their timestamp atomically, and validates remote rules before adding fallbacks. The pricing config reads the timestamp from the public cache.
- **Guardrail**: keep public catalog refreshes independent of vault unlock and application releases; reject unusable remote data before replacing the last good snapshot. Covered by the refresh schedule, download failure, and locked-vault tests in `pricing/list_prices.rs`.
- **Watch points**: `server.rs` background startup, `pricing.rs` config reads, `handlers.rs` usage summary and timeseries price loading.

### Backup inclusion and restore allowlists must stay aligned
- **Symptom**: restoring an encrypted backup silently discarded pricing groups and assignments.
- **Root cause**: `lib.rs:2510` export enumeration and `lib.rs:1957` import validation both omitted `pricing.aipstate`.
- **Fix**: share the backup root-file list between export/import and include encrypted pricing; preserve the separate synchronization policy.
- **Guardrail**: add new persistent user state to both backup and restore through the shared list. Test export/import/decrypt round trips and explicitly assert whether each local file enters sync snapshots.
- **Watch points**: vault `BACKUP_ROOT_FILES`, `sync_path_allowed`, agent pricing state and server configuration.

## Endpoint inference (three implementations)

### Valid AI endpoints misclassified as custom_http
- **Symptom**: OpenAI-compatible endpoints such as `/api/paas/v4`, `/v1beta`, custom-domain Anthropic relays, and MiniMax `/v1` were saved as `custom_http`, making them non-proxyable.
- **Root cause**: inference lived in three places that had drifted apart: `packages/schemas/src/index.ts` `inferProviderFromEndpoint` (narrow 10-word fallback regex), `apps/extension/src/content/detector.ts` `inferInterfaceFromEndpoint` (wider keyword set, plus a `replicate|cohere|minimax` short-circuit that ran before the OpenAI check), and `crates/aipass-agent/src/server.rs` `infer_interface_from_endpoint`. The schemas fallback was the narrowest and won in save flows.
- **Fix**: shared evidence regexes exported from schemas and consumed by the extension; Rust hand-aligned with a sync comment; minimax registry entry corrected to `openai_compatible`; `custom_http` is now strictly the no-AI-evidence fallback.
- **Guardrail**: endpoint inference changes must land in all three implementations in the same change, with regression tests on both sides. `custom_http` may only be chosen when no AI evidence exists.
- **Watch points**: `packages/schemas/src/index.ts`, `apps/extension/src/content/detector.ts`, `crates/aipass-agent/src/server.rs` `infer_interface_from_endpoint`, `crates/aipass-provider-registry/src/lib.rs` interface lists.

### New API speed-test URLs were saved as API endpoints
- **Symptom**: importing a token from a New API console stored the channel's health-check/speed-test URL as the provider API endpoint.
- **Root cause**: the extension treated every HTTP URL with API-like path evidence as equivalent and did not inspect labels such as `测速地址` when choosing among candidates.
- **Fix**: endpoint extraction now includes input labels and filters explicitly labelled speed-test URLs, external speed-test wrappers, and known health-check paths before applying API evidence; regression tests cover mixed, speed-test-only, neighbouring, and external-link New API pages.
- **Guardrail**: when scraping endpoint candidates, exclude speed-test, probe, latency, ping, and health-check URLs before provider inference; if no API URL remains, fall back to the provider origin rather than persisting the test target.
- **Watch points**: `apps/extension/src/content/detector.ts` endpoint candidate ranking, `apps/extension/src/content/detector.test.ts`, and any future detector implementation that derives endpoints from gateway management pages.

## Update channel resolution (apps/desktop)

### Update notices appeared before vault unlock
- **Symptom**: a background update download displayed its banner over the locked or onboarding screen; restart confirmation could survive a lock.
- **Root cause**: `apps/desktop/src/App.svelte` rendered the banner solely from the available version and mounted the restart dialog outside the authenticated workspace.
- **Fix**: gate both surfaces on the visible unlocked workspace, clear confirmation on lock, and recheck visibility after the asynchronous proxy-status probe.
- **Guardrail**: keep background update results pending until unlock; hide update notices and close confirmation when locking. Exercise lock/unlock and delayed restart checks in `App.test.ts`.
- **Watch points**: automatic update banner, restart confirmation and settings update UI.

### Stored channel preference outlived the build's version family
- **Symptom**: after a beta build updated to a stable release, the app kept polling the beta update feed (and vice versa).
- **Root cause**: every call site used `getStoredUpdateChannel() ?? inferUpdateChannel(version)`, so a manual/stale localStorage choice permanently overrode the channel implied by the running build.
- **Fix**: `resolveUpdateChannel(version)` is the single entry point — version inference wins, a mismatching stored value is cleared; empty version falls back to stored ?? "official".
- **Guardrail**: the update channel follows the build's version family; a stored preference is a hint, never an override across families. All call sites must go through `resolveUpdateChannel`. Enforced by `updates.test.ts`.
- **Watch points**: `apps/desktop/src/App.svelte` update flows, `SettingsPanel.svelte` channel picker.

### Nightly tag validation and update feeds must agree
- **Symptom**: a repeated daily nightly tag passed release validation but was ignored by the updater feed.
- **Root cause**: `.github/workflows/release.yml` accepted a daily revision suffix while `apps/web/src/worker.ts` still matched only the date.
- **Fix**: align both tag patterns and exercise the public nightly feed against revised, older, draft, and beta release candidates.
- **Guardrail**: when changing release tag syntax, update both release validation and the deployed channel resolver. Verify the public feed selects the published version before declaring the release complete.
- **Watch points**: `.github/workflows/release.yml`, `apps/web/src/worker.ts`, and `apps/web/scripts/worker.test.mjs`.

### Updating the desktop stopped the installer itself
- **Symptom**: installing nightly closed the app before replacing its bundle; every subsequent launch retried the cached update and exited again. Logs repeated `desktop.update.install_pending` → `desktop.singleton.quit_request` → `desktop.exit.completed`, still on the old version.
- **Root cause**: `apps/desktop/src-tauri/src/updates.rs:421,437` called `stop_tray_autostart_for_current_desktop`, which sets the supervisor's kill-child flag and sends Quit to the same desktop singleton that is running the installer (`crates/aipass-agent/src/autostart.rs:400`). A launchd-started desktop can also die when its supervisor's process group is removed.
- **Fix**: use a separate tray suspension operation without Quit or a kill-child flag, preserve tray children with `AbandonProcessGroup`, and restore supervision on failed installs. Keep the tray UI alive until the normal restart/exit path. Back up and remove a stuck pending-update cache to recover an already affected old build; that build cannot install its own fix reliably.
- **Guardrail**: update preparation must keep the installing desktop alive for both manual and login launches; stop/uninstall APIs are for intentional termination. Exercise actual launchd suspension, stale stop flags, child survival and supervisor restoration in `suspending_tray_supervision_preserves_the_desktop_child`. Both workflows must run `scripts/verify-macos-runtime.mjs` on the finished DMG and updater archive; require responsive main/tray startup and manual/cached install → new-process restart → cleared cache. Keep original signatures and retain failure diagnostics. See `docs/desktop-artifact-validation.md`; signature and bundle checks alone do not validate update lifecycle.
- **Watch points**: both agent-available/unavailable branches in `stop_runtime_processes`, `install_pending_update`, `install_update`, tray Quit, and macOS supervisor/plist generation.

### Background readiness restarted the agent during bundle replacement
- **Symptom**: the finished-artifact install test failed with `AIPass agent did not exit before update`; logs showed a new agent PID between authenticated shutdown and installation.
- **Root cause**: desktop status polls and the tray watchdog called `ensure_agent_running_for_desktop` while `updates.rs::stop_runtime_processes` was waiting for the agent to exit. Stopping launchd alone did not stop desktop-owned restart attempts.
- **Fix**: serialize bundle installation against startup and autostart repair with `runtime_lifecycle::RUNTIME`. Wait for in-flight startup, reject new readiness/repair attempts without blocking the native event loop, and release the update guard before failure recovery.
- **Guardrail**: hold the runtime update guard from agent shutdown through bundle replacement and process restart. Test queued startup, watchdog rejection and failed-update recovery in `update_waits_for_existing_startup_and_rejects_watchdog_restarts`; keep ordinary background polling active in the artifact install scenarios so this race remains observable.
- **Watch points**: desktop startup warmup, all desktop/tray readiness calls, login-agent repair and both update-install entry points.

## Tray vs UI vs agent validation

### Tray could start the proxy with no valid route groups
- **Symptom**: starting the local proxy from the tray succeeded (or failed with a cryptic message) in configurations the UI correctly refused, e.g. zero enabled route groups.
- **Root cause**: `TraySnapshot::can_start_proxy` only checked agent/vault/proxy run-state, while `ServerDetailPane.svelte` disabled Start on `enabledRoutes.length === 0`, and the agent's `start()` did not reject an empty enabled-route set either — three entry points, three validation levels.
- **Fix**: tray `can_start_proxy` requires an active route; agent `start()` rejects configs with no enabled route group (ValidationFailed); tray surfaces the agent's validation error text instead of a generic failure.
- **Guardrail**: every action with multiple entry points (tray, UI, CLI, extension) must share one server-side validation; UI-level gating is a convenience, never the authority. When changing validation, update all three layers together.
- **Watch points**: `apps/desktop/src-tauri/src/tray.rs`, `apps/desktop/src/lib/components/server/ServerDetailPane.svelte`, `crates/aipass-agent/src/proxy_service.rs` `validate_config`/`start`.


## Usage periods (desktop / agent / proxy store)

### Chart range did not filter provider details
- **Symptom**: switching between 24 hours, 7 days, and 30 days changed chart totals while provider details continued showing all history.
- **Root cause**: `UsageChart.svelte:11` owned a private range, while `App.svelte:1791` loaded an unfiltered summary and `UsageStore::summary` read every request and attempt.
- **Fix**: share the selected range in `ServerDetailPane`, preload matching summaries with chart series and publish them together, and use `usage_window_start` for both core queries. Filter attempts as well as requests.
- **Guardrail**: keep chart and breakdown on the same range and timezone; apply the cutoff to requests, attempts, costs, and health metrics. Enforced by `usage_summary_matches_chart_periods_and_filters_attempts`, `serverUsage.test.ts`, and `ServerDetailPane.test.ts`.
- **Watch points**: `App.svelte` refresh/clear/reset, `services/serverUsage.ts`, `UsageChart.svelte`, Tauri summary command, agent summary handler, `UsageStore::summary_since` / `timeseries`.


## OAuth lifecycle and native credential reconciliation

### Canceled device flows and failed persistence reused one-shot exchanges
- **Symptom**: closing/canceling during an in-flight start or poll could reconnect later; a failed vault write spent the authorization code again on retry.
- **Root cause**: `oauth/mod.rs` retained only device metadata, while `handlers.rs` consumed after persistence without caching tokens or serializing cancellation. `OAuthConnectDialog.svelte` invalidated cancellation only after IPC completed and accepted late start responses.
- **Fix**: cache exchanged bundles until successful commit, serialize completion/cancellation, enforce one in-flight poll and server intervals, clear pending flows on lock, and invalidate UI generations immediately.
- **Guardrail**: test cancellation during both start and poll, failed persistence followed by retry without a second token exchange, and overlapping polls. Cached token bundles must be redacted and zeroized on drop.
- **Watch points**: `oauth/mod.rs` tests, OAuth handlers, session lock, `OAuthConnectDialog.test.ts`.

### Refresh races and native reconciliation mixed token generations or accounts
- **Symptom**: an old refresh failure invalidated a fresh login; native CLI rotations could trigger false reauthentication or copy another account's tokens; identical emails across Codex workspaces reused the wrong header.
- **Root cause**: `oauth/refresh_loop.rs` guarded only successful responses by timestamp; `oauth/native_write.rs` compared generations without account identity; `official_accounts.rs` deduplicated only by email/fingerprint. Codex nested `error.code` was ignored.
- **Fix**: compare both timestamp and refresh token on success and failure; adopt complete newer native bundles before refresh and recheck after rejection; match workspace plus subject for Codex and identity for Grok; deduplicate Codex entries by workspace too; parse nested/flat machine error codes.
- **Guardrail**: preserve token/header/account identity together. Guard every asynchronous refresh outcome by its source generation. Test the same timestamp with different refresh tokens and the same email with different workspaces.
- **Watch points**: refresh-loop race test, native account/rotation tests, official-account workspace regression, Codex refresh error parser.

### Native OAuth backup and mirror failures escaped vault protections
- **Symptom**: native backup files retained plaintext refresh tokens; malformed native files were overwritten as if missing; native write failures prevented saving a rotated token in the vault.
- **Root cause**: `oauth/native_write.rs` copied raw bytes and flattened read/parse errors to `None`; login and refresh propagated mirror errors before storing managed accounts.
- **Fix**: encrypt new native backups with the vault backup key, preserve malformed/unreadable files, and treat native write errors as logged mirror failures while committing managed credentials. Bound OAuth response reads and redact parse errors.
- **Guardrail**: never create plaintext credential backups or treat unreadable credentials as absent. Keep authoritative token persistence independent of optional native mirrors; test encrypted backup recovery and malformed input preservation.
- **Watch points**: native backup tests, login completion, background refresh persistence, OAuth response parsing tests. Existing legacy backup files are not migrated by this change.


### OAuth management hid destructive effects and recovery states
- **Symptom**: account removal silently retired linked provider routes; loading looked empty, failed login returned to provider selection, and failed browser/clipboard actions gave no feedback.
- **Root cause**: `OAuthConnectDialog.svelte` used a single busy flag and icon-only account actions, mixed loading with empty lists, and used WebView `window.open` for an external browser.
- **Fix**: separate connection and account views with explicit loading/error/retry states; preserve provider and reauthentication context; confirm removal with its effects; notify the host after a successful removal even if list refresh fails. Launch provider HTTPS links through the Tauri `oauth_open_verification` command on an explicit user click and surface fallback instructions.
- **Guardrail**: never equate a loading/failed list with an empty list. Confirm account removal before IPC; verify host invalidation independently of list reload. Test browser/clipboard failures and retry without losing the selected provider. Clear delayed close callbacks on unmount.
- **Watch points**: `OAuthConnectDialog.test.ts`, `src-tauri/src/oauth_browser.rs`, host `onAccountsChanged`, and the shared English/Chinese OAuth messages. Validate all states at 960×640.

### Advanced route settings failed without actionable feedback
- **Symptom**: editing retry settings appeared not to save when the agent rejected an invalid backoff range; the dialog closed or showed no reason to correct the values.
- **Root cause**: the route dialog used bespoke controls and discarded a `false` save result, while the agent requires maximum backoff to be at least initial backoff (`crates/aipass-agent/src/proxy_service.rs:1068`).
- **Fix**: reuse shared form/card controls, validate retry numbers and ordering before IPC, and keep the dialog open with an inline error on persistence failure.
- **Guardrail**: validate advanced retry fields before saving and surface every failed route-config write in the open dialog. When disabling an option, retain valid persisted/default numbers instead of submitting invalid hidden inputs. Covered by `RouteGroupDialog.test.ts`.
- **Watch points**: `RouteGroupDialog.svelte`, `App.svelte` `saveRouteGroup`, and agent `validate_config`.

### Lock and save outcomes must own the active dialog lifecycle
- **Symptom**: OAuth accounts stayed over the lock screen, settings errors appeared behind the drawer, a failed close hid unsaved settings, and save retries left an empty editor or duplicated providers.
- **Root cause**: `App.svelte:1098` omitted OAuth from lock cleanup; settings closed before awaiting persistence; provider saves inferred success from a shared stale error and had no operation-level exclusion.
- **Fix**: gate OAuth on the unlocked workspace; keep settings open until save succeeds with feedback inside the drawer; return explicit provider save results and guard in-flight writes. Preserve drafts from late settings reloads.
- **Guardrail**: unmount every sensitive dialog on lock. Keep failed mutations visible with their draft, close on explicit success, and distinguish committed writes from refresh failures. Test actual App-to-dialog paths, including late list responses, close-save failure and repeated submits at 960×640.
- **Watch points**: `App.operations.test.ts`, `IntegrationCard.test.ts`, inline/provider-modal saves, settings close/Escape/outside events, OAuth account loading and login cancellation.

## Desktop operation feedback

### Shared errors leaked into the selected provider
- **Symptom**: import, settings and other operation failures appeared as a permanent error banner on the provider detail page.
- **Root cause**: `App.svelte:3331` forwarded its shared `errorText` into `ProviderDetailPane.svelte:523`, independent of which operation failed; closing a dialog exposed its stale error behind it.
- **Fix**: remove the detail page's generic error prop, route ordinary failures into a dismissible, expiring toast, and scope inline auth, provider-form and settings feedback to their own surfaces. Dialogs that already handle rejected callbacks own their errors without a duplicate host notification.
- **Guardrail**: report operation errors through `reportError`; never pass app-wide errors to an entity detail page. Test unrelated errors during editing, repeated identical failures, auto-dismiss and closing a failed settings surface in `App.operations.test.ts`. Preserve drafts on failure and clear workspace toasts on lock.
- **Watch points**: App operation handlers, provider inline/modal saves, settings feedback, integration/usage dialogs, and `ErrorToast.svelte` at 960×640.

## Build toolchain

### CI silently omitted the DMG installation layout
- **Symptom**: published DMGs opened without the configured background or icon positions; the original 1x PNG also looked soft on Retina displays.
- **Root cause**: `.github/workflows/release.yml` set `CI=true`, which makes Tauri pass `--skip-jenkins` to create-dmg and skip saving Finder's layout. The background generator downsampled everything to 660×400 pixels.
- **Fix**: set `TAURI_BUNDLER_DMG_IGNORE_CI=true` in both macOS workflows, generate a TIFF with 1x/2x representations at the same logical size, and verify the mounted DMG with `scripts/verify-macos-dmg.mjs`.
- **Guardrail**: build and mount the actual DMG on macOS with Finder; verify saved background selection, both image resolutions, window/icon positions, Applications link, and executable payloads. A copied image file alone does not prove Finder uses it.
- **Watch points**: both workflows, `tauri.conf.json`, the background generator, and the local macOS bundle gate.

### Homebrew Rust's objcopy dependency produced misaligned macro libraries
- **Symptom**: macOS 27 Tauri release builds failed to load procedural macros with `mis-aligned LINKEDIT string pool`, including after rebuilding their caches.
- **Root cause**: Homebrew's Rust 1.98.0 formula links `lib/rustlib/<host>/bin/rust-objcopy` to LLVM 22's `llvm-objcopy`, whose debug-info stripping has the Mach-O alignment defect in [rust-lang/rust#157750](https://github.com/rust-lang/rust/issues/157750). LLVM 23.1.0 contains [the alignment fix](https://github.com/llvm/llvm-project/pull/203680). Both `scripts/build-desktop-sidecars.mjs:15` and Tauri inherit the same compiler and dependency.
- **Fix**: keep the latest Homebrew `rust`, upgrade Homebrew `llvm` to 23.1.0, and point only the Rust sysroot's `rust-objcopy` symlink to `$(brew --prefix llvm)/bin/llvm-objcopy`. Keep Rust's LLVM 22 library dependency unchanged. The same stripped-library reproducer fails with LLVM 22 and loads with LLVM 23; rebuild affected cached macros after the repair.
- **Guardrail**: use Homebrew-managed Rust locally per the user's preference; verify Cargo/rustc provenance and the actual objcopy link before macOS validation. Recheck a freshly compiled stripped library after Rust upgrades/reinstalls, which can recreate the formula's original symlink. Do not replace the LLVM 22 library with LLVM 23 or switch to rustup to bypass the issue.
- **Watch points**: desktop development, `scripts/build-desktop-sidecars.mjs`, Tauri release builds, and the local macOS validation shell.
