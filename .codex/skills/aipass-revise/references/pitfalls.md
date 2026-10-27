# AIPass Pitfalls Registry

Lessons sedimented from real bug fixes. Read the entry for an area before modifying it.
Newest entries last within each section.

## Sync lifecycle (aipass-agent server / session / sync_watch)

### Audit events multiplied encrypted snapshot storage
- **Symptom**: one macOS vault accumulated hundreds of GiB in `sync-cache` and `sync-outbox`; CloudKit stopped accepting snapshots above the 11 MiB IPC limit.
- **Root cause**: each immutable sync snapshot included the growing local audit directory, every failed retry kept another full snapshot in both directories, and automatic usage/subscription refresh wrote audit events repeatedly. CloudKit also retained every successful historical snapshot.
- **Fix**: remote snapshot v2 excludes audit while full local recovery journals retain it; coalesce verified pending history to one latest snapshot, share cache/outbox data with hard links, cap local audit allocation, and prune CloudKit history after a successful authenticated sync. Bound full copies by count (8), bytes (8 MiB) and age (30 days); a 128 MiB byte-only budget still left 178 snapshots for a 400 KiB vault. Include acknowledged checkpoint heads omitted by an eventual listing so their parents stay protected.
- **Guardrail**: exclude high-churn local telemetry from remote state; bound retry queues independently of network health, preserve the confirmed remote parent during compaction, and never remove a remote branch head or its direct parent or prune during conflict/quarantine. Test old pending migration and crash recovery before cleaning real data. Measure normal post-upgrade full-copy counts and allocated bytes; a generous byte ceiling alone does not demonstrate acceptable steady-state storage.
- **Watch points**: `aipass-vault` snapshot export/apply and audit retention; `aipass-agent` staging, checkpoint and CloudKit retention; signed Swift CloudKit transport; the IPC snapshot size bound.

### Cloud history deleted by another device left local cached copies behind
- **Symptom**: one device reduced CloudKit history while another device kept every snapshot it had downloaded, even after successful syncs.
- **Root cause**: `vault_sync/cloudkit_retention.rs::prune` removed local cache files only when this device itself deleted the remote snapshot. A later remote listing no longer included peer-deleted records, so they never became deletion candidates here.
- **Fix**: collect orphaned local cache bodies and metadata after authenticated CloudKit cleanup. Mark active remote IDs, every target's checkpoints/conflicts, queued uploads and direct parents before sweeping; hold the session lock only during local collection, after remote I/O, so staging cannot race deletion.
- **Guardrail**: test both the deleting device and a peer with already-cached historical bodies. Preserve inactive targets and pending uploads, abort before any local deletion on corrupt recovery metadata, and serialize local mark/sweep with vault staging. Never hold the session lock across CloudKit requests.
- **Watch points**: `vault_sync/storage/cache_retention.rs`, checkpoint/outbox compaction and locked sync; `keeps_other_targets_checkpoints_parents_and_unsent_uploads` and `unreadable_checkpoint_aborts_before_removing_any_body`.

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

### Vault replacement must reopen its local usage database
- **Symptom**: Windows file/WebDAV imports failed with access denied; Unix could retain a usage connection into the renamed backup.
- **Root cause**: `vault_sync::install_import` stopped the proxy but left `ProxyService` holding SQLite's open database inside the directory being replaced.
- **Fix**: release the stopped runtime's usage store before renaming, lock the session, reopen the installed or rolled-back database on every exit, and clear old runtime configuration after replacement. Keep the encrypted vault backup and settings recovery journal.
- **Guardrail**: test successful import and rollback against actual usage reads, retain old backup contents, and run the same import tests on native Windows. Fail closed if storage cannot reopen; never substitute an empty in-memory database.
- **Watch points**: `vault_sync/import.rs`, `proxy_service/import.rs`, WebDAV first-install, settings-write failures and Agent usage queries.

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

### Native switches must follow the latest vendor store
- **Symptom**: switching through API or proxy writes can leave a subscription bound to replaced active credentials, or replay an older refresh token.
- **Root cause**: the configuration writer previously backed up files without coordinating native-store ownership and local provider references.
- **Fix**: `tool_switch` journals file/Keychain/config/binding changes, archives the latest native grant, rebinds stable IDs, and shares transactions with local proxy integration.
- **Guardrail**: route every Codex/Claude native configuration write through the coordinator; standalone `env` helper generation does not change native credentials or bindings. Validate the displayed preimage, back up before renewal or mutation, and restore managed accounts from their latest vendor home. Hold the switch lock across the final restore/login-continuation check and mutation; acquire account locks outside the vault lock, including `openai` aliases and crash recovery. Canonicalize archive homes before deriving Keychain IDs.
- **Watch points**: `handlers/tools.rs`, `tool_switch/{native,restore,login,proxy}.rs`, Claude quota renewal in `official_accounts.rs`, `native_auth.rs`, `transaction.rs`, `ProviderIntegration.test.ts`, CLI `tool` commands. Acquire the coordinator before status snapshots/renewals; `status_waits_for_the_switch_before_reading_its_binding` covers an overlapping switch.

### Tool configuration and usage prices must bind an exact credential
- **Symptom**: mixed-format site keys could write the primary key using another key's protocol; automatic price refresh could replace explicit multipliers or edited shared rules.
- **Root cause**: tool requests selected only an entry and helpers read `api_key`; pricing inferred manual intent from a non-unit multiplier (`server::build_tool_config_plan`, `pricing::sync_newapi_pricing`).
- **Fix**: carry the stable secret ID through desktop, panel, CLI, agent and helpers; apply per-key endpoint/model/auth overrides and distinct config/env identifiers. Persist manual assignment/rule ownership and preserve it during refresh. Pricing forms save explicitly and retain failed drafts.
- **Guardrail**: reject ambiguous or missing key IDs; never fall back from a deleted selected key to a label or primary key. Invalidate previews on selection changes and all configuration inputs (including site auth and WebSocket settings), keep manual 1x settings, and require a visible history scope for shared rule edits/deletion.
- **Watch points**: `ProviderDetailPane.test.ts`, `PricingGroupDialog.test.ts`, `App.operations.test.ts`, agent `tool_configuration_binds_the_selected_key_and_its_overrides`, pricing `explicit_unit_multiplier_and_edited_rules_survive_gateway_sync`, vault exact-ID tests, proxy runtime refresh and LAN preview generations.

### Missing models must not hide compatible integrations
- **Symptom**: Grok Build disappeared from provider quick integration until a default model was set, making a supported OpenAI endpoint look unsupported.
- **Root cause**: `apps/desktop/src/lib/utils/integrations.ts::compatibleToolsFor` filtered on readiness, combining protocol compatibility with the default-model prerequisite.
- **Fix**: list protocol-compatible tools and show a disabled reason for missing models; keep execution eligibility strict and use the Grok Build product name.
- **Guardrail**: distinguish protocol compatibility from configuration prerequisites; treat whitespace-only models as missing in the UI and writer. Cover both OpenAI backends, catalog visibility and disabled controls.
- **Watch points**: provider and route quick integration, `integrations.test.ts`, `ProviderDetailPane.test.ts`, and Grok config writer tests.

### Quick integration and Agent management must share installed-tool capabilities
- **Symptom**: quick integration listed uninstalled tools and omitted Kimi Code, Zed, DeepSeek Harness and Antigravity CLI.
- **Root cause**: `IntegrationCard.svelte` sorted missing tools instead of filtering; configuration-directory presence was treated as installation, and the Rust catalog/writers contained only seven targets.
- **Fix**: share `installedIntegrationTools` between both entry points; add typed targets, executable discovery, credential readers and Rust writers. Group each new writer's backups in one discoverable directory so rollback restores every file across directories.
- **Guardrail**: test no detections, stale configuration without a binary, refresh, model prerequisites, every supported proxy protocol, and native-only rejection. Preserve source credentials and fingerprint all affected files before managed writes. Redact YAML refs and grant records before diffs; quote sourced shell values literally. Separate public config writers from UI-only BYOK guides, preserve account/edition limits, and never report a guide as an automatic switch. Detect editor extensions through their vendor manifest and installed entry point. Require the matching host IDE, reject obsolete extensions and dangling npm shims, and require Cursor Local itself. Recheck installation in the Rust write entry point. Raise the Agent protocol when adding tool IDs or write guarantees that older residents cannot implement. Generate shell helpers for the native OS, respect Windows USERPROFILE/APPDATA, and test actual PowerShell execution with non-ASCII paths and metacharacters.
- **Watch points**: `integrations`, `IntegrationCard`, `AgentManagementPage`, protocol/Tauri/CLI target mappings, `additional_tools`, `tool_credentials::additional`, backup grouping and `docs/agent-integrations.md`.

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
- **Watch points**: dialog portal mounting, `ServerDetailPane` lazy loading and `onLoadProxyLogs`, typed `server_logs` IPC. The control-panel modal shares its existing guarded state refresh; reverse its newest-first snapshot for tail following, preserve duplicate rows, and unmount the portal on authorization loss. Check focus return, live refresh and session removal in control-panel `App.test.ts`.

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
- **Guardrail**: never scan session/history files during preview, including direct, official and local-proxy modes. Never place history contents in an IPC frame. Legacy writer apply retains migration and encrypted backups; native transactions retain existing custom provider IDs and their preferences rather than rewriting history used by running CLI processes. Built-in IDs cannot override API routing: select the custom AIPass provider and preserve original session files. Test preview against unreadable history (`codex_preview_never_reads_session_history_in_any_auth_mode`) and native provider selection (`native_plan::tests`).
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

### Structured secrets must be redacted before diffing
- **Symptom**: TOML multiline tokens appeared in full-file previews and continuation lines of diffs.
- **Root cause**: Agent preview redaction used a regex matching only ordinary quoted values after generating a raw diff.
- **Fix**: `preview_redaction.rs` redacts JSON/TOML values recursively; the Agent diffs already-redacted documents. Integration completion also rechecks the selected credential after every await and treats cancelled vendor logins as terminal.
- **Guardrail**: never compute frontend diffs from raw credential documents. Redact whole Codex `auth` command objects and credential environment assignments, including custom AIPass key names. Test multiline literals, arrays/nested headers, delayed reauthentication previews, selection changes before apply, and cancelled polling (`preview_redaction::tests`, `tool_switch::tests`, `ProviderIntegration.test.ts`).
- **Watch points**: direct and proxy preview files, API helper commands, subscription login and restore callbacks.

### Claude Code must receive an unversioned Anthropic base
- **Symptom**: a mixed OpenAI/Anthropic provider produced a Claude Code configuration whose requests used `/v1/v1/messages`.
- **Root cause**: `crates/aipass-config-writers/src/plan.rs::plan_claude_code` and `plan_claude_code_plaintext` copied the selected or inherited versioned API base into `ANTHROPIC_BASE_URL`; Claude Code's SDK appends `/v1/messages` itself. Only the local-proxy path accounted for that difference.
- **Fix**: both Claude writers share terminal `/v1` normalization, preserving gateway prefixes and other version paths. Preview and apply use the same writer.
- **Guardrail**: test Claude helper and plaintext modes with per-key and inherited endpoints, both auth schemes, preserved unrelated settings, encrypted apply/rollback, and exact secret IDs. Keep OpenAI bases versioned. Cover mixed-format selection, read-only/full preview, confirmation and stale credential invalidation in `ProviderDetailPane.test.ts`.
- **Watch points**: direct provider, local-proxy and CLI configuration plans; agent `tool_configuration_binds_the_selected_key_and_its_overrides`; writer `claude_writers_normalize_versioned_bases_and_preserve_other_settings`.

### Credential overrides must survive the desktop request adapter
- **Symptom**: a credential's endpoint and default model appeared editable but reverted after saving; integrations kept using provider defaults.
- **Root cause**: `apps/desktop/src/App.svelte::addSecret` and `updateSecret` rebuilt metadata with only the older interface/group/billing fields, dropping the endpoint/model supplied by `ProviderDetailPane`.
- **Fix**: forward endpoint/model on both operations, trimming input and preserving empty strings on edits so Rust can clear overrides and restore inheritance.
- **Guardrail**: test the mounted App through its Tauri boundary, not only the detail component's callbacks. Cover add, edit, clear and reopen with mixed-format keys in `App.operations.test.ts`; keep omitted fields distinct from explicit clears.
- **Watch points**: `SecretKeyMetadata`, `SecretMetadataInput`, desktop add/update adapters, per-key integration availability and Agent configuration plans.

### Extension key editors must preserve the provider and credential boundary
- **Symptom**: extension provider editing could patch only the first key, change its format on a site-only save, and omit endpoint/model overrides supported by desktop.
- **Root cause**: `popup/Popup.svelte::draftFromEntry` hydrated first-key fields and native `ProviderUpdate` always patched that key; summary types and worker routing exposed no full per-key mutation.
- **Fix**: edit/add/remove by credential ID through existing Agent requests, mark site-only updates explicitly, preserve empty key arrays and share `CredentialBillingFields` with desktop. Refresh caches only from Rust's masked summary; invalidate stale entries when a successful mutation cannot return its summary.
- **Guardrail**: verify mixed-key edit/clear/rotation, provider-only saves, last-key deletion and lock rejection in native-host, worker and popup regressions. Never construct cached summaries from plaintext mutation requests. Keep credential sections from flex-shrinking inside popup scroll containers.
- **Watch points**: `CredentialList.svelte`, `Popup.svelte`, `native-client.ts`, `service-worker.ts`, native-host protocol/request adapters, and desktop `ProviderDetailPane`.

## Proxy credential snapshot (proxy_service / handlers)

### HTTP fixtures must consume complete requests before closing
- **Symptom**: `provider_update_refreshes_running_credentials_and_headers` intermittently returned 502 during the full workspace run but passed alone.
- **Root cause**: lifecycle and routing fixtures captured only one TCP read, then responded and closed with unread request-body bytes; segmented requests could lose the response to a connection reset.
- **Fix**: a shared bounded fixture reader consumes headers and the declared body before replying; all three credential-lifecycle fixtures use it.
- **Guardrail**: do not assume one TCP read contains a request. Run `fixture_consumes_headers_and_body_across_separate_reads` and the proxy-service suite; use read timeouts on fixture sockets.
- **Watch points**: `proxy_service/tests.rs`, `proxy_service/tests/{lifecycle,routing}.rs`.

### WebSocket completion is not a usage persistence barrier
- **Symptom**: a conversion test received both completed responses but immediately counted only one successful attempt in SQLite under a full workspace load.
- **Root cause**: forwarding a terminal frame and persisting its final usage are separate async steps; client close does not await the server-side database write.
- **Fix**: conversion-context, parallel-lane recovery and mixed-route fallback fixtures wait within a bounded deadline for successful usage records, then retain the exact request/attempt assertions.
- **Guardrail**: await the observed persistence state before asserting asynchronous usage counts; never replace exact-count assertions with arbitrary sleeps.
- **Watch points**: `websocket/tests.rs`, `websocket/tests/persistence.rs`, terminal event forwarding and usage persistence.

