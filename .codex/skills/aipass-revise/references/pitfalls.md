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

### Agent startup paid fixed polling delays
- **Symptom**: first requests after an update were delayed by seconds even after the agent had bound its socket.
- **Root cause**: the nonblocking listener slept 200ms after every empty accept, and supervisors checked child exit only every 10 seconds; desktop setup also risked synchronously waiting on agent repair.
- **Fix**: wake the listener with a Unix socket poll timeout, reduce supervisor checks to 1 second, and keep desktop agent warmup asynchronous.
- **Guardrail**: do not add fixed waits to the agent readiness path; keep launch repair and desktop setup off the Tauri setup critical path.
- **Watch points**: `crates/aipass-agent/src/server.rs`, `src/ipc.rs`, `src/autostart.rs`, and `apps/desktop/src-tauri/src/lib.rs`.

### No remote-change awareness for folder sync
- **Symptom**: iCloud Drive changes made on another device only appeared after a manual "sync now".
- **Root cause**: no file watching existed; iCloud Drive materializes files via the OS and the app never noticed.
- **Fix**: `sync_watch.rs` uses `notify` (FSEvents) with a 2s debounce to trigger `run_sync_local` plus the reload/restart follow-ups; watcher restarts when sync settings change and exits on shutdown.
- **Guardrail**: realtime triggers must reuse the exact same post-sync path (vault reload + proxy reconcile) as manual sync — never fork a second "sync completed" flow.
- **Watch points**: WebDAV has no watcher by design; new folder-based backends must opt into `folder_sync_dir` resolution.

## Tool configuration writes (aipass-agent / config-writers)

### Local diagnostics stopped at rotation limits and runtime boundaries
- **Symptom**: component logs stopped after reaching the daily size cap; proxy failures disappeared after stopping/restarting; failed probes and sync could look successful at the IPC layer.
- **Root cause**: `logging.rs` returned on overflow, `proxy::RuntimeStats` retained only in-memory errors, and request logging relied on transport success and serialized requests to discover their event names.
- **Fix**: shared component writer with size rotation and process locking; static exhaustive request event names, UUID correlation and semantic outcomes; bounded persistent proxy diagnostics linked to upstream attempts. See `docs/local-logging.md`.
- **Guardrail**: never serialize requests for diagnostics; log only allowlisted metadata, retain operation pairs across repetition/rotation, correlate attempts across HTTP/SSE/WebSocket, and test persistence after stop/restart and business failures within successful IPC responses.
- **Watch points**: agent `operation_log.rs`, shared `logging.rs`, client/envelope/server correlation, desktop logger, proxy `diagnostics.rs` and `persist_attempt`. Regression tests cover provider lifecycle, semantic errors/unwinding, concurrent rotation, restart retention and successful fallback.

### Configuration failures lacked an operation trail
- **Symptom**: a failed config write surfaced only a transport error such as `failed to fill whole buffer`, with no way to identify which write was interrupted.
- **Root cause**: tool config preview/apply/rollback handlers had no lifecycle logs, while writes can wait on vault or Codex SQLite state migration.
- **Fix**: handlers now log start, completion, rejection, and write/rollback failures with operation or request IDs, target metadata, and elapsed time; config requests use the long response timeout.
- **Guardrail**: log every configuration operation's lifecycle and correlate apply/rollback failures by operation ID without logging config contents or secrets.
- **Watch points**: `crates/aipass-agent/src/handlers.rs`, `crates/aipass-agent-protocol/src/lib.rs`, and `crates/aipass-config-writers/src/backup.rs`.

## Proxy credential snapshot (proxy_service / handlers)

### New or changed credentials invisible to the running proxy
- **Symptom**: a credential added from the extension, imported, archived/restored, or synced from another device did not work through the local proxy until the proxy was manually restarted.
- **Root cause**: the proxy resolves plaintext credentials into an immutable in-memory `RuntimeConfig` at start/restart (`proxy_service.rs` `runtime_config`); invalidation relied on each handler remembering to call `refresh_proxy_provider_credentials`, and many write paths never did (BrowserSaveDetected, ProviderArchive/Restore, CcSwitchImport, all sync paths, VaultImport).
- **Fix**: `ProxyService::reload_if_running()` plus refresh hooks on every missed path; sync downloads trigger a full reconcile; VaultImport stops the proxy before locking so a stale snapshot cannot outlive the vault it came from.
- **Guardrail**: every vault mutation path must either refresh the proxy snapshot or provably not affect proxy-visible data. When adding a new write path, grep for `refresh_proxy_provider_credentials` / `reload_if_running` call sites and add yours.
- **Watch points**: `crates/aipass-agent/src/handlers.rs` (all provider/secret/sync/import branches), `crates/aipass-agent/src/server.rs` `save_detected_secret`, `crates/aipass-agent/src/session.rs` unlock/lock transitions.