### Overlay teardown can outlive the test document
- **Symptom**: all desktop assertions passed but CI failed with `document is not defined` from Bits UI's deferred body scroll-lock cleanup.
- **Root cause**: settings and search-dialog fixtures unmounted overlays without waiting for their deferred cleanup; the callback could run after Vitest destroyed the browser environment.
- **Fix**: control timers from mount through unmount, drain deferred callbacks and assert the body scroll lock is released before restoring real timers.
- **Guardrail**: drain overlay cleanup before destroying its test document; do not suppress uncaught errors or substitute arbitrary sleeps.
- **Watch points**: `SettingsPanel.test.ts`, `SearchSettingsDialog.test.ts`, Bits UI overlay teardown and test browser lifetime. Keep the post-unmount scroll-lock assertion in both fixtures.

### Unified key editors must not resubmit a first-key snapshot
- **Symptom**: the first key had a separate editor and could not be deleted; provider saves could overwrite an independently edited key or relabel its successor after deletion.
- **Root cause**: `apps/desktop/src/App.svelte::openEdit` copied the first credential into the provider draft; `saveProvider` resubmitted it, while `summaryToEntry` treated an empty key list as missing legacy data.
- **Fix**: edit every key by stable ID, omit key fields from desktop provider updates, preserve explicit empty lists, and allow keyless provider metadata saves. Local deletion reloads live proxy credentials using the same tolerant path as sync.
- **Guardrail**: do not hydrate key fields into a provider-only draft. Distinguish absent legacy `secretRefs` from `[]`; test first/last deletion, subsequent provider saves, recreation, and per-key pricing/route cleanup without retargeting siblings. Save open key editors before provider metadata; retain the editor on failure and ignore completions from an older editor. Preserve legacy gateway defaults in provider-only saves.
- **Watch points**: desktop `ProviderDetailPane`, `App.repro.test.ts`, extension `entrySecrets`, vault `update_provider`, and proxy `deleting_first_and_last_keys_cleans_live_routes_and_pricing_by_id`.

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
- **Subscription coverage (2026-10-16)**: Agent subscription transport must also disable automatic retries and response read timeouts. Retire the operation setup deadline before provider generation dispatch; never put an idle timer around buffered SSE aggregation. Shield rotating credentials through durable ACK even if the setup deadline expires. Virtual-clock regressions live in `subscriptions/lifecycle/tests.rs` and proxy `subscription.rs`; establish real TCP connections before pausing Tokio time so scheduler auto-advance cannot masquerade as a connect failure.
- **Test guardrail**: allow CI startup/parsing/SQLite work before upstream submission, then wait beyond the configured budgets after the upstream acknowledges receipt. A 30 ms hold budget can expire before submission and falsely fail a slow-response test; race arrival against early request completion for actionable failures. Local development tools can send unauthenticated `GET /` probes to IPv4 and IPv6 test listeners: LocalStack traffic caused a false backup-connection failure on macOS. Ignore only those root probes in HTTP and WebSocket fixtures (including multimodal failover and handshake attempt counters), reject API/authenticated traffic at the backup, and independently verify that no diagnostic references the backup target.

- **Shared fixture correction (2026-10-19)**: `tests/mock_upstream.rs` now filters unauthenticated root discovery probes before capturing the expected request. Regressions preserve actual API requests and authenticated root requests. An isolated rerun cannot replace the full workspace validation. The hold-expiry test asserts the elapsed budget and a failed upstream request, not a minimum number of retries within 300 ms; the separate recovery test requires retries before success.

### Passive quota updates must reach held requests
- **Symptom**: an exhausted subscription recovers while a local request waits, but every retry still filters it out until the hold budget expires.
- **Root cause**: forwarding retained the authenticated route snapshot, including stale quota and admission metadata; quota updates intentionally do not invalidate active generations.
- **Fix**: refresh quota, concurrency, priority and weight from the live trusted runtime before each new selection round, retaining the original credential and protocol snapshot. Credential/account changes still invalidate the route. Regression: `tests/local_proxy_review.rs` covers recovery over actual local HTTP, new exhaustion, changed limits and credential isolation.
- **Guardrail**: never cancel a submitted generation merely because quota changes; never replay it to apply a new routing preference. Refresh eligibility only before a permitted next attempt, and keep encrypted reasoning/compaction pinned to its recorded origin.

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

### Subscription backends are not ordinary wire-compatible endpoints
- **Symptom**: Codex OAuth had a pinned URL/token but ordinary request bodies and JSON response expectations; CC/Responses routes were rejected despite the UI allowing both formats.
- **Root cause**: `proxy_service.rs::runtime_config_inner` resolved only credentials/protocol, while `forward.rs::prepare_upstream_request` and the converter lacked subscription semantics and a direct OpenAI pair.
- **Fix**: trusted runtime `UpstreamKind`, Codex stream-only normalization/collection, direct OpenAI request/response/SSE adapters, and account-bound usage requests. See `docs/subscription-proxy-review.md` for remaining gaps.
- **Guardrail**: resolve subscription kind from official credential metadata, never editable URLs or token prefixes. Keep native HTTP, downstream WS bridges, buffered responses and startup validation aligned; preserve call IDs, late usage and incomplete status. Never replay an unconfirmed/truncated generation or erase missing history to make a request pass.
- **Watch points**: Agent runtime resolution, `codex.rs`, `forward.rs`, `websocket.rs`, `openai/stream.rs`, `official_accounts.rs`; regressions `codex_subscription_restores_nonstream_response_and_keeps_token_scope`, `chat_stream_preserves_parallel_tools_reasoning_and_usage_after_finish`, `codex_usage_is_bound_to_the_requested_workspace`.

## Public model pricing (aipass-agent pricing)

### Routing groups need transport-wide model bindings
- **Symptom**: A mixed-brand group sent the caller's one model name to every member; spooled requests or native WebSocket paths could omit an override.
- **Root cause**: Config had no member model; Agent runtime initialized `model_override` to `None`, and HTTP/WS used different submission paths.
- **Fix**: Explicit member `model`, a public group alias, trusted Agent normalization, effective-model quota/pricing, and shared history-origin restrictions. Preserve rewritten spooled bodies and invalidate changed model bindings.
- **Guardrail**: Run `cargo test -p aipass-proxy model_routes` and Agent routing tests; cover HTTP/WS, API/subscription fallback, spooled JSON, unknown opaque history, cache invalidation and actual model quota. Keep schemas, desktop and panel round trips aligned.
- **Watch points**: `model_routes`, `forward/prepare`, `selection`, `websocket/upstream`, `proxy_service/config`, `runtime`, tool config plans and shared route editor.

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

### Tray quit must finish startup before unloading supervision
- **Symptom**: quitting from the tray could launch another desktop once.
- **Root cause**: `src-tauri/src/tray_lifecycle.rs` stopped supervision outside the runtime lifecycle guard while startup/repair could register it again; the stop helper also sent Quit to the caller's own singleton.
- **Fix**: drain startup under the lifecycle write lock, mark the runtime terminal, suspend and verify the supervisor without killing its child, then exit once. A failed unload keeps the application alive and allows retry.
- **Guardrail**: reject startup, watchdog repairs and updates after terminal quit. Exercise both the concurrency regression in `runtime_lifecycle.rs` and supervisor-unload/child-exit/no-relaunch in `autostart/imp/tests.rs`.
- **Watch points**: desktop startup, tray repair, updates, singleton exit and macOS LaunchAgent suspension.

### Display and edit must share API observations and field positions
- **Symptom**: editing replaced the header/body layout and exposed API-derived quota as editable inputs; saving could send an obsolete quota draft.
- **Root cause**: `ProviderDetailPane.svelte` had separate display and form trees, and `App.svelte` allowed quota draft fields to replace observations.
- **Fix**: share `ProviderItemFields` and credential rows across modes, keep quota cards read-only and mounted, use the latest selected observation when saving, and collapse domain/protocol overrides. Rust derives absent domains from API hosts; editing an existing exact API-host set treats it as the default.
- **Guardrail**: keep header/credential/endpoint geometry stable at 960x640; never bind API quota values to draft inputs. Keep explicit custom domains when the API changes and refresh embedded control-panel output for shared field/token changes.
- **Watch points**: ProviderDetailPane, ProviderFormFields, desktop provider_inputs, App saveProvider, ProviderItemFields and AgentAccountDetail. Agent statistics must follow the active entry/secret, never a pending switch selection, and label local request coverage.

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

### Similar plan names do not share credential or billing rules
- **Symptom**: valid Token Plan keys were rejected locally for missing a Coding Plan prefix.
- **Root cause**: `subscriptions/plans.rs::fresh` required `sk-sp-` for Qwen/Bailian Token Plan, copying a restriction from the separate Coding Plan product.
- **Fix**: stop inferring Token Plan key format from Coding Plan; preserve the fixed provider/region endpoint and key-digest ownership checks. Correct the expansion documentation and avoid claiming that the vendor has no quota API merely because AIPass has not integrated one.
- **Guardrail**: confirm credential rules against current vendor docs per billing product. Test non-prefixed Token Plan keys, each region, durable identity ACK, reconnect idempotency and changed-key refusal. Do not relabel usage-based Token Plan billing as a fixed monthly subscription.
- **Watch points**: plan specs/authentication, catalog methods, quota placeholders and localized UI copy. See the current Qwen Code authentication guide and `token_plan_keys_without_coding_plan_prefix_keep_their_region_and_identity`.

### New subscription adapters need complete protocol and allowance boundaries
- **Symptom**: a public model catalog suggested a usable free login although generation rejected it; text-tool examples could dispatch tools, scoped usage could fabricate unused bonus capacity, and Messages paths omitted `/v1`.
- **Root cause**: `subscriptions/opencode.rs:52` admitted the public key, `subscriptions/plans.rs:270` joined an SDK base as a full URL, and the Trae decoder/allowance parser did not enforce request tool policy or distinguish missing usage from zero.
- **Fix**: reject unsupported public grants after a real public-only probe; use exact Messages paths; validate tool choice, quoted examples, complete JSON and call IDs; exclude expired packs and require reported usage for positive bonus pools.
- **Guardrail**: test full request URLs, actual generation admission separately from catalog availability, all tool-choice modes, quoted text and embedded closing tags, missing/zero/expired quota, numeric tenant IDs and the earliest token expiry. Check first plain-text delivery before completion, and reject output constraints that conversion cannot preserve before dispatch. Never impersonate the vendor client to bypass a free-tier restriction.
- **Watch points**: native subscription modules, proxy-conversion provider codecs, Agent quota scopes, connection catalog/methods, regional key-page links and Tauri URL allowlists.

### Partial quota failures must preserve valid model-scoped limits
- **Symptom**: a failed Cline paid-balance request discarded successfully read ClinePass limits; model scope also depended on a naming prefix instead of catalog membership.
- **Root cause**: `subscriptions/cline.rs::usage` marked the entire result with `error`, and `community/quota.rs::native_quota` correctly rejects such failed readings wholesale.
- **Fix**: assemble independently known balance and limits without a global error when useful allowance data exists; use the catalog's subscription flag and exact IDs, retaining an explicit empty scope when no plan models are known.
- **Guardrail**: test missing and invalid balance with valid exhausted plan windows, unknown total allowance and non-prefix plan IDs. Unknown balance must stay absent, and scoped limits must never expand to all paid models.
- **Watch points**: Cline catalog/usage assembly, Agent snapshot errors and proxy exact model matching.

### Connection failures must preserve region and reveal the error
- **Symptom**: a failed international login showed China on retry; focus-induced scrolling left the error above the visible form.
- **Root cause**: `CommunityConnectDialog.svelte` cleared select inputs after login start and did not reveal newly inserted error banners. An inline reactive DOM-ref assignment can also invalidate its own dependency repeatedly.
- **Fix**: retain select values while clearing credentials; reveal new errors through a helper dependent only on the error transition, and clear credential drafts on unmount.
- **Guardrail**: test failed login with a non-default region, empty secret input and an initially scrolled form. Do not assign a DOM-ref property inside a reactive expression that reads the same ref; verify that error-state rendering settles.
- **Watch points**: provider/method switching, connection retries, key-page links, close/cancel and 960×640 dialog scrolling.

### Subscription directories and generated titles must share provider metadata
- **Symptom**: new Agent subscriptions were missing from the first connection picker; generated regional account names retained English labels and dotted titles in Chinese.
- **Root cause**: the root picker kept a second provider list, and `packages/schemas/src/account-presentation.ts` recognized generated titles only when their suffix exactly matched the ownership key.
- **Fix**: load the Agent catalog in both connection dialogs, localize descriptor keys, and split generated brand/friendly-name fields in shared presentation. Scroll the selected provider into view after catalog loading.
- **Guardrail**: add new subscriptions to the Rust catalog and dispatch together; render the full catalog without a frontend allowlist. Preserve custom titles and ownership keys while localizing generated brands. Cover catalog search, region input and friendly names in tests.
- **Watch points**: `subscriptions/{catalog.json,mod.rs}`, `community.rs`, OAuth/Community connection dialogs, account/list/detail presentation and shared locale/icon catalogs.

### Claude subscriptions must use the installed vendor CLI throughout
- **Symptom**: Claude subscriptions could only be imported; usage called Anthropic HTTP directly even though generation used real Claude Code. Missing or incompatible CLI installations were discovered only when a request failed.
- **Root cause**: `official_accounts.rs::claude_usage` had a separate token HTTP transport, while the connect dialog only offered Codex/Grok device flows and `claude_bridge::claude_binary` accepted any file.
- **Fix**: Agent-owned isolated `claude auth login --claudeai`, capability detection shared with generation, and account-scoped `claude -p /usage`; preserve CLI rotations even after a failed usage read, serialize identical grants, retain rotation receipts/recovery until lock, and CAS background updates against the complete source grant.
- **Guardrail**: never fallback to direct Anthropic auth/usage HTTP or an unrelated native account. Reject missing/unusable/unsupported CLI installations with install/update and recheck actions. Test canceled/locked/late logins, failed save retries, grant-only rotations, timeout cleanup, unknown output, model windows, IANA dates and year rollover; verify both locales/themes at 960×640. Preserve old quota windows as stale on CLI errors.
- **Watch points**: `claude_cli.rs`, `claude_bridge.rs`, `official_accounts.rs`, typed protocol, session lock, Tauri browser helper, `ClaudeConnectPane` and `OAuthConnectDialog`.

### Claude account identity includes its organization
- **Symptom**: multiple Claude seats sharing an email collapsed into one entry, and a same-email login could replace another organization during reauthentication.
- **Root cause**: `claude_cli/login.rs::signed_in` and `native_auth.rs::NativeStore::identity` extracted email alone; persistence and native switches trusted that shared identifier.
- **Fix**: share `native_auth::claude_identity` across CLI status/profile reads and native stores; scope identity by organization UUID and reject conflicting metadata. Upgrade legacy entries only from their pinned device-local profile, preserving entry/secret IDs; normalize old tool bindings against that upgraded entry.
- **Guardrail**: never match scoped Claude identities by email alone. Test same-email distinct organizations, missing/changed organization metadata, mismatched status/profile, wrong-device legacy records, targeted reauthentication and unchanged sibling bindings. Keep upgrades out of proxy snapshot reads.
- **Watch points**: `official_accounts/identity.rs`, `claude_cli/login.rs`, `tool_switch/{identity,login,native,restore}.rs`, subscription import and native backups. Regressions live in the shared identity, official-account and tool-switch tests.

### Default Claude profiles and login homes need the same ownership boundary
- **Symptom**: the default terminal account could disagree with native switches on its organization; canceled logins retained private directories and four completed tickets blocked a fifth account.
- **Root cause**: `account_environment` always set `CLAUDE_CONFIG_DIR`, moving the default profile inside the directory, while native stores read the parent profile. Login directories were persistent before a save was attempted, and the cap counted saved tickets.
- **Fix**: share `claude_identity::profile_path`, leave the default config-directory environment unset, reject malformed profiles, and own new login directories through RAII until persistence is attempted. Keep ambiguous failed writes recoverable; exclude completed receipts from the pending-login cap.
- **Guardrail**: test default/custom profile identity agreement, cancel cleanup, four pending versus four saved tickets, and a failed commit followed by cancellation. Never delete a CLI home after an ambiguous vault write or delete borrowed vendor homes.
- **Watch points**: `claude_cli::{account_environment,ConfigDir}`, `claude_cli/login.rs`, native stores and legacy upgrades. Scope reconnect upgrades to the matching organization and refresh runtime readers after background migration, even with quota tracking disabled.

### Subscription wire and quota contracts must not fork between transports
- **Symptom**: CLI-owned Codex requests advertised 0.116 while the proxy used 0.159, used obsolete underscored session headers and exposed hidden catalog models. Claude family-specific exhaustion never affected routing.
- **Root cause**: `subscriptions/cli_accounts/backend.rs` duplicated Codex headers/catalog URLs; `proxy_service::account_quota` dropped model-specific Claude windows.
- **Fix**: reuse the proxy's Codex header/catalog helpers, send `session-id` and `thread-id`, and carry known Claude family scopes into `QuotaWindow` matching.
- **Guardrail**: test header/catalog version agreement across both paths and preserve same-session cache keys. Family exhaustion must affect only matching Claude models, honor freshness/reset bounds, and never turn unknown scopes into account-wide limits.
- **Watch points**: `aipass-proxy::codex`, CLI subscription backend, `proxy_service/routing.rs`, `ResolvedTarget::quota_usage`, direct HTTP, downstream WS bridges and fixed-model groups.

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


### Refresh cancellation and IO recovery must not spend a grant twice
- **Symptom**: cancellation could drop a rotated token before durable ACK; bare 403s invalidated good sign-ins; mirror failures left proxy snapshots stale or retried an already-spent refresh grant.
- **Root cause**: native adapter cancellation covered credential rotation; official refresh wrote a CLI mirror before vault authority; persistence failures had no generation-bound recovery and proxy reads trusted the secondary secret.
- **Fix**: shield bounded refresh/ACK, preserve failed saves in Agent recovery caches with lock revocation, read managed OAuth authority for the linked primary key, reconcile mirrors idempotently, classify explicit grant rejection and keep original token expiry. Prefer Kiro CLI-owned rotation and bind its previous native generation to both tokens; retry unsaved Claude native grants before session expiry.
- **Guardrail**: test canceled upstream rotation, failed/ambiguous ACK, read-only vault IO, new-login/revision conflicts, omitted tokens, unchanged cached access with a rotated refresh token, expiry after delayed persistence, and concurrent Copilot exchange. Do not fallback after a rotated grant's save fails or extend its lifetime on retry.
- **Watch points**: `oauth/mod.rs`, `community.rs`, native subscription fresh/organization repair paths, `claude_bridge.rs`, `proxy_service.rs`, proxy `copilot.rs`. See `docs/oauth-refresh-review.md`.

### Native rotation may change only the refresh token
- **Symptom**: Claude skipped saving a refreshed native grant when access was unchanged; Kiro ignored a valid native rotation with equal expiry. A stale Claude process could overwrite another process's refresh-only rotation.
- **Root cause**: Native refresh detection compared access tokens or required a strictly later expiry; the Claude write CAS bound only the access mirror.
- **Fix**: Compare Claude's complete native grant and CAS its canonical hash, allow exact durable replays, protect pending grants from capacity eviction, and adopt Kiro's changed live pair except known previous native generations.
- **Guardrail**: Test refresh-only changes, equal/shorter live expiry, stale writers, durable replay and pending credential retention. Bind rotation detection and CAS to the complete grant rather than access alone.
- **Watch points**: `claude_bridge::persist_native`, `persist_native_account`, `handlers::ClaudeNativeWrite`, typed IPC, Kiro `live_native_rotation` and native readers.

### Exchanged device logins outlive the authorization challenge
- **Symptom**: delayed login saves extended access expiry or discarded an already exchanged refresh grant at the device-code deadline; optional mirrors could trigger replay of a login already saved.
- **Root cause**: `oauth/mod.rs` retained only relative token lifetime and expired exchanged bundles with their challenge; `handlers::complete_oauth_login` wrote CLI files before managed authority and propagated later mirror failures.
- **Fix**: capture absolute access expiry once, retain exchanged bundles until durable save/cancel/lock, store complete managed grants before optional CLI mirrors, and treat mirror failures as repairable after login succeeds.
- **Guardrail**: test failed saves past both deadlines, successful exchanges at the challenge deadline, exact expiry on retry, fresh/reauthorized grants durable before mirrors, and proxy reload failure without login replay.
- **Watch points**: `OAuthManager::poll_with`, `register`, `complete`, `handlers::complete_oauth_login_with`, refresh recovery and native mirror repair.

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
- **Guardrail**: validate advanced retry fields before saving and surface every failed route-config write in the open dialog. When disabling an option, retain valid persisted/default numbers instead of submitting invalid hidden inputs. Covered by `RouteGroupDialog.test.ts`. When only renaming or reordering a group, retain each existing target's private headers, base URL, group, protocol and WebSocket preference; do not rebuild it from provider metadata.
- **Watch points**: `RouteGroupDialog.svelte`, `App.svelte` `saveRouteGroup`, and agent `validate_config`.

### Lock and save outcomes must own the active dialog lifecycle
- **Symptom**: OAuth accounts stayed over the lock screen, settings errors appeared behind the drawer, a failed close hid unsaved settings, and save retries left an empty editor or duplicated providers.
- **Root cause**: `App.svelte:1098` omitted OAuth from lock cleanup; settings closed before awaiting persistence; provider saves inferred success from a shared stale error and had no operation-level exclusion.
- **Fix**: gate OAuth on the unlocked workspace; keep settings open until save succeeds with feedback inside the drawer; return explicit provider save results and guard in-flight writes. Preserve drafts from late settings reloads.
- **Guardrail**: unmount every sensitive dialog on lock. Keep failed mutations visible with their draft, close on explicit success, and distinguish committed writes from refresh failures. Test actual App-to-dialog paths, including late list responses, close-save failure and repeated submits at 960×640.
- **Watch points**: `App.operations.test.ts`, `IntegrationCard.test.ts`, inline/provider-modal saves, settings close/Escape/outside events, OAuth account loading and login cancellation.

## Shared language settings

### Desktop, tray and extension could drift to different languages
- **Symptom**: changing the desktop language did not update the tray or an already-open extension popup, and equivalent actions used different translations.
- **Root cause**: the desktop stored its preference locally while the extension followed browser language and tray strings were hard-coded in Rust/Swift; each surface also kept separate message copies.
- **Fix**: store the preference through the authenticated agent, return one resolved system locale, expose it through native-host ping, and load one shared English/Chinese catalog with aliases for synonymous messages.
- **Guardrail**: route every surface through `LanguageSettingsGet/Set`; test first-run legacy migration, live extension polling, catalog key/interpolation parity, and the native tray render and 960×640 settings layout.
- **Watch points**: `crates/aipass-agent/src/language.rs`, `crates/aipass-agent-protocol/src/lib.rs`, `apps/desktop/src/App.svelte`, `apps/desktop/src-tauri/src/tray.rs`, Swift tray DTO/view, native-host ping, and `packages/ui/src/locales/`.

## Desktop operation feedback

### Agent navigation must retain the last completed scan
- **Symptom**: every return to Agent management replaced both panes with a full loading state.
- **Root cause**: `AgentManagementPage.svelte` owned detections/statuses inside the component that navigation destroys.
- **Fix**: keep a presentation-only snapshot in the unlocked workspace, initialize remounts from it, and scan in the background. Separate first-load UI from refresh activity; keep the completed snapshot on detection failure.
- **Guardrail**: clear the snapshot with unlocked workspace state, reject responses after teardown, coalesce overlapping refreshes, and disable mutation controls while refreshing. Cover cached empty lists and lock during a pending scan in `AgentManagementCache.test.ts` and `App.operations.test.ts`.
- **Watch points**: Agent page mount/focus/manual refresh, selected tool fallback, preview/apply refresh, and `App.svelte::clearSensitiveUnlockedState`.

### Shared errors leaked into the selected provider
- **Symptom**: import, settings and other operation failures appeared as a permanent error banner on the provider detail page.
- **Root cause**: `App.svelte:3331` forwarded its shared `errorText` into `ProviderDetailPane.svelte:523`, independent of which operation failed; closing a dialog exposed its stale error behind it.
- **Fix**: remove the detail page's generic error prop, route ordinary failures into a dismissible, expiring toast, and scope inline auth, provider-form and settings feedback to their own surfaces. Dialogs that already handle rejected callbacks own their errors without a duplicate host notification.
- **Guardrail**: report operation errors through `reportError`; never pass app-wide errors to an entity detail page. Test unrelated errors during editing, repeated identical failures, auto-dismiss and closing a failed settings surface in `App.operations.test.ts`. Preserve drafts on failure and clear workspace toasts on lock.
- **Watch points**: App operation handlers, provider inline/modal saves, settings feedback, integration/usage dialogs, and `ErrorToast.svelte` at 960×640.

## Desktop settings layout

### Automatic saves must retain the newest draft and finish before closing
- **Symptom**: retry fields depended on another card's save button; changing to automatic saves could overwrite a newer edit with an earlier response or discard pending edits on close.
- **Root cause**: `SettingsPanel.svelte:227` mutated the host's whole proxy configuration without persisting individual edits; `App.svelte:2273` rejected writes while busy and replaced the shared draft on completion.
- **Fix**: keep a settings draft and serialize its latest revisions, wait for writes before close, and retain failed writes for explicit retry. Save listener controls on completed changes, wait for both certificate files, and preserve immediate stop independently of validation. Ignore stale responses and queued edits after lock/unmount; distinguish committed config writes from status refresh failure.
- **Guardrail**: test rapid edits, close during a pending write, incomplete custom proxy/pricing/certificate inputs, invalid numbers, write retry, lock during a write and failed post-commit refresh. Keep file imports retryable, clear selected files on success/unmount, and zeroize the transient private-key string.
- **Watch points**: `SettingsPanel.svelte`, `ControlPanelSettings.svelte`, `App.svelte` mutation/load guards, `App.operations.test.ts` and `ControlPanelSettings.test.ts`.

### Empty fixtures concealed clipped controls and inconsistent card content
- **Symptom**: Server settings had flush card content and native checkboxes; populated pricing rules clipped their delete buttons, and long group names overlapped proxy actions.
- **Root cause**: `ControlPanelSettings.svelte:139` duplicated control styling without horizontal padding; `SettingsPanel.svelte:1462` required more column width than the settings drawer provides. Header badges retained their full intrinsic text width.
- **Fix**: Reuse shared controls with explicit 16px content padding, place pricing values in two columns, and constrain badge text while keeping header actions on their own row in narrow panes.
- **Guardrail**: Review at 960×640 with populated pricing, long group names, running/stopped states and expanded certificate forms in both languages and themes. Check child bounds against card edges; a parent's hidden overflow can conceal clipped controls despite equal scroll and client widths. Use content-sized shared buttons when adding a text label to an icon action; verify English labels remain on one line. Keep immediate stop independent of invalid listener drafts.
- **Watch points**: `ControlPanelSettings.svelte`, `SettingsPanel.svelte`, `ServerDetailPane.svelte`, `ProviderDetailPane.svelte` credential pricing, shared `Card`, `Field`, `Button` and `SwitchField`, and the control-panel behavior regressions. Center empty states in the available list area using shared `EmptyState`; validate independent group scrolling and shared switches at 960×640. Keep legacy global button rules from overriding shared controls' dimensions, alignment and hover states.

## Build toolchain

### Every release job must stamp its own checkout
- **Symptom**: the nightly CLI artifact reported `0.2.0-beta.1` from `--version`; the SBOM generation path also retained the repository version.
- **Root cause**: `.github/workflows/release.yml` stamped the desktop job's checkout, but independent CLI and publication jobs checked out the tag without applying its version.
- **Fix**: stamp the resolved release version before CLI compilation and SBOM generation. Execute each platform's finished CLI and require its reported version to match before packaging.
- **Guardrail**: treat every job checkout as independent. Verify compiled CLI versions and SBOM metadata against the resolved tag; changing the archive name does not change the embedded version.
- **Watch points**: desktop, CLI matrix, extension and publish jobs; `scripts/set-release-version.mjs`, `scripts/set-extension-release-version.mjs`, and `scripts/release-metadata.mjs`.

### CI silently omitted the DMG installation layout
- **Symptom**: published DMGs opened without the configured background or icon positions; the original 1x PNG also looked soft on Retina displays.
- **Root cause**: `.github/workflows/release.yml` set `CI=true`, which makes Tauri pass `--skip-jenkins` to create-dmg and skip saving Finder's layout. The background generator downsampled everything to 660×400 pixels.
- **Fix**: set `TAURI_BUNDLER_DMG_IGNORE_CI=true` in both macOS workflows and generate a TIFF with 1x/2x representations at the same logical size.
- **Guardrail**: preserve the configured packaging appearance. Per the 2026-10-27 user instruction, automated DMG validation checks only image integrity, read-only mounting, app metadata and executable payloads; do not restore Finder/background/icon/Retina presentation tests. Keep application startup/update and production signature/notarization gates.
- **Watch points**: both workflows, `tauri.conf.json`, the background generator, and the local macOS bundle gate.