### WebSocket transport must share proxy configuration and invalidation
- **Symptom**: Responses WebSocket clients could not connect to the local proxy; a separate direct WS connector would also bypass configured outbound proxies and leave authenticated sessions alive after credential changes.
- **Root cause**: `crates/aipass-proxy/src/lib.rs` used `serve_connection` without upgrades, and `build_upstream_headers` intentionally removes HTTP hop headers. Runtime refresh originally only replaced the request-time credential snapshot.
- **Fix**: enable Hyper upgrades, negotiate WS through the shared reqwest proxy configuration, and notify upgraded sessions from `ProxyHandle::update_config`. Track usage per response and retry only before the upgrade is committed.
- **Guardrail**: route every new transport through the existing token selection, credential injection and outbound proxy settings; terminate authenticated long-lived sessions when their runtime snapshot changes, including while blocked writing to a slow client. Never replay a committed Responses WS session on another target.
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
- **Fix**: expose enabled target IDs with recent unresolved failures or an open circuit through `ProxyStatus`; include them in degradation and display badges per group and credential while running.
- **Guardrail**: derive target degradation from shared runtime health, including successful fallback, recovery, expiration, and config reload. Keep target IDs distinct across credentials. Enforced by `degraded_targets_follow_recent_failures_circuits_and_recovery`, `proxy_authenticates_fails_over_and_records_usage`, and desktop route component tests.
- **Watch points**: HTTP/model discovery/stream/WebSocket `mark_failure` and `mark_success`, agent stopped status, desktop status polling, route list and editor.

### Empty successful bodies must not commit
- **Symptom**: an upstream could return HTTP 200 with an empty body, leaving the model client with an empty response instead of trying another target.
- **Root cause**: non-stream forwarding treated any 2xx response as successful before checking whether a body was present.
- **Fix**: empty buffered success bodies now mark the target failed and enter the normal fallback chain.
- **Guardrail**: before committing a non-stream upstream response, reject empty bodies and structured error payloads as target failures.
- **Watch points**: `crates/aipass-proxy/src/lib.rs` `forward_request`, silent retry buffering, and usage attempt accounting.

### Validate provider payloads before success accounting
- **Symptom**: a malformed 200 response on a converted route returned 502 without fallback and was counted as successful; model discovery also returned empty/error 200 bodies as healthy results.
- **Root cause**: `crates/aipass-proxy/src/lib.rs` `forward_request` started usage tracking before non-stream conversion; `handle_models_request` skipped the error-body guard used by generation.
- **Fix**: validate non-stream conversion before starting success/usage tracking, and reject empty or structured-error model-list bodies before marking success.
- **Guardrail**: finish response validation before success accounting or committing a response; on provider response conversion failure, continue fallback and record only the failed attempt. Enforced by `invalid_converted_response_fails_over_without_recording_success` and `model_discovery_empty_and_error_success_bodies_fail_over`.
- **Watch points**: `forward_request`, `handle_models_request`, `track_usage_stream`, and converted WebSocket calls to the shared forwarding path.

### Hold duration must bound in-flight waits
- **Symptom**: a short maximum hold duration could still leave HTTP requests or WS handshakes waiting for the much longer per-attempt timeout, and start new attempts after the budget expired.
- **Root cause**: HTTP `forward_request` and native WS handshake loops only checked elapsed time after an entire round, before sleeping; buffered body reads restarted the idle timeout on every chunk.
- **Fix**: share an absolute hold deadline across target selection, response headers, first SSE events, buffered body reads and WS handshakes. Count only targets actually attempted and preserve the last failure across backoff.
- **Guardrail**: enforce the hold deadline on every pre-commit network wait and before each attempt; never apply it to an already committed live stream. Covered by `hold_deadline_bounds_backoff_headers_and_buffered_bodies`, `hold_deadline_does_not_cut_off_a_committed_stream`, and `websocket_hold_deadline_bounds_backoff_and_handshake`.
- **Watch points**: `crates/aipass-proxy/src/lib.rs` forwarding/collection and `websocket.rs` handshake; converted WS shares the HTTP path.

### Circuit-open weights distorted round robin
- **Symptom**: after a high-weight target opened its circuit, one healthy fallback received its weight while another equally weighted target received no requests.
- **Root cause**: `crates/aipass-proxy/src/lib.rs` `select_route_targets` rotated by all enabled targets' weights before removing circuit-open targets.
- **Fix**: remove unavailable targets before calculating weighted rotation.
- **Guardrail**: calculate round-robin weights over the actual eligible target set; keep explicit hold-mode circuit bypass intact. Covered by `round_robin_redistributes_weight_among_available_targets`.
- **Watch points**: shared target selection used by HTTP, model discovery, native WS, and converted WS.

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

### Nightly tag validation and update feeds must agree
- **Symptom**: a repeated daily nightly tag passed release validation but was ignored by the updater feed.
- **Root cause**: `.github/workflows/release.yml` accepted a daily revision suffix while `apps/web/src/worker.ts` still matched only the date.
- **Fix**: align both tag patterns and exercise the public nightly feed against revised, older, draft, and beta release candidates.
- **Guardrail**: when changing release tag syntax, update both release validation and the deployed channel resolver. Verify the public feed selects the published version before declaring the release complete.
- **Watch points**: `.github/workflows/release.yml`, `apps/web/src/worker.ts`, and `apps/web/scripts/worker.test.mjs`.

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