### Homebrew Rust's objcopy dependency produced misaligned macro libraries
- **Symptom**: macOS 27 Tauri release builds failed to load procedural macros with `mis-aligned LINKEDIT string pool`, including after rebuilding their caches.
- **Root cause**: Homebrew's Rust 1.98.0 formula links `lib/rustlib/<host>/bin/rust-objcopy` to LLVM 22's `llvm-objcopy`, whose debug-info stripping has the Mach-O alignment defect in [rust-lang/rust#157750](https://github.com/rust-lang/rust/issues/157750). LLVM 23.1.0 contains [the alignment fix](https://github.com/llvm/llvm-project/pull/203680). Both `scripts/build-desktop-sidecars.mjs:15` and Tauri inherit the same compiler and dependency.
- **Fix**: keep the latest Homebrew `rust`, upgrade Homebrew `llvm` to 23.1.0, and point only the Rust sysroot's `rust-objcopy` symlink to `$(brew --prefix llvm)/bin/llvm-objcopy`. Keep Rust's LLVM 22 library dependency unchanged. The same stripped-library reproducer fails with LLVM 22 and loads with LLVM 23; rebuild affected cached macros after the repair.
- **Guardrail**: use Homebrew-managed Rust locally per the user's preference; verify Cargo/rustc provenance and the actual objcopy link before macOS validation. Recheck a freshly compiled stripped library after Rust upgrades/reinstalls, which can recreate the formula's original symlink. Do not replace the LLVM 22 library with LLVM 23 or switch to rustup to bypass the issue.
- **Watch points**: desktop development, `scripts/build-desktop-sidecars.mjs`, Tauri release builds, and the local macOS validation shell.


### Embedded styles must not depend on the checkout directory
- **Symptom**: the Node CI job passed tests and build but failed the committed embedded-output comparison; local macOS rebuilding produced no diff.
- **Root cause**: `apps/control-panel/vite.config.ts` used Svelte's default CSS hash for shared components outside the Vite root, including the absolute filename in their scope identifiers.
- **Fix**: hash the repository-relative, separator-normalized filename together with the CSS. Rebuilt assets match byte-for-byte in two independent checkout directories.
- **Guardrail**: keep the CI embedded-output comparison enabled and verify both JS and CSS when changing shared component compilation. Never patch only the generated scope identifiers or accept machine-specific output. Resolve shared schemas directly to the fingerprinted source instead of an ignored, possibly stale `dist` directory.
- **Watch points**: control-panel Vite config, shared UI components, `scripts/stamp.mjs`, Agent `build.rs`, and the Node CI reproducibility step.

### Embedded fingerprints require LF rules for every shared source
- **Symptom**: Windows CLI releases failed with a stale-panel assertion while macOS and Linux passed.
- **Root cause**: `apps/control-panel/scripts/stamp.mjs` added schemas inputs, but `.gitattributes` only protected panel and UI sources. Windows converted schemas files to CRLF, changing the raw hashes checked by `crates/aipass-agent/build.rs`.
- **Fix**: protect schemas sources and package metadata with `text eol=lf`. The repository gate checks Git attributes for every fingerprinted text input, with a regression for missing shared-source rules.
- **Guardrail**: add LF checkout rules whenever adding fingerprinted text inputs. Run `pnpm repo:check` and verify hashes in an `autocrlf=true` checkout; keep binary assets exempt and retain Cargo's stale-asset rejection.
- **Watch points**: `.gitattributes`, panel stamp inputs, Agent build inputs, `scripts/repo-health.mjs`, and Windows CLI release builds.

## LAN control panel

### Credential navigation must share desktop categories and scope selection
- **Symptom**: The panel flattened providers under one Credentials entry, ignored desktop favorites/recent/provider kinds, and kept details selected outside the active category.
- **Root cause**: The remote snapshot omitted favorite and lifecycle metadata; panel navigation and list filtering were maintained separately.
- **Fix**: Share `VaultSidebar`, `ProviderListPane`, provider counts and filter helpers. Expose only public grouping metadata with RFC3339 timestamps through the authenticated Agent snapshot, including archive/trash views.
- **Guardrail**: Count only active providers in ordinary categories; sort Recent by use time. Scope search and selection to the chosen group and clear stale previews when polling removes a selected entry. Exclude archived/trashed records from proxy choices and tool actions. Bundle provider icons from the shared UI assets; never reference desktop `/src` URLs in a production surface. Use masks for monochrome SVGs to preserve theme colors. Verify categories, counts, empty groups, icons and scroll masks in both languages/themes at 960×640.
- **Watch points**: Shared schema provider helpers, desktop counts/search, panel App tests, Agent snapshot metadata tests and read-only list capabilities.

### Shared controls must own their appearance and pending layout
- **Symptom**: The panel's sidebar icons, native selects and action buttons differed from desktop; clicking Preview changes shifted the entire form and resized its button.
- **Root cause**: `apps/control-panel/src/style.scss` duplicated global button/input/select/svg rules while `App.svelte` inserted a generic pending paragraph. Native modal dialogs also block SelectField portals outside the browser top layer.
- **Fix**: Reuse SidebarItem, SearchField, Button, IconButton, SelectField, Field, SwitchField and the shared Bits UI Modal. Preserve button content dimensions during loading; discard pending previews when groups change, retain failed editor drafts, and return modal focus to the opening control.
- **Guardrail**: Keep surface CSS scoped to layout; let common controls own sizing, padding, icon strokes, hover and focus states. Verify pending button bounds, keyboard selection, modal portal hit testing, form-associated footer submission, failure retry and focus return at 960x640 in both languages and themes. Constrain Select grid tracks and triggers to min-width zero; keep popup width tied to its anchor and test long labels for viewport overflow. Preserve elevated confirmation layers above existing dialogs. Synthetic Select tests must dispatch pointerdown on triggers and pointerup on options.
- **Watch points**: Shared UI controls, desktop ConfirmModal, panel ProxyLogsDialog and App.test.ts; embedded panel build/stamp and Agent source fingerprint.

### Plain and rich selects must share their popup appearance
- **Symptom**: Generic select options and credential options used different radii, spacing, selected and keyboard-highlight states.
- **Root cause**: `SelectField.svelte` and desktop `CredentialPicker.svelte` each owned the complete popup skin and viewport sizing.
- **Fix**: Share `packages/ui/src/styles/select-menu.scss` through the UI base stylesheet; retain only rich-content layout in CredentialPicker. Reserve an indicator column and distinguish the selected option from the currently highlighted option.
- **Guardrail**: Use the shared select-menu classes for popup containers, viewports, options and indicators. Bound height to Bits UI's available space; verify keyboard navigation, long-label ellipsis, scrolling and modal hit testing at 960x640 in both themes.
- **Watch points**: SelectField, CredentialPicker, shared base styles and embedded panel output.

### Shared group management must retain Agent authority and snapshot ordering
- **Symptom**: The panel could only toggle existing groups; copying the desktop draft would expose private routing fields, and an older poll could leave a successful save showing stale groups.
- **Root cause**: The panel maintained its own layout and received only a sanitized snapshot, while desktop route configuration includes tokens, endpoints and headers.
- **Fix**: Share the list, editor, empty state, cards, status grid and schema helpers. Submit a strict editable group DTO with the opening revision; resolve new credentials and preserve existing private target fields inside the Agent. Await an older poll and then fetch the committed snapshot.
- **Guardrail**: Reject stale revisions and injected private fields; capture the revision when an editor opens. Verify create/edit/delete, deletion of the last running group, overlapping polls, authorization-loss portal removal, and credential redaction. Keep supported authentication in eligibility, configuration validation and runtime resolution aligned, including Google API keys. Use cryptographic `getRandomValues` for UUIDs when HTTP LAN contexts omit `randomUUID`; verify creation through a non-loopback HTTP origin.
- **Watch points**: Shared `ProxyRouteList` / `ProxyRouteGroupDialog`, panel `App.test.ts`, protocol `ControlPanelRouteDraft`, Agent HTTP management tests and `proxy_service` auth validation tests.

### Remote authorization must follow the current vault session
- **Symptom**: checking a browser session only when a request arrives can allow queued work to run after a local lock/unlock, password change, or access-code rotation; a code tied only to a directory can authorize a replacement vault.
- **Root cause**: HTTP authentication and the actual vault operation occur at different lifecycle boundaries (`crates/aipass-agent/src/control_panel/transport.rs`, `session.rs`).
- **Fix**: bind access-code verifiers to the vault UUID, bind browser sessions to the unlock instance and password envelope, and recheck scoped authorization under the vault session mutex. Keep background polling from touching either activity timer.
- **Guardrail**: run `cargo test -p aipass-agent control_panel` when changing session replacement, password/sync reload, code rotation, or HTTP dispatch. Retain the replacement-vault, in-flight revocation, expiry, and proxy-continuity assertions.
- **Watch points**: `control_panel/{mod,sessions,transport,tests}.rs`, `session::{with_vault,with_vault_mut,touch_session}`, and `Vault::password_revision`.

### Remote unlock must activate through the shared session lifecycle
- **Symptom**: requiring an unlocked host makes remote management unusable after lock/restart; a separate unlock path can skip proxy restoration or accept a revoked queued login.
- **Root cause**: the original panel login only inspected `SessionState::Unlocked` in `control_panel/transport.rs`; post-unlock work lived inside password-only `session::unlock_with_password`.
- **Fix**: locally opt in when generating a new random code, seal the root key under a domain-separated key derived from the raw code, validate UUID/password revision, and share `session::complete_unlock`. Serialize authorization and session installation with session→panel locks and reject old listener identities/generations.
- **Guardrail**: never derive the wrapping key from the persisted verifier. Keep ordinary codes unable to unlock. Run vault `remote_unlock` and Agent `control_panel` tests, including locked restart, proxy restore, password revision changes and revocation before activation.
- **Watch points**: `aipass-vault/src/remote_unlock.rs`, `aipass-crypto::derive_remote_unlock_key`, `control_panel::ControlPanel::login`, local code rotation/revocation, desktop grant controls.

### Disabling remote access must bypass listener form validation
- **Symptom**: a remote-access checkbox only changed a form draft; incomplete certificate files or invalid listener fields could block saving the disabled state.
- **Root cause**: the checkbox in `ControlPanelSettings.svelte` shared the listener configuration form and its validation instead of calling the dedicated stop operation.
- **Fix**: use an immediate switch backed by Agent status; call `control_panel_stop` directly when disabling, preserve unrelated drafts, and roll back a failed enable to the saved state.
- **Guardrail**: keep stopping available independently of access-code, address, port and certificate validation. Verify immediate stop with invalid drafts, failed-enable rollback, and disabled persistence with the control-panel component and Agent tests.
- **Watch points**: `ControlPanelSettings.svelte`, `panel_commands::control_panel_stop`, `ControlPanel::{stop,restore}`, tray Stop.

### Authentication primitive tests must use the actual stored credential
- **Symptom**: The new proof verifier rejected the value returned by `compute_verifier`, although the round-trip test passed.
- **Root cause**: `aipass-crypto/src/remote_auth.rs` generated proofs with the auth secret and verified with a different HMAC key; the test supplied the auth secret instead of the computed verifier. Comments incorrectly described this as SRP or zero-knowledge authentication.
- **Fix**: Derive one domain-separated proof key for both paths; explicitly document that the verifier is a secret authentication credential. Use zeroizing key buffers, the dependency's safe consuming ECDH API, and reject non-contributory peer keys.
- **Guardrail**: Test the exported derive, verifier, proof and verify sequence, wrong keys, changed nonce, changed or reversed public keys and low-order peers. Encrypt verifiers at rest; never use them as vault wrapping keys. Protocol callers must authenticate the server and expire and consume challenges once.
- **Watch points**: `aipass-crypto/src/{ecdh,remote_auth}.rs`; these primitives are not yet wired into the existing control-panel login protocol.

### Stop must finish releasing the listener before returning
- **Symptom**: an immediate stop/restart intermittently failed to bind the saved port during the full workspace test suite.
- **Root cause**: `control_panel/transport.rs` signaled shutdown but returned while the accept thread still owned a cloned listening socket.
- **Fix**: retain and join the listener thread after revocation and stop signaling; keep Tokio's blocking-task shutdown bounded. The regression repeatedly restarts while a client holds incomplete HTTP headers.
- **Guardrail**: require stop to release its listening socket and close incomplete connections before reporting success. Run `cargo test --workspace` with the listener restart regression enabled.
- **Watch points**: `transport::Listener::drop`, `Prepared::launch`, `ControlPanel::{configure,stop,shutdown,restore}`.

### Community credential rotation, native history and model-scoped quota
- **Symptom**: A caught auth-write failure could let a plugin continue with an uncommitted rotated refresh token; native Messages converted through Chat lost signatures; a model-only quota could exhaust the whole account.
- **Root cause**: The former plugin catch boundaries, `aipass-proxy/src/subscription.rs` protocol round trips, and quota selection without an effective model represented different ownership scopes as account-wide state.
- **Fix**: Stop the native Rust adapter on rejected durable ACK, retain native Messages/Responses, bind Gemini signature history to account generations, and filter quota windows by effective model/exclusions. Preserve the previous quota snapshot while an account is busy.
- **Guardrail**: Run `cargo test -p aipass-agent subscriptions` and `cargo test -p aipass-proxy-conversion`, native subscription codec tests, and `quota_scope_follows_the_effective_model_and_preserves_unrelated_models`; never use a rotated credential before a successful Agent CAS ACK.
- **Watch points**: Native-file rereads/writeback, login cancellation versus account commit, credential refresh and target retention, model overrides, advisory quota windows, HTTP/WS conversion.

### Subscription converters belong in Rust
- **Symptom**: External subscription adapters introduced an unnecessary Node runtime and embedded plugin loader in the desktop distribution.
- **Root cause**: Letting external adapter packages define implementation ownership and runtime architecture.
- **Fix**: Implement provider auth/transport in `aipass-agent/src/subscriptions`, and pure request/SSE/Connect/AWS codecs in `aipass-proxy-conversion/src/providers`; remove the Node worker, loader and vendored JavaScript.
- **Guardrail**: Base compatibility on vendor protocols and official CLI behavior; keep implementation and architecture in AIPass. Verify HTTP/2 duplex, early quota failures, incomplete streams, bounded buffers, cancellation and durable credential acknowledgements in Rust. Vendor authentication CLIs are distinct from converter runtimes.
- **Watch points**: Model-specific wire selection, API/region headers, one-use refresh grants, signed reasoning, native-file ownership and model-scoped quota.

### Stream retry safety must not depend on network chunk boundaries
- **Symptom**: Content and a quota error in one network chunk could discard the content and turn an already submitted generation into a retryable HTTP rejection. Malformed HTTP 200 bodies were also incorrectly treated as explicit upstream 502 refusals.
- **Root cause**: `aipass-agent/src/subscriptions/mod.rs` and `subscriptions/cursor.rs` checked only delivered bytes, while a decoder can consume content before returning an error for a later event in the same chunk.
- **Fix**: Provider decoders retain generation progress; only explicit upstream refusals supply a retryable status. Unknown, truncated and already producing attempts propagate an error without replay. Shared proxy silent-retry buffers also detect content, reasoning, tool declarations and completed generations before allowing failover.
- **Guardrail**: Test coalesced content-plus-error and malformed/empty successful HTTP responses, alongside early quota rejection. Validate complete tool argument objects in native decoders before sending completion; same-protocol SSE deliberately preserves upstream events.
- **Watch points**: Connect, AWS, JSON-lines and Qoder codecs; Cursor duplex; `ProviderStream::has_output`, `failure_status`, and proxy submitted-attempt handling.

### New IPC variants must preserve existing wire discriminators
- **Symptom**: Inserting community variants below the `official_accounts.refresh` serde attribute attached that existing operation to the catalog request; other new subscription operations lacked stable wire tags.
- **Root cause**: Enum insertion in `aipass-agent-protocol/src/lib.rs` separated serde attributes from the variants they described.
- **Fix**: Assign explicit tags to every subscription/runtime variant and restore the official refresh tag. Test wire tags against operation names, round trips, and existing raw JSON dispatch.
- **Guardrail**: Keep attributes with their variants and test existing wire JSON whenever adding IPC operations. Typed Rust round trips alone cannot catch a coordinated but incompatible rename.
- **Watch points**: Agent request enum, event names, desktop/CLI/native-host and management API clients.

### Subscription records and credential ownership must be committed together
- **Symptom**: Initial login used two vault writes, leaving an incomplete provider if interrupted. Cached account metadata could mask a changed JWT subject, while adding legitimate profile metadata could falsely look like an account switch.
- **Root cause**: `community::persist` wrote the account bundle after creating its provider; `ensure_owner` compared only the first available identity, mixing profile IDs, email and JWT subject namespaces.
- **Fix**: Create the provider and bundle in one encrypted record. Compare established metadata fields and token claims independently; require a remaining identity anchor when a token loses its claims.
- **Guardrail**: Preserve first-write credential completeness; test metadata enrichment, JWT subject changes with unchanged cached metadata, and identity removal before granting a durable ACK.
- **Watch points**: `Vault::add_provider_with_runtime_extension`, provider edits, native credential imports and all refresh paths.


### Desktop setting dialogs must clear drafts without resetting destroyed UI state
- **Symptom**: The subscription settings dialog failed to open again after cancellation; Svelte reported `derived_inert` during teardown.
- **Root cause**: Resetting the reactive `options` object in the dialog's `onDestroy` invalidated bindings while its effects were being destroyed.
- **Fix**: Isolate the editor in `ProviderRuntimeDialog.svelte`, clear draft secret fields through a plain helper during teardown, and keep summary requests guarded by a generation counter.
- **Guardrail**: Clear sensitive draft fields on teardown without assigning reactive editor state; verify cancel-and-reopen and preserve failed-save drafts. Let portalled select focus handlers settle between synthetic pointer actions.
- **Watch points**: `ProviderRuntimeDialog.svelte`, `ProviderRuntimePanel.svelte`, shared `SelectField`, `Card`, and `ProviderRuntimePanel.test.ts`.

### Disclosure panels must preserve forms and independent header actions
- **Symptom**: Certificate import, advanced provider fields, billing references, cards and diagnostic details used separate disclosure implementations with inconsistent hit areas and keyboard behavior.
- **Root cause**: Native details and copied grid animations were maintained independently across desktop, extension and control-panel UI.
- **Fix**: Share `packages/ui/src/components/Collapsible.svelte`; retain mounted content behind `inert` and `aria-hidden`, bind expansion state, and place header actions outside the toggle button.
- **Guardrail**: Preserve file selections and drafts across collapse; verify external closure restores focus, unique content IDs, disabled toggles, independent actions, and runtime-panel lazy loading. Keep legacy slot forwarding snippets scoped to the component rather than hoisted callbacks. When replacing disclosure markup, update both component and App integration tests to use the named toggle and assert `aria-expanded`; search for stale `details`/`summary` selectors and run the affected App tests before pushing.
- **Watch points**: Desktop `Card`, `ControlPanelSettings`, `SubscriptionAccountCard` account identifiers, `ProviderItemFields` inline advanced fields, credential billing, OAuth diagnostics and pricing history; `Collapsible.test.ts`, `ProviderDetailPane.test.ts`, `App.test.ts` and certificate/billing draft regressions. New detail views must reuse the same component rather than reintroducing native `details`.


### Native subscription references must invalidate cached credential bindings
- **Symptom**: A CLI account/home switch could reuse a cached Claude process, legacy managed OAuth could still forward, and quota scope could exhaust unrelated Copilot models.
- **Root cause**: Stable routing markers did not encode a changed native binding; proxy fallback still selected managed grants; premium billing scopes were missing from model metadata.
- **Fix**: Let official CLIs own the five native subscriptions, store device/account/home references, rotate markers only on changed bindings, block legacy grant forwarding, and scope premium quota using explicit model billing multipliers. Keep CLI renewal noninteractive and bounded.
- **Guardrail**: Preserve entry/secret IDs during handoff; prefer matching CLI stores and reject foreign populated homes. Test reconnect generations, native rereads, device/workspace changes, CLI RPC, and every new IPC wire tag. Status insertion must preserve the adjacent Claude serde tag.
- **Watch points**: `official_accounts`, `subscriptions/cli_accounts`, `community`, `claude_bridge`, `Vault::bind_cli_subscription`, `subscription.cli.status`, local proxy route restoration and 960×640 subscription dialogs.

### Provider identity must reach every shared brand icon
- **Symptom**: Subscription picker cards and proxy target rows showed initials even though a built-in brand asset existed.
- **Root cause**: Callers passed editable account titles but omitted `providerId`; the shared resolver intentionally does not infer identity from titles. CLI aliases and additional subscription brands were also absent from the registry.
- **Fix**: Pass canonical identity through picker, connection, list/detail and control-panel target rows; bundle local brand assets and explicit aliases in the shared registry.
- **Guardrail**: Test all subscription catalog IDs with custom account titles, as well as picker cards and connection views. Keep assets local and monochrome masks theme-aware. Rebuild schemas before consumer checks and rebuild the embedded panel after shared source changes.
- **Watch points**: Shared `ProviderIcon`, schema icon map, subscription dialogs, desktop lists/details and control-panel target rows.

### CLI ownership includes default vendor stores and child process lifetime
- **Symptom**: A default Claude account could become invisible after setting its config home; Claude inference omitted the global proxy; a pending code submission disabled a later login. A Factory icon also contained markup outside its SVG root.
- **Root cause**: Default and custom Claude homes use different macOS Keychain names; the process-backed transport bypassed HTTP proxy defaults; abandoned UI generations retained busy flags; icon presence tests did not verify asset structure. Reaped child PIDs were also signaled again during cleanup.
- **Fix**: Preserve the default secure-store selector, use CLI status instead of reading Claude grants, discover NVM binaries and provide their Node PATH, inherit or override outbound proxy for both inference and quota, reset abandoned login flags, and skip signals for reaped children. Remove retired grant persistence code, repair the SVG, and separate CLI IPC handlers/types.
- **Guardrail**: Verify default/custom command environments, credential-free account discovery, pending-submit/restart, reconnect capability revocation, proxy precedence and HTTP-only subscription transport. Parse bundled SVGs and test root/clipping integrity. Rebuild the embedded panel after shared asset/locale changes. The native Rust gate must also reject TS, Python and shell subscription implementations.
- **Watch points**: `claude_cli::{account_environment,configure}`, `claude_bridge::Run`, `subscriptions::native_cli::Running`, proxy runtime resolution, CLI IPC handlers, `ClaudeConnectPane`, brand SVG assets and `repo-health.mjs`.

### Managed Agent switches must preserve the outgoing credential before writing
- **Symptom**: Switching a local Agent could overwrite an unimported API or select a new Gemini API key while retaining Google login. Desktop and Agent also used different OpenCode spellings.
- **Root cause**: Configuration writes alone do not preserve credential ownership or select a vendor authentication mode; serde's kebab-case spelling differs from the desktop tool ID.
- **Fix**: Use the typed `tool_config.managed_apply` path with a required preview fingerprint, encrypted source snapshots and non-overwriting vault imports. Retain native CLI account homes before changing selection, and stop on unsupported imports. Set Gemini's authentication selection alongside its API configuration; preserve the existing Agent `open-code` wire tag while bridging desktop `opencode`.
- **Guardrail**: Test stale/missing previews, import failure with unchanged files, API deduplication, independent native homes, OpenCode auth.json, selected-provider ordering and effective per-secret authentication. Check the page and confirmation dialog at 960×640; never return source credentials to Svelte.
- **Watch points**: `tool_credentials`, `tool_switch`, `handlers/tools`, config writer Gemini settings, `tool_commands`, `agent-credentials` and `AgentManagementPage`.


### Subscription presentation must distinguish identity, quota and availability
- **Symptom**: imported records repeated internal account keys, import states appeared twice, Codex limits used internal primary/secondary labels, and failed reads erased useful quota data.
- **Root cause**: generated titles used ownership identities (`community/account.rs`), provider details rendered raw snapshot rows, and `community/quota.rs::refresh` replaced the snapshot with an error-only result.
- **Fix**: derive human-readable Rust titles separately from ownership keys; share account presentation for legacy records; render period-aware progress bars, optional credits and localized empty/loading/recovery states. Keep failed readings stale and schedule retries from the attempt receipt.
- **Guardrail**: never change account identity for display. Never infer a missing usage window or treat unknown balances as zero; retain model scope, explicit zero readings, and the last successful timestamp on failure. Keep the retry timestamp separate from cached readings.
- **Watch points**: `SubscriptionImportDialog.test.ts`, `SubscriptionUsageCard.test.ts`, `community::display`, `community::quota::snapshot_tests`, Codex allowance tests, shared Chinese/English catalogs and 960×640 Tauri validation.

### Inherited XDG defaults must not escape an independent tool home
- **Symptom**: OpenCode credential import and preview tests read or wrote the runner account instead of their isolated home.
- **Root cause**: `utils.rs::opencode_config_path` and the auth reader applied a process-default XDG directory to a different supplied home.
- **Fix**: Share config/auth path resolution, rebase inherited default XDG paths onto independent homes and preserve custom XDG locations.
- **Guardrail**: Test config and data defaults against independent homes, retain explicit custom overrides, and use the same resolver for detection, snapshots, previews and writes.
- **Watch points**: `aipass-config-writers::{utils,detect,plan}`, `tool_credentials::reader`, Linux CI and per-vault test home overrides.

### Native helper previews and fixtures must follow the host shell
- **Symptom**: Windows previews hid a generated Claude helper; Cursor/Zed import fixtures read no keys; a JSON path assertion failed on escaped backslashes.
- **Root cause**: preview redaction only recognized the Unix helper, while tests hard-coded `.env` files, Unix quoting and serialized path substrings. Zed also inherited the process profile's default APPDATA for independent homes.
- **Fix**: recognize only the exact generated PowerShell script after strict base64/UTF-16 decoding, retain redaction for arbitrary commands, use native helper paths/literals, parse JSON paths before comparison, and rebase default APPDATA like default XDG paths.
- **Guardrail**: test both generated shell forms and malicious encoded commands on every OS; retain explicit custom profile directories and keep test writes inside their isolated home.
- **Watch points**: config writer previews, native credential imports, `ToolEntry::credential_command`, profile path resolution and Windows workspace tests.

### Every prerelease extension needs a numeric browser version
- **Symptom**: beta packaging could succeed with an unloadable semver prerelease in manifest.version; the desktop bundled a different extension version.
- **Root cause**: `release.yml` normalized only nightly builds and `validate-edge-submission.mjs` did not check browser version constraints.
- **Fix**: Stamp desktop and standalone extension packages for every channel; use numeric build versions and version_name for prereleases. Share strict version validation before writing and packaging.
- **Guardrail**: Reject prerelease text, leading zeroes, overflow and all-zero browser versions before publication. Test beta and nightly stamping without partial writes.
- **Watch points**: both release workflow stamp steps, `set-extension-release-version.mjs`, `extension-version.mjs`, Edge validation and packaged extension metadata.


### Passive subscription feedback must not change generation replay semantics
- **Symptom**: Scheduled quota lagged real generation; partial readings could refresh unrelated windows, and a failed passive write could hide a submitted response.
- **Root cause**: HTTP/CLI streams and scheduled quota used different state paths; response feedback shared fatal authentication-update handling.
- **Fix**: Merge credential/generation-bound encrypted observations through existing CAS writes; retain per-window ages, refresh runtime quotas without rotating markers, and forward the original response when passive persistence fails.
- **Guardrail**: Test rejected feedback ACKs without response loss or replay, partial-window timestamps, scoped limits and delayed old-generation writes. Preserve scheduled scopes and a separate poll-at timestamp when publishing a partial response view. Passive Claude identity checks read only the non-sensitive profile, never the native grant/Keychain. Internal polls must not touch session idle time.
- **Watch points**: `subscriptions/feedback`, `community/{feedback,quota}`, `official_accounts/feedback`, `claude_bridge/feedback`, `QuotaWindow` and typed internal IPC.

### Subscription identity includes the vendor's billing scope
- **Symptom**: Same-name Enterprise Cloud, explicit Cloud project or WorkBuddy enterprise accounts could merge or send personal-account headers.
- **Root cause**: Import readers and request signers retained the user ID while dropping the host/project/enterprise selection.
- **Fix**: Pin Copilot host, Gemini explicit project and WorkBuddy enterprise in account identity; share Copilot header/endpoint contracts, send tenant headers and preserve free/premium model quota scopes.
- **Guardrail**: Reject cross-tenant endpoints and changed native scope; preserve legacy public/personal identities. Share Copilot user/tool continuation and image accounting headers across transports. Normalize numeric scope IDs and keep missing used quantities unknown. Parse literal project settings without shell evaluation. Infer omitted protobuf zero only with the matching quota billing strategy and a known reset.
- **Watch points**: native imports, `cli_accounts/{store,copilot,gemini,quota}`, `workbuddy/scope`, `devin/quota`, Factory pool membership and billing-period snapshots.

### Cost and native-login automation require durable intent and existing authority
- **Symptom**: Repeating an uncertain reset could spend another credit; independent account automation could overwrite manual CLI choices or omit backups.
- **Root cause**: Remote spend and native config writes have separate commit points from their callers' response delivery.
- **Fix**: Default new policies off, persist reset request IDs before the official idempotent RPC, preserve unresolved attempts, and reuse the native switch coordinator and encrypted transactions. Return-to-preferred uses hysteresis and unknown quotas never authorize switching.
- **Guardrail**: Test reset-before-ACK refusal, lost/duplicate outcomes including zero remaining credits, unknown allowance with paid credits disabled, policy validation and UI busy/default-off states. Revalidate current native identity and all policy members before renewal and write; preserve manual/external choices, active Claude turns and login-before-switch. Never replay generation after renewal.
- **Watch points**: `subscription_policy`, `cli_accounts/reset`, `tool_switch/automatic`, provider runtime IPC, desktop, CLI and MCP.

### Restored opaque history must retain its original account binding
- **Symptom**: Eviction lost Claude tool ownership; newly requested Codex encrypted reasoning could otherwise follow group failover to another account.
- **Root cause**: Process-local call maps and session affinity cannot prove ownership after restart or target edits.
- **Fix**: Persist encrypted completed Claude checkpoints and Codex ciphertext digests; validate current generation and enabled target/secret binding before resolving ownership. Fork a native resume rather than overwrite an external transcript.
- **Guardrail**: Reject mixed/unknown encrypted history on subscription routes while leaving API-only routes to their existing authority. Share typed ciphertext recognition across outgoing ownership records, incoming hashes, model-group routing and cross-protocol validation: include agent messages and explicit encrypted content blocks, without parsing arbitrary tool JSON/text or deleting its IDs. Do not restore pending tools or partial transcripts. Bound transcript/event caches and verify plaintext is absent from AIPass storage.
- **Watch points**: `community/history`, `official_accounts/sessions`, `claude_bridge/{checkpoint,stream}`, subscription backend route-aware ownership, target edits and revocation.

### Sparse streaming usage must merge before cache normalization
- **Symptom**: Late usage-only events erased known input/cache counters; Anthropic-to-Chat missed cached tokens; normalizing each partial OpenAI observation then taking maxima double-counted uncached input when cache details arrived later.
- **Root cause**: Each converter and proxy ledger tracked different subsets, with zero defaults for omitted fields and normalization before the complete wire observation existed.
- **Fix**: Share `aipass-proxy-conversion::UsageAccumulator`; merge present cumulative counters and nested detail fields first, preserve missing/null fields and explicit zero, then extract protocol totals. Reuse buffered response usage mapping in streamed conversion. Split Chat/Messages state and tests from the legacy oversized stream module.
- **Guardrail**: Verify sparse and late cache observations against buffered conversion across protocols; check actual local HTTP output and SQLite records. Do not add cumulative events together or take maxima of already-normalized uncached input. Preserve finish reasons across usage-only updates, mark Messages-to-Responses terminal exactly once, and preserve tool start arguments if no JSON delta follows.
- **Watch points**: conversion `usage`, `stream/chat_messages`, `openai/stream`, proxy `sse`; regressions in `usage/tests` and `tests/local_proxy_review`.

### Client cache controls must remain explicit and loss must be reported
- **Symptom**: Native CLI caching could change from ordinary client annotations, or cache marks could disappear across system/MCP/history rendering while appearing supported.
- **Root cause**: The native CLI has different cache control surfaces from the vendor Messages API (`claude_bridge.rs`, cache planning before native prompt construction).
- **Fix**: Default CLI behavior plus explicit Messages header, shared per-request plan, mark-loss headers and local preflight rejection.
- **Guardrail**: Strip ordinary marks before the CLI boundary; preserve system/tool authority, report unmapped marks, and bind cache mode/tail to the native context. Reject invalid local requests without account replay or vendor circuit failures.
- **Watch points**: `claude_bridge/cache.rs`, `forward/http_attempt.rs`, `tests/local_proxy_review.rs`.

### Mixed cache TTLs and optional prices must not create free or duplicate usage
- **Symptom**: One-hour writes could use the wrong price; a missing optional source price could become explicit zero because the legacy scalar parser defaults missing fields to zero.
- **Root cause**: Four-bucket usage/pricing assumed one cache creation rate (`aipass-proxy-conversion/src/lib.rs`, `pricing/list_prices.rs::litellm_rule`).
- **Fix**: Retain the hour count as a subset, persist/migrate it, and price the remainder separately through the same rule/version/off-peak/multiplier pipeline.
- **Guardrail**: Preserve absent versus explicit zero hour rates. Never add the subset to input totals. Test late streaming updates, protocol round trips, old SQLite rows, query-time repricing and UI edit round trips.
- **Watch points**: `usage.rs`, `pricing/cost.rs`, `sse::estimate_cost`, price DTOs and both price editors.

### Retained native tool results need ownership and reservation before upload
- **Symptom**: Recreating the provider Run after a tool call loses native state; retrying partial result writes could execute a continuation twice.
- **Root cause**: Per-request adapter runtimes end after the HTTP response (`subscriptions/cursor.rs` before native Run pooling).
- **Fix**: Rust-owned connection workers, account/context/history checks, native KV retention and complete-batch reservation before writes; revoke on target changes.
- **Follow-up (2026-10-16)**: A parked Run outlives its request adapter. Reconciling only live adapter Weak pointers missed outbound-proxy changes; reconcile the connection pool independently and check outbound settings again before continuation. Resolve the service endpoint only when opening a new Run.
- **Guardrail**: Keep generation reads unbounded, bound only idle retention/setup/writes, never replay uncertain results, bind call routing to its original credential and label whole-Run usage rather than inventing a last-step count. Revoke retained connections on effective transport changes even after the originating request adapter has ended.
- **Watch points**: Cursor pool/codec, CommunityBridge owner retention, HTTP/2 fixture and proxy cancellation.

### Keychain metadata caching cannot authorize generation or native backups
- **Symptom**: Repeated account polling opens duplicate authorization prompts; a cached pre-switch credential can miss an external account change.
- **Root cause**: Store selection and every credential read independently accessed Keychain (`native_auth.rs::NativeStore::codex`). Metadata modification dates alone can have coarse resolution.
- **Fix**: Non-interactive attribute selection, in-flight coalescing, bounded zeroizing discovery snapshots, refusal cooldown and explicit reconnect retry. Fresh reads remain mandatory at generation/backup boundaries.
- **Follow-up (2026-10-16)**: A shared completed-generation counter let a faster fresh read overwrite an earlier waiter's completion. Each in-flight read now owns its result until its waiters finish. Capacity eviction cannot discard live callers or an unexpired refusal. The 16-caller/1,024-read regression failed before the fix and passes after it.
- **Guardrail**: Never treat attribute/ACL failure as absence. Keep unanswered reads coalesced, scope explicit retry to the selected account, preserve other refusal cooldowns across lock/invalidation and capacity eviction, invalidate in-flight epochs on writes/lock, erase idle snapshots, and test overlapping fresh reads as well as completed-cache bypass. A new completion must not erase another read's outcome.
- **Absence follow-up (2026-10-19)**: an attributes-only `None` now immediately clears snapshots and returns absence, without starting an authorization worker. The former worker raced global epoch invalidation from parallel vault locks, causing unrelated file-backed Claude operations to fail. Attribute/ACL errors still propagate; a regression covers cached-byte erasure and concurrent invalidation.
- **Watch points**: `native_auth/keychain_cache`, CLI account fresh/generate/login, native switching and session locking.

### Launchd fixtures must publish complete readiness data atomically
- **Symptom**: The complete workspace intermittently failed with `kill: illegal process id` although the isolated tray suspension test passed.
- **Root cause**: `autostart/imp/tests.rs` waited for PID-file existence while shell redirection had created the file before `echo` wrote the PID.
- **Fix**: Publish the fixture PID through a temporary file and atomic rename; production tray suspension is unchanged.
- **Guardrail**: Never use existence of a partially written PID/readiness file as completion. Keep the actual child-survival assertion and exercise it in the full workspace.
- **Watch points**: Launchd fixtures, process cleanup and runtime-check readiness handshakes.

### Grok compatibility must preserve tool identity and argument values
- **Symptom**: Namespaces/custom tools are rejected upstream, regex character classes fail schema validation, and strictly typed clients reject integral argument decimals.
- **Root cause**: Native and API forwarding lacked a shared provider adapter; generic JSON floating-point parsing can round large integers or alter unrelated application data.
- **Fix**: Shared request/JSON/SSE adapter for explicit Grok profiles and native Build; deterministic alias mapping, bounded custom-input envelopes, lexical per-call number normalization and schema-position walking.
- **Guardrail**: Apply identity restoration to item/done/completed events and historical calls/tool choice. Test every delta cut and interleaved calls; preserve strings/fractions/exponents and schema defaults/enums. Reject unsupported grammar tools explicitly. Protobuf Struct decoders must also preserve safe integral tool values for strictly typed clients.
- **Watch points**: `providers/grok`, Cursor protobuf values, API HTTP/adapted WS and native subscription dispatch.

### Linux packages must resolve helpers from their installed resource directory
- **Symptom**: Linux desktop/CLI could not find Agent or Native Host outside a build checkout; standalone CLI archives omitted Agent, and Linux tray login registration silently returned unsupported.
- **Root cause**: sibling-only discovery in desktop CLI/native-host installation and Agent launch omitted Tauri's `usr/lib/AIPass` product resource directory; CLI release staging copied only two of the three required binaries. Linux autostart was a stub and browser configuration assumed `~/.config`.
- **Fix**: share helper candidates and absolute XDG configuration resolution, package Agent with CLI, register the tray through XDG desktop login and retain the persistent AppImage launch path. Add isolated CLI/Native Messaging smoke and Linux deb main/tray startup CI at 960×640.
- **Guardrail**: validate helpers from installed package layouts and test standalone packages with isolated HOME and no helper overrides. Linux build success is not desktop runtime evidence. Record actual remote results; native macOS tests cannot establish Linux/Wayland, update or vendor-login behavior.
- **Watch points**: `bundled`, `launcher`, CLI/desktop native-host discovery, `cli_install`, Linux autostart, `runtime_check`, CI and release staging.

### Dedicated plan keys, current model metadata and quota headroom must share scopes
- **Symptom**: Coding Plan and Token Plan keys were conflated; failed catalog refreshes replaced successful account models; global allowance gates could suppress free or unrelated models.
- **Root cause**: Product-name/prefix heuristics and static fallback rebuilding in `subscriptions/plans.rs`; protocol metadata without current price/scope in `subscriptions/opencode.rs`.
- **Fix**: Separate Kimi Code, MiniMax M Plan and Bailian Coding Plan entries; retain last successful catalogs; refresh bounded provider-specific metadata; parse native key allowance with exact metered-model scopes. Bind console URLs to encrypted account region. Apply optional quota headroom only to routing readings and keep display percentages unchanged.
- **Guardrail**: Never infer a billing product or wire from a model name. Keep missing usage unknown, exclude only explicitly free models, preserve model scopes and freshness in reset/pace ordering, and enforce configured headroom even when paid-credit/reset policies are enabled. Reject duplicate/truncated tool JSON and preserve declared string parameters in vendor templates.
- **Watch points**: `plans`, `plan_quota`, `opencode`, `providers/trae/tools`, `community/console`, `subscription_policy`, proxy eligibility/affinity and quota UI.

### Subscription allowance fields are not interchangeable across billing products
- **Symptom**: exhausted Copilot enterprise pools disappeared, Cursor paid accounts looked exhausted, numeric Command Code resets and Factory extra balances were lost, and missing counters became zero usage.
- **Root cause**: blanket unlimited/percentage handling in `subscriptions/cli_accounts/quota.rs`, included-only Cursor pools, raw numeric dates in `commandcode.rs`, display-only money in `factory.rs`, and default-zero counters in Kiro/Grok/Devin. Inference URLs were also reused for OpenCode billing.
- **Fix**: normalize vendor billing semantics in Rust, preserve exact model scopes and separate purchased balances; share MiniMax account/key parsing, add Ollama allowance queries, and correct Kiro key RPC and ZCode week units. Evidence and remaining gaps: `docs/subscription-quota-audit-2026-10-23.md`.
- **Guardrail**: test enterprise `unlimited` plus exhausted pools, precise counts versus rounded percentages, cents versus dollars, seconds versus milliseconds, missing versus explicit zero, and paid budgets alongside remaining included allowance. Trace every field through the typed snapshot and routing; a display-only amount is not a balance. Treat Magpie as protocol evidence, not proof of live account compatibility.
- **Watch points**: provider usage parsers, `community/quota.rs`, `QuotaWindow` model scopes and freshness, `SubscriptionUsageCard.test.ts`, and regional/account billing endpoints.

### Vendor task identity is distinct from a model call's request ID
- **Symptom**: Qoder tool-loop calls repeatedly sent a fresh start time and stage `start`; native request/session IDs and cache-envelope fields were absent.
- **Root cause**: `subscriptions/qoder.rs::generate` built each call from the current clock; the converter modeled only one request, not the user prompt it answers.
- **Fix**: retain bounded, credential-free task metadata under the existing account generation/session; keep request IDs fresh, native task IDs/start time stable and user cache markers scoped. Recognize DSH and WorkBuddy session headers through existing affinity resolution.
- **Guardrail**: test tool continuations, new prompts, account/session separation, changed system text, retries, expiry, eviction and late completions. Store prompt hashes, not plaintext. Do not claim vendor billing savings without account measurements or replay uncertain generation.
- **Watch points**: `subscriptions/qoder/task.rs`, conversion `providers/qoder.rs`, proxy session affinity, `docs/subscription-quota-audit-2026-10-23.md`.

### Provider compatibility rewrites must preserve instructions and meaningful completion
- **Symptom**: Factory removed instructions joined to its billing header, Kimi Work rejected valid tool-schema shapes, and Devin completed usage-only replies as empty success.
- **Root cause**: whole-block metadata filtering, direct schema passthrough and unconditional stream completion in the provider converters.
- **Fix**: remove only the billing line; normalize schema locations with bounded reference handling; reject a fully completed Devin stream with no content before sending success headers. Read Devin's current cost tiers before applying account-wide quota gates.
- **Guardrail**: preserve literal defaults/examples, constraints and block metadata; test idempotent rewrites. Distinguish a confirmed content-free completion from malformed/truncated streams and post-content errors, whose outcome must remain non-replayable. Exempt only verified free variants and all-free aliases.
- **Watch points**: conversion `providers/{devin,kimi,qoder}.rs`, `factory_request`, Agent `Context::converted`, Devin quota scope, and their regression tests.

### Subscription renewal must validate the source after failure
- **Symptom**: early Grok 401s could remain stuck despite future expiry; concurrent Agents could renew the same source twice; copied managed grants could repeatedly send an already rejected refresh token.
- **Root cause**: `subscriptions/cli_accounts/session.rs` treated expiry and CLI exit status as renewal evidence; entry-level serialization did not cover a shared CLI store. Token endpoint failures had no authoritative reread.
- **Fix**: Grok locks the canonical source across reread, CLI renewal and verified reread; read-only GETs retry once and generation is never replayed. Selected managed OAuth adapters reread before durably marking a rejected grant, preserving a newer grant and all account data.
- **Guardrail**: require changed usable same-owner credentials, test two processes against a single-use-token fixture, and never steal a live lock by age. Distinguish source locks from cross-device ownership: synchronized legacy grant copies require explicit fresh local authorization; new grants use the local v3 path. Do not flag rate limits or server failures as rejected authorization.
- **Watch points**: `cli_accounts/{grok,source_lock}`, `subscriptions/{feedback,renewal}`, `community/renewal`, MiniMax/Qoder/Antigravity/Trae token exchange, cancellation and durable ACK.

### Local subscription authorization must not travel with account descriptors
- **Symptom**: copied refresh grants could race across vaults/devices; late authorization writes could overwrite a new login. Successful IPC writes were parsed as unit despite an empty-object response.
- **Root cause**: synchronized runtime extensions mixed logical account identity with writable authorization; revision checks alone did not distinguish device-local reconnects.
- **Fix**: encrypted local grant records use per-write DEKs and device/canonical-directory AAD outside sync/export; v3 descriptors contain no grants. Explicit reconnect pins the account and unlocked session, and compares the descriptor/local binding before commit. Parse the actual ACK shape. Reset quota request state on account selection and gate late responses by request generation, including A-B-A selection.
- **Guardrail**: preserve entry/secret IDs and routes; require fresh authorization before migrating legacy grants; reject old local-binding writes even at a matching revision. Test two real Agent processes, backup/copy exclusion, epoch/password changes, lock/reunlock and concurrent reconnect. Do not sync local authorization failures as shared quota failures or claim retroactive revocation of old copies.
- **Watch points**: `community/{account,device,reconnect,feedback,quota,renewal}`, subscription import, `local_subscription`, snapshot/export allowlists, protocol version and reconnect UI capability. Local-state key wrapping is stable across sync epochs; do not claim provider-record epoch erasure.

### Devin billing scope must survive every account entry path
- **Symptom**: email/JWT deduplication merged organization accounts; a same-source legacy import could refuse its own scope upgrade; checking an obsolete key first blocked a valid CLI replacement.
- **Root cause**: `subscriptions/native_import.rs` enriched only missing identities; browser login and native import used different identity proof, while `subscription_import/persist.rs::same_binding` treated migration as an ordinary save.
- **Fix**: resolve principal/server/org/team with the actual key in both login paths. Migrate legacy encrypted records by CAS and a new generation/storage version. Verify a changed source key against established scope before saving; labels are metadata.
- **Guardrail**: preserve entry/secret IDs and routes, reject stale writes and organization changes, and test same-source migration plus repeat import. Missing scope must not deduplicate by email. Never relax ordinary save ownership checks to make a migration pass.
- **Watch points**: Devin identity/login/fresh/quota, `community/{account,scope}`, subscription import persistence, vault subscription bindings, active proxy generation and older-reader compatibility.

### Import discovery must track device-local authorization revisions
- **Symptom**: custom Devin and managed OAuth sources vanished from rescans; an in-flight import could overwrite a newer local reconnect and report its local change as unchanged.
- **Root cause**: `subscription_import/sources.rs` read only v1 extensions; `persist.rs` compared only synchronized entry timestamps, which local reconnect deliberately preserves.
- **Fix**: resolve stored v2 and this vault's local v3 bindings without network access, and compare a digest of the current binding as well as the entry timestamp before committing. Use the same revision to classify updates.
- **Guardrail**: preserve custom-source discovery through format upgrades, require this device's source authority, reject late local reconnect/refresh writes, and test restored vaults without borrowed local grants. Both regressions must fail against the old implementation.
- **Watch points**: import snapshots, CAS persistence, encrypted local bindings, native source references and import result status.

### Packaged subscription fixtures must follow the current IPC and identity contract
- **Symptom**: Linux packaged runtime rejected capability probes after a protocol upgrade; synthetic Devin imports could no longer prove their organization.
- **Root cause**: `scripts/verify-linux-native-clis.mjs` hardcoded protocol 18 and assumed a JWT alone established Devin ownership.
- **Fix**: read the protocol constant from its Rust owner and serve key-bound status from an isolated HTTPS fixture with a process-local Linux CA file. Assert principal, server, organization, team and stable entry IDs across repeated imports.
- **Guardrail**: search packaged scripts whenever changing IPC or authorization semantics. Keep positive identity/import checks, verify TLS and wrong-key rejection, and never contact real vendor accounts or disable certificate verification for fixtures.
- **Watch points**: `verify-linux-native-clis.mjs`, `scripts/fixtures/devin-status*`, protocol version, native import and platform runtime jobs.

### Desktop startup sizes must fit the monitor work area
- **Symptom**: the installed Linux app opened at 1280×820 on a 1280×800 X11 desktop, hiding bottom navigation below the screen.
- **Root cause**: default and restored window sizes enforced only the 960×640 minimum and a saved-size tolerance, without limiting the size to the current monitor work area.
- **Fix**: extract window sizing/persistence into `window_geometry.rs` and cap main/server dimensions to the monitor work area in logical pixels, retaining the Tauri minimum and invalid-monitor fallback.
- **Guardrail**: test first-launch and restored sizes on small work areas and HiDPI monitors; visually verify the installed app with desktop panels present, in addition to the 960×640 runtime fixture.
- **Watch points**: initial activation, existing-window reveal from the tray, server targets, saved window size, monitor scale and minimum-window configuration.

### Sync settings must respect the desktop build target
- **Symptom**: Linux offered active iCloud CloudKit save/sync actions even though its Agent cannot execute the macOS-native transport.
- **Root cause**: the settings target list was unconditional; native CloudKit rejection occurred only after the user chose an unusable action.
- **Fix**: extract sync target controls and use Tauri's target-platform build variable for CloudKit presentation. Omit the unavailable option and explain/block an existing iCloud selection until a supported target is chosen.
- **Guardrail**: test non-Apple choices, an existing unsupported selection, recovery to local sync and macOS CloudKit actions. Keep Rust's platform rejection authoritative.
- **Watch points**: Vite target injection, sync target settings, local/OneDrive/WebDAV controls, saved settings and native CloudKit bridge.

### GTK tray submenu captions need reconstruction after locale changes
- **Symptom**: Linux leaf actions switched to Chinese while Local proxy and Control panel submenu captions stayed in English.
- **Root cause**: muda 0.19.2 creates GTK submenus with a custom AccelLabel child, but its set_text changes only MenuItem.label; the visible custom label retains its original text.
- **Fix**: extract classic-menu construction and rebuild the root tray menu once per locale change on Linux, retaining the existing action IDs, enabled states and group items.
- **Guardrail**: switch language in a running X11 app and inspect submenu captions plus leaf actions; verify menu actions still work after reconstruction. Translation catalog tests alone cannot establish native GTK updates.
- **Watch points**: tray feedback, localized submenu captions, dynamic group menu, classic menu creation and Linux AppIndicator.

### Linux session policy needs native event sources
- **Symptom**: sleep/screen-lock preferences appeared available on Linux, but the power watcher was a no-op; the Agent remained unlocked after the X11 session ended.
- **Root cause**: native power and system-idle integration existed only inside the macOS branch of `session.rs`.
- **Fix**: extract platform power modules; subscribe to authenticated logind and desktop screensaver signals through zbus, verify the owning UID before applying per-session logind locks, and apply the current policy on active signals or loss of the desktop bus. Cache supported desktop idle APIs without blocking IPC.
- **Guardrail**: test real private DBus transport, sender filtering, own/other session ownership, inactive/resume signals and policy switches. Verify installed Agent lock/unlock and an actual X11 bus teardown separately from physical suspend/resume.
- **Watch points**: Agent startup, current session policy, vault lock and proxy sanitization, DBus reconnect, system-idle fallback and native macOS observer behavior.

### Linux window chrome must preserve column ownership
- **Symptom**: custom buttons overlapped detail actions. Native decorations and then a separate full-window header both violated the requested appearance/layout.
- **Root cause**: the fix treated window chrome as a separate whole-window row instead of preserving the existing middle-list title/menu and right-detail control strip.
- **Fix**: retain the shared middle-column title/menu placement; constrain Linux custom window controls to the right column, share its first row with the detail identity and place metadata/actions in the next row. Use one shared 8-pixel inner top inset for the sidebar, middle title/menu, detail identity and window buttons; reclaim the old empty strip without placing content against the panel edge. Cover provider, edit, server and spanning Agent management views. Keep decorations disabled and opaque rendering. Tauri owns Linux drag-region double-click handling.
- **Guardrail**: preserve the three-column layout and shared column variables. Do not move the app title to the navigation column or add a full-width header row. At 960×640 and maximized size, verify title/menu alignment, window controls, detail actions, drag and close/reopen. Match native-window configuration and frontend build target for packaged, dev and tray-first entry paths.
- **Watch points**: `AppTitleBar`, `linux-window.scss`, provider/server detail and Agent management roots, platform build define, `window_geometry` and tray reveal.

### Native CLI discovery and credentials must share the desktop environment rules
- **Symptom**: Linux detected an installed tool but could not import/login/renew it; npm CLIs failed under a desktop PATH, custom XDG credentials disappeared, and Codex/Copilot OS credentials were unavailable.
- **Root cause**: GUI detection and each vendor resolver maintained different search paths. Child processes lost user runtimes and the desktop DBus address. Credential-store access only handled macOS Keychain.
- **Fix**: share executable discovery and child PATH construction, require Unix execute permissions, discover custom package-manager roots and sort Node versions semantically. Keep user runtimes ahead of system bins in vendor subprocesses, including when the discovered CLI itself lives in a system bin directory. Partition the complete child PATH after adding executable parents; partitioning only the fallback directories reintroduces the old system Node. Use the same vendor-aware Cursor/Grok candidate filter in GUI detection and login; unrelated programs with the same generic name must not appear installed. Share absolute XDG config/data resolution. Use bounded, encrypted Secret Service reads with the exact Rust-keyring or keytar attribute schema; never unlock or prompt during discovery. Codex auto may read its file fallback, while native writes still require every resource preimage. Copilot prefers fresh OS credentials over its plaintext fallback.
- **Guardrail**: run `verify-linux-native-clis.mjs` against production binaries with isolated HOME, a minimal desktop PATH and synthetic grants; test wrong permissions, malformed/expired/missing stores and non-executable files. Run `verify-linux-keyring.mjs` only under a fresh private `dbus-run-session`. Include real vendor capability probes separately from fixture tests. When transferring source snapshots, preserve fresh modification times or extract with `tar -m`; matching bytes on disk do not prove Cargo rebuilt a file whose archived timestamp stayed unchanged.
- **Watch points**: `cli_discovery`, native CLI command environment, `native_import`, XDG helpers, `native_auth/linux_keyring`, default versus independent vendor homes and Linux packaged-runtime CI.

### CLI update completion must verify the running suite
- **Symptom**: an installer acknowledgement can be mistaken for a completed update, while copied CLI helpers or the resident Agent still run the previous version.
- **Root cause**: desktop replacement, copied CLI installation and Agent readiness have separate asynchronous lifetimes (`updates.rs`, `cli_install.rs`, `startup.rs`).
- **Fix**: authenticated singleton update control reuses the signed updater and serializes UI/CLI mutation; the CLI checks a changed desktop PID, expected desktop/CLI versions and typed Agent readiness. Standalone updates verify official asset digests, stage all three executables, scope service ownership and retain rollback files on recovery errors.
- **Guardrail**: never report `installed` on acknowledgement alone; keep status responsive during install, preserve tray mode for unattended restart, fail without authorization prompts, and exercise CLI-driven signed restarts in `verify-macos-runtime.mjs`. Do not overwrite another installation's service or weaken update signatures.
- **Watch points**: `updates/control.rs`, singleton auth, CLI dispatch/updates, Rust updates/install, copied helpers and runtime-check fixtures.

### CLI update fixtures must wait for completed startup probes
- **Symptom**: macOS `cli-install-restart` exited during installation; the CLI timed out waiting for a restarted desktop with `Connection refused`.
- **Root cause**: `apps/desktop/src-tauri/src/runtime_check.rs:334` emitted `desktop.runtime_check.ready` before its ten-second Agent/event-loop probes completed. `verify-macos-runtime.mjs` immediately started the CLI installer, whose intentional Agent shutdown failed a remaining probe and exited the installing desktop.
- **Fix**: emit readiness only after all startup probes and viewport checks succeed, then wait for the CLI install/restart handshake.
- **Guardrail**: publish fixture readiness after every probe that requires the pre-update Agent. Run the signed CLI install/restart scenario; retain changed-PID, running-version, copied-CLI, Agent-readiness and tray-mode assertions.
- **Watch points**: `runtime_check.rs::observe/start`, `verify-macos-runtime.mjs`, CLI update restart polling and `updates.rs::stop_runtime_processes`.

### Linux updates must preserve installation format and authorization boundaries
- **Symptom**: Linux feeds contained only macOS entries, package authorization could fall through to password prompts, and installation failures disappeared while an update remained available.
- **Root cause**: generic updater targets, the plugin's system-package fallback sequence, and an error banner nested inside the no-update branch. Plugin `install` also does not verify bytes reread from cache.
- **Fix**: require Linux architecture and installer-format targets; verify the exact install bytes; preflight AppImage directory access and package-manager availability; use one pkexec operation for system packages and stop Linux runtime processes only after successful installation. Keep cached system packages for explicit installation and keep errors visible and localized.
- **Guardrail**: exercise signed manual/cached AppImage restarts, cross-version executable hashes, denied directory/cache access and bad signatures as an ordinary user. Reject truncated or wrong-architecture AppImages before replacing the executable. Close the restart confirmation after clearing its busy guard, and test failure visibility with a release still available. Cancellation/denial must preserve the live Agent and proxy; never collect sudo passwords. Real desktop Polkit, RPM and Wayland evidence must be reported separately from fixture/unit checks.
- **Watch points**: `updates/linux.rs`, cache reread, `runtime_lifecycle`, updater manifest merging, Settings update banner and Linux artifact gates.

### Linux transparent chrome and native form controls need runtime review
- **Symptom**: Linux forced opaque square windows; window buttons used a different right inset; forced dark mode left native selects with light backgrounds and unreadable text; Chinese segmented labels wrapped at the minimum viewport.
- **Fix**: request RGBA windows, follow compositor changes and use square edges when uncomposited/maximized. Share detail/header insets, preserve the 8px top gap, keep segmented labels on one line, and bind native control colors and color-scheme to the selected theme. GTK native select popups can still use the OS theme despite these declarations; use the shared SelectField for desktop selectors and inspect the expanded menu, not only its closed trigger.
- **Guardrail**: inspect actual GTK/WebKit windows at 960x640 in both languages and themes, with compositing on/off, real edge resizing, title dragging, double-click, maximize/restore, minimize and close/reopen. Synthetic X11 presses must include realistic down/up intervals; extremely short injected events can be dropped by the test desktop.
- **Watch points**: `window_chrome`, `AppTitleBar`, `linux-window.scss`, shared theme tokens, segmented controls and Agent headers.

### OAuth device fixtures must route requests before consuming a poll response
- **Symptom**: the full macOS suite failed because a local GET reached the device-token listener before the expected POST; polling then expired.
- **Root cause**: `subscriptions/device.rs` consumed one prepared OAuth response per accepted TCP connection, regardless of request route.
- **Fix**: return 404 for routes other than `/token` without advancing the response sequence. Inject a root GET before polling in the regression; preserve the POST, device-code, verifier and rotated-token assertions.
- **Guardrail**: route unrelated listener discovery traffic before consuming a prepared API response. Retain the original failure diagnostics and require the full unchanged assertions to pass after the fixture repair.
- **Watch points**: OAuth device fixtures, `vault_sync/tests.rs` HTTP fixtures and proxy `tests/mock_upstream.rs`.

### No-replay fixtures must distinguish local discovery from API requests
- **Symptom**: a full macOS run failed because the fallback listener had an unexpected TCP connection; the old assertion discarded request diagnostics.
- **Root cause**: the fixture asserted absence of every TCP connection instead of an API replay. The original connection's request was not retained, so its exact origin remains unproven.
- **Fix**: share the existing unauthenticated-root-GET classifier for primary fixture accepts and no-replay fallback assertions. Explicitly reject API POSTs, authenticated root requests and other paths in a new guard regression.
- **Guardrail**: never ignore real API requests or authenticated probes to make no-replay tests pass. Capture the unexpected request method/path, preserve the failed run, and require the complete proxy suite after hardening the fixture.
- **Watch points**: `aipass-proxy/src/tests/mock_upstream.rs`, no-replay and truncated-stream fixtures.

### Readable update bytes do not imply a removable update cache
- **Symptom**: a signed cached AppImage in a read-only cache installed/restarted successfully but left the pending package behind; the real artifact gate failed.
- **Root cause**: cache write permission was tested only when downloading, and post-install cleanup discarded removal errors.
- **Fix**: after verifying/copying install bytes, clear the expendable cache under the update transaction before replacing the app or stopping the Agent. Return cache permission failures before installation. Cached system packages still wait for explicit installation.
- **Guardrail**: test both an empty read-only cache and a complete valid signed package in a read-only cache as an ordinary user. Require unchanged executable/PID/Agent on denial and no cache after success; installation failures may need a fresh download.
- **Watch points**: `updates::install_verified_update`, Linux `readonly-cached` artifact scenario, macOS manual/cached restart gates.

### Linux packaging dependencies must be explicit on every architecture
- **Symptom**: aarch64 nightly AppImage packaging failed because `/usr/bin/xdg-mime` was missing while x86_64 CI passed.
- **Root cause**: desktop jobs relied on `xdg-utils` preinstalled on the x86_64 runner; the ARM64 image did not include it.
- **Fix**: install `xdg-utils` explicitly in CI and release desktop jobs and check `xdg-mime` before building.
- **Guardrail**: declare Linux packager dependencies in both workflows; verify every shipped architecture rather than inferring ARM64 readiness from x86_64.
- **Watch points**: `.github/workflows/ci.yml` Linux desktop dependencies, `.github/workflows/release.yml` Linux matrix.

### Hidden Claude tools must preserve the outer client's execution boundary
- **Symptom**: All native tools disabled also removed subscription web search; simply enabling all tools would execute local actions inside the hidden bridge workspace.
- **Root cause**: `claude_bridge.rs` used an empty `--tools` list without an explicitly requested search exception; stream completion assumed one assistant message per public reply.
- **Fix**: Whitelist only `WebSearch` for native search declarations, exclude it from caller MCP tools, merge internal search messages and accumulate usage. Gateway search reuses Rust routing and an account-scoped memory ledger.
- **Guardrail**: Keep Bash/Read/Write off. A caller function named WebSearch grants no native capability. Honor disabled/forced caller tools, preserve account identity, bound hidden search rounds, and reject unsupported search constraints. Run `cargo test -p aipass-agent claude_bridge` and `cargo test -p aipass-proxy search`.
- **Watch points**: `claude_bridge/{native_tools,stream}`, proxy `search/{wire,gateway,history,mcp}`, HTTP and adapted WebSocket forwarding, configuration refresh and vault lock.

## Tool integration

### Cursor IDE, regular CLI and Local runtime have different BYOK contracts
- **Symptom**: Cursor Local's Anthropic integration requested `/v1/messages/messages`; config-only tests passed while the official runtime failed the endpoint contract.
- **Root cause**: `plan.rs::cursor_local_endpoint` and `server/tools.rs::proxy_endpoint_for_tool` passed a full Messages URL to a runtime that appends `/messages`. Ordinary Anthropic model catalogs also lack Local's `api_types` discovery extension.
- **Fix**: Use a versioned base for Local proxy routes, rely on AIPass `/models` protocol metadata, and offer direct Local integration only for OpenAI-compatible credentials. Keep the writer in `plan/cursor.rs`.
- **Guardrail**: Distinguish IDE BYOK, Cursor account API keys and the separately distributed Local executable. Verify the actual model-request URL, authentication, streamed output and process exit with the official binary. Do not infer runtime or Linux success from generated text or fixture CLI imports.
- **Watch points**: desktop integration availability, Rust direct/proxy validation, config writer and credential reader, `tests/unix_integrations.rs`, `scripts/verify-cursor-local.mjs`, official runtime version and `/models` `api_types`.

### Vendor configuration text is not an executable integration contract
- **Symptom**: Pi 0.73.1 sent `$NAME` as a literal key; Muse 1.4.11-R6667.1 exited on a missing `/muse-code/models` even with a configured model. Pi also requested `/v1/v1/messages`; Zed, Kimi and Harness use SDKs that append the version themselves. Grok and Pi wrote to the wrong directory when their home overrides were set.
- **Root cause**: newer source documentation was treated as the released Pi contract; generic Responses support was treated as sufficient for Muse; readers, writers and discovery hardcoded separate paths.
- **Fix**: use Pi's cross-version `!command` credential reference; restrict Muse quick integration to its native Meta endpoint and reject local routes until its catalog protocol exists; share Grok/Pi directory resolution across detection, writing and outgoing imports; normalize versioned Anthropic bases in SDK writers while preserving gateway prefixes.
- **Guardrail**: test published vendor code/binaries, not just generated JSON. Skip command substitutions before parsing helper values, including guarded `|| return` assignments; do not execute them during credential import. Execute helpers under a clean shell, test home overrides and rollback of every auxiliary file, and keep manual editor guides distinct from automatic writes. Require explicit auth-header handling: bearer-only Anthropic gateways use a proxy unless the writer supports bearer tokens. Protocol availability must agree in UI and Rust; Qoder desktop now supports all three proxy APIs.
- **Watch points**: `plan.rs`, `utils.rs`, credential readers, `byok_tools.rs`, desktop integration catalog, `server/tools.rs`, `tests/unix_integrations.rs`; Linux CI results apply only to their tested source SHA.

## Credential clipboard

### Frontend timers and blind clears left credentials exposed or deleted user content
- **Symptom**: secret cleanup depended on WebView timers; a failed clipboard read fell back to clearing unrelated user content.
- **Root cause**: `apps/desktop/src/App.svelte::scheduleClipboardClear` retained plaintext in a frontend closure and `clearCopiedSecretFromClipboard` wrote an empty value after read failures.
- **Fix**: native `secret_clipboard` owns clipboard lifetime, bounded retry backoff, SHA-256 ownership digests instead of retained plaintext, confirmation before API-key reads, and lock/exit invalidation. All desktop sensitive-copy entry points reuse it.
- **Guardrail**: clear only after a successful matching read; preserve newer clipboard content, keep previous cleanup after a failed copy, and invalidate pending confirmation/Agent responses on lock. Never abandon cleanup after transient OS failures; retain only a digest and retry with bounded backoff until content changes or clearing succeeds. Cover these paths in `secret_clipboard/tests.rs`.
- **Watch points**: provider API keys, recovery kit, server tokens and panel access codes; UI preferences defaults/migration; native clipboard-history exclusion hints are best effort.


### Windows compatibility includes IPC, serialized paths and script launchers
- **Symptom**: Windows clients rejected every Agent request while Unix tests passed; portable backups used host-specific path separators; Node build helpers could not execute pnpm.cmd; Native Messaging registration polluted JSON stdout.
- **Root cause**: Interprocess named pipes explicitly reject socket timeout APIs. PathBuf serde retained Windows separators. Node execFile cannot launch CMD shims. reg.exe was inherited onto CLI stdout. The desktop singleton also used a machine-wide fixed pipe name and default pipe ACLs.
- **Fix**: Use nonblocking deadline framing for Windows clients and the desktop singleton, explicit user-SID pipe ownership/ACLs, client-side owner verification before credentials are sent, and per-profile singleton names. Canonicalize encrypted export/sync metadata to slash-separated relative paths and reject both styles of traversal on every OS. Invoke pnpm's JavaScript entry point or native executable without a shell, and capture registry output. A standalone Windows CLI may launch its sibling Agent without SCM registration. Refresh copied CLI companions together. Direct Agents receive Windows session/power notifications and query session-local input idle time. Empty nonblocking pipe reads must use PeekNamedPipe because interprocess/std map ERROR_NO_DATA to EOF; retry zero-byte writes as backpressure. Windows cancellation terminates vendor shim descendants. Development process cleanup matches exact development paths, never every production process with the same basename.
- **Guardrail**: Require native Windows workspace tests plus an installed NSIS main/tray/CLI/Native Messaging/uninstall smoke. Keep installer registration tests on disposable runners; preserve existing Native Messaging registry values in standalone CLI tests. macOS tests and PowerShell syntax checks are not Windows runtime evidence. Signed upgrade/restart, multi-user pipe ACL denial and real lock/suspend events remain separate native acceptance checks.
- **Watch points**: ipc/framing.rs, client.rs, singleton.rs, power/windows.rs, storage/portable_path.rs, VaultExportFile, pnpm-command.mjs, verify-windows-runtime.mjs and Windows CI.

### CLI argument construction must fit the native main-thread stack
- **Symptom**: native Windows `aipass --json init` aborted with a main-thread stack overflow although CLI unit tests passed. A macOS debug binary with a 1 MiB stack reproduced it even for `--version` and `--help`.
- **Root cause**: `crates/aipass-cli/src/cli_types.rs` derived one large `Command::augment_subcommands` frame containing both credential argument builders. The larger test-thread stack concealed the executable's startup failure.
- **Fix**: extract add/update arguments into independent `Args` implementations, preserving command names, aliases, options and help output. Exercise init, add, update and version parsing on a 1 MiB thread.
- **Guardrail**: keep large clap builders in separate argument modules. Run `argument_parsing_fits_the_windows_main_thread_stack` and actual debug/release CLI runtime checks; passing parser tests on their default thread stack is insufficient.
- **Watch points**: `cli_types.rs`, `cli_types/credentials.rs`, CLI dispatch destructuring and `verify-cli-runtime.mjs`.

### Windows login startup needs real per-user registration and stable namespaces
- **Symptom**: Windows tray login APIs always returned unregistered; normal repair required SCM privileges. A new nested vault could hash to a different pipe after its directory was created. CrossOver requests above the default pipe buffer never progressed.
- **Root cause**: `autostart/windows.rs` was a tray stub and delegated Agent autostart to SCM. `paths.rs` mixed ordinary nonexistent paths with verbatim canonical Windows paths. PIPE_NOWAIT could return zero for an oversized write forever, so retrying the same payload did not help. Login argv also survived updater restart.
- **Fix**: Register quoted literal HKCU Run entries, keep release/dev separate even during repair, launch the hidden tray/GUI-subsystem Agent without elevation, and clean only the uninstalling executable's entries and processes. Normalize Windows ancestors/prefixes before hashing. Fragment writes after zero progress while retaining deadlines and auth. Hold an exclusive profile-local listener file because named-pipe creation permits competing servers. Capture shutdown's process handle on its authenticated connection, wait for actual exit, and disable inherited handles on background spawns; null stdio alone leaked Node's output handles into the resident Agent. Version desktop login argv so a new executable reveals main even when NSIS/Explorer loses the explicit main-mode environment. Use the Windows updater's before-exit hook because install exits internally. Split desktop startup and tray lifecycle ownership out of the oversized roots.
- **Guardrail**: Test Run registry idempotence/removal, Unicode and metacharacter argv, pre/post-creation namespace equality, bidirectional frames above pipe-buffer size, CLI Agent install/stop/start/uninstall, close/reopen, quit and both login/manual update restart. Never count CrossOver as native Windows boot, Explorer, user isolation or lock/suspend acceptance. Preserve binary hashes and failure logs.
- **Watch points**: autostart/windows*, paths.rs, ipc/framing.rs, Agent main.rs, desktop launch_options/startup/tray_lifecycle/singleton/updates, NSIS hooks and verify-cli-runtime.mjs.


### Local listener discovery must not consume model fixture requests
- **Symptom**: Credential-reload tests sporadically returned 502 and image tests captured `/` instead of `/v1/images/edits`.
- **Evidence**: A disposable loopback observer received `GET / HTTP/1.1` with `User-Agent: LocalStack/1.0` from the running local service scanner.
- **Fix**: Reuse probe-aware accepts in Agent proxy fixtures and keep unauthenticated root discovery out of image fixture request counters. Preserve assertions for authenticated root and every API-path request.
- **Guardrail**: Inject discovery before the intended request in a regression test and prove authenticated/API traffic is still captured. Do not stop unrelated apps, retry until green, or relax production proxy behavior to repair test isolation.
- **Watch points**: proxy_service/tests.rs, proxy_service/tests/lifecycle.rs, proxy_service/tests/routing.rs and proxy/tests/image_api.rs.


### Extension detection must pair each extension with its own host
- **Symptom**: retained Cline files from VS Code Insiders appeared installed when only stable VS Code remained, or a sandbox extension was paired with a native host.
- **Root cause**: `detect.rs` checked any VS Code host separately from any Cline extension directory.
- **Fix**: pair Stable, Insiders and Flatpak extension roots with their respective host; exclude Flatpak wrappers from native host discovery.
- **Guardrail**: test a real extension entry point with only a different host edition installed, no hosts, obsolete metadata, and the matching host. Never use settings or extension leftovers as installation evidence.
- **Watch points**: config-writers `detect.rs`, `installed_apps.rs`, quick integration and Agent management discovery.

### New Unix vaults need stable canonical names before creation
- **Symptom**: first launch through a symlinked directory could start an Agent on a pipe/socket name that later clients no longer used.
- **Root cause**: `paths.rs::canonical_unix_vault` canonicalized only the immediate parent, falling back to unresolved aliases when multiple parent directories were missing.
- **Fix**: canonicalize the longest existing ancestor, resolve only the missing suffix, and return filesystem errors rather than silently inventing another namespace.
- **Guardrail**: compare namespaces before/after creating a nested vault under a symlink; preserve existing symlink-plus-parent semantics and normalize parent components only in the missing suffix. Exercise both first launch and reconnection.
- **Watch points**: canonical_vault_dir, namespace_for_vault_dir, AgentClient configuration, Agent launch and service registration.


### Uninstall cannot decode an older status as an absent Agent
- **Symptom**: Windows stop/uninstall skipped an older resident after a protocol bump, leaving the executable running during cleanup.
- **Root cause**: `autostart/windows.rs::stop_agent` used a typed SessionStatus request; its protocol-mismatch error was treated as absence.
- **Fix**: inspect the raw response version and reuse the authenticated older-Agent retirement handshake before waiting for exit.
- **Guardrail**: run a real named-pipe regression that returns an older status, verifies the same token on legacy shutdown and confirms cleanup completes. Restrict legacy retries to shutdown, never credential or provider mutations.
- **Watch points**: autostart/windows.rs, AgentClient::shutdown_older_agent, Agent protocol changes, NSIS cleanup.
