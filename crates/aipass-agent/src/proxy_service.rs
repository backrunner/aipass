use crate::logging::{write_component_log, AGENT_LOG};
use crate::session::{map_vault_error, ServiceError, ServiceResult};
use aipass_agent_protocol::{
    CredentialAssignment, ModelPriceRule, PricingApplyScope, PricingConfig, PricingGroup,
    ServerTokenResponse, ServerUsageSummary,
};
use aipass_crypto::Ciphertext;
use aipass_provider_registry::{
    AuthScheme, CredentialKind, EndpointKind, InterfaceType, ProviderKind,
};
use aipass_proxy::{
    ProxyConfig, ProxyHandle, ProxyStatus, ResolvedRoute, ResolvedTarget, RuntimeConfig,
    UsageGranularity, UsageRow, UsageStore, UsageTimeseriesPoint,
};
use aipass_storage::atomic_write_bytes;
use aipass_vault::{Vault, VaultError};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;
use zeroize::Zeroize;

const CONFIG_FILE: &str = "server-config.aipstate";
const CONFIG_PURPOSE: &str = "proxy-server-config";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PersistedProxyConfig {
    version: u32,
    payload: Ciphertext,
}

pub struct ProxyService {
    community_bridge: Arc<crate::community::CommunityBridge>,
    claude_bridge: Arc<crate::claude_bridge::ClaudeBridge>,
    vault_dir: PathBuf,
    config: ProxyConfig,
    handle: Option<ProxyHandle>,
    usage: Arc<UsageStore>,
    /// A stop requested while locked cannot rewrite encrypted configuration.
    /// Keep the intent until the next unlocked config load can persist it.
    pending_disabled_persist: bool,
    pending_ws_events: Vec<aipass_proxy::WebsocketCapabilityEvent>,
    /// A provider write can commit before auditing or runtime refresh fails.
    /// Such refreshes must survive later WS success clearing the live evidence.
    pending_ws_refresh: HashSet<Uuid>,
}

impl ProxyService {
    pub(crate) fn community_bridge(&self) -> Arc<crate::community::CommunityBridge> {
        self.community_bridge.clone()
    }
    fn subscription_backend(&self) -> Arc<dyn aipass_proxy::SubscriptionBackend> {
        Arc::new(crate::community::Dispatch {
            claude: self.claude_bridge.clone(),
            community: self.community_bridge.clone(),
        })
    }
    pub(crate) fn claude_bridge(&self) -> Arc<crate::claude_bridge::ClaudeBridge> {
        self.claude_bridge.clone()
    }
    pub(crate) fn begin_ws_probe(
        &self,
        key: [u8; 32],
    ) -> Option<aipass_proxy::WebsocketObservation> {
        self.handle.as_ref()?.begin_websocket_probe(key)
    }
    pub(crate) fn confirm_ws_probe(&self, observation: Option<aipass_proxy::WebsocketObservation>) {
        if let Some(handle) = &self.handle {
            handle.confirm_websocket_probe(observation);
        }
    }
    pub fn new(vault_dir: &Path) -> anyhow::Result<Self> {
        let usage = Arc::new(UsageStore::open(vault_dir.join("proxy-usage.sqlite"))?);
        Ok(Self {
            claude_bridge: Arc::new(crate::claude_bridge::ClaudeBridge::new(vault_dir)),
            community_bridge: Arc::new(crate::community::CommunityBridge::new(vault_dir)),
            vault_dir: vault_dir.to_path_buf(),
            config: ProxyConfig::default(),
            handle: None,
            usage,
            pending_disabled_persist: false,
            pending_ws_events: Vec::new(),
            pending_ws_refresh: HashSet::new(),
        })
    }

    fn capture_ws_events(&mut self) {
        if let Some(handle) = &mut self.handle {
            // The live ledger is authoritative: concurrent WS success may have
            // invalidated a previously pending write before shutdown.
            self.pending_ws_events = handle.stop_with_websocket_capability_events();
        }
    }

    /// Called by the agent, even when no desktop is running. Keep events while
    /// locked or after a write failure; validate against fresh vault config.
    pub(crate) fn persist_ws_capabilities(&mut self, vault: &Vault) -> ServiceResult<bool> {
        if let Some(id) = self.pending_ws_refresh.iter().next().copied() {
            let saved = match vault.get_provider_summary(id) {
                Ok(summary) => summary.supports_websockets == Some(false),
                // Deletion supersedes the attempted preference write. Still
                // refresh any running snapshot before dropping the retry.
                Err(aipass_vault::VaultError::RecordNotFound) => true,
                Err(error) => return Err(map_vault_error(error)),
            };
            self.reload_if_running(vault)?;
            self.pending_ws_refresh.remove(&id);
            if saved {
                if let Some(handle) = &self.handle {
                    for event in handle.websocket_capability_events() {
                        if event.provider_entry_id == id {
                            handle.acknowledge_websocket_capability_event(&event);
                        }
                    }
                }
                self.pending_ws_events
                    .retain(|event| event.provider_entry_id != id);
                // Publish a committed change even when its original capability
                // event has since been invalidated by a concurrent WS success.
                return Ok(true);
            }
        }
        if let Some(handle) = &self.handle {
            self.pending_ws_events = handle.websocket_capability_events();
        }
        if self.pending_ws_events.is_empty() {
            return Ok(false);
        }
        self.load_config(vault)?;
        // Synced or archived providers may remain in the stored selection.
        // Only currently usable targets can confirm a capability event.
        let runtime = self.runtime_config_inner(vault, true)?;
        for event in self.pending_ws_events.clone() {
            let current = runtime
                .routes
                .iter()
                .flat_map(|route| &route.targets)
                .any(|target| {
                    target.config.provider_entry_id == event.provider_entry_id
                        && aipass_proxy::websocket_config_key(target, &runtime.upstream_proxy)
                            == event.config_key
                });
            if !current {
                if let Some(handle) = &self.handle {
                    handle.acknowledge_websocket_capability_event(&event);
                }
                self.pending_ws_events
                    .retain(|pending| pending.id != event.id);
                continue;
            }
            let summary = vault
                .get_provider_summary(event.provider_entry_id)
                .map_err(map_vault_error)?;
            let already_saved = summary.supports_websockets == Some(false);
            if !already_saved {
                let mut write = || {
                    // Register before the write: an error can mean either an
                    // uncommitted record or a committed record with failed audit.
                    self.pending_ws_refresh.insert(event.provider_entry_id);
                    vault.disable_provider_websocket(
                        event.provider_entry_id,
                        aipass_provider_registry::WebsocketWarning {
                            reason: "responses_ws_rejected".into(),
                            status: event.status,
                            detected_at: event.detected_at,
                            config_key: event.config_key,
                        },
                    )
                };
                let result = match &self.handle {
                    Some(handle) => handle.with_websocket_capability_event(&event, write),
                    None => Some(write()),
                };
                match result {
                    Some(result) => result.map_err(map_vault_error)?,
                    None => {
                        self.pending_ws_events
                            .retain(|pending| pending.id != event.id);
                        continue;
                    }
                }
            }
            self.pending_ws_refresh.insert(event.provider_entry_id);
            self.reload_if_running(vault)?;
            self.pending_ws_refresh.remove(&event.provider_entry_id);
            if let Some(handle) = &self.handle {
                handle.acknowledge_websocket_capability_event(&event);
            }
            self.pending_ws_events
                .retain(|pending| pending.id != event.id);
            // Publish this revision before attempting another potentially failing write.
            return Ok(true);
        }
        Ok(false)
    }

    pub fn status(&self) -> ProxyStatus {
        let status = self
            .handle
            .as_ref()
            .map(|handle| handle.status())
            .unwrap_or_else(|| ProxyStatus {
                running: false,
                enabled: self.config.enabled,
                bind_addr: self.config.bind_addr.clone(),
                active_routes: self
                    .config
                    .routes
                    .iter()
                    .filter(|route| route.enabled)
                    .count(),
                requests: 0,
                failures: 0,
                last_error: None,
                degraded: false,
                degraded_target_ids: Vec::new(),
                channels: Vec::new(),
                recent_requests: 0,
                recent_tokens: 0,
                success_rate_bps: 0,
                average_first_token_ms: None,
                in_flight_requests: 0,
                available_channels: 0,
                total_channels: self
                    .config
                    .routes
                    .iter()
                    .filter(|route| route.enabled)
                    .flat_map(|route| route.targets.iter())
                    .filter(|target| target.enabled)
                    .count(),
            });
        status
    }

    pub fn logs(&self) -> ServiceResult<Vec<aipass_agent_protocol::ProxyLogEntry>> {
        self.usage
            .logs()
            .map_err(|err| ServiceError::internal(anyhow::anyhow!(err)))
    }

    pub fn load_config(&mut self, vault: &Vault) -> ServiceResult<ProxyConfig> {
        let path = self.vault_dir.join(CONFIG_FILE);
        if !path.exists() {
            self.pending_disabled_persist = false;
            return Ok(self.config.clone());
        }
        let persisted: PersistedProxyConfig =
            serde_json::from_slice(&std::fs::read(path).map_err(ServiceError::internal)?)
                .map_err(ServiceError::internal)?;
        let bytes = vault
            .decrypt_local_state(CONFIG_PURPOSE, &persisted.payload)
            .map_err(map_vault_error)?;
        self.config = serde_json::from_slice(&bytes).map_err(ServiceError::internal)?;
        let normalized = ensure_route_tokens(&mut self.config);
        if self.pending_disabled_persist {
            self.config.enabled = false;
        }
        if normalized || self.pending_disabled_persist {
            self.save_config(vault)?;
            self.pending_disabled_persist = false;
        }
        Ok(self.config.clone())
    }

    pub fn save_config(&self, vault: &Vault) -> ServiceResult<()> {
        let bytes = serde_json::to_vec(&self.config).map_err(ServiceError::internal)?;
        let payload = vault
            .encrypt_local_state(CONFIG_PURPOSE, &bytes)
            .map_err(map_vault_error)?;
        let persisted = PersistedProxyConfig {
            version: 1,
            payload,
        };
        atomic_write_bytes(
            self.vault_dir.join(CONFIG_FILE),
            &serde_json::to_vec_pretty(&persisted).map_err(ServiceError::internal)?,
        )
        .map_err(ServiceError::internal)
    }

    pub fn config(&mut self, vault: &Vault) -> ServiceResult<ProxyConfig> {
        self.load_config(vault)
    }

    pub fn client_config(&mut self, vault: &Vault) -> ServiceResult<ProxyConfig> {
        self.load_config(vault)
    }

    pub fn set_config(
        &mut self,
        vault: &Vault,
        mut config: ProxyConfig,
    ) -> ServiceResult<ProxyConfig> {
        self.load_config(vault)?;
        let _ = ensure_route_tokens(&mut config);
        validate_config(&config)?;
        let previous = std::mem::replace(&mut self.config, config);
        let was_running = self
            .handle
            .as_ref()
            .is_some_and(|handle| handle.status().running);
        if let Err(err) = self.save_config(vault) {
            self.config = previous;
            return Err(err);
        }
        if let Err(err) = self.apply_runtime_config(vault) {
            self.config = previous;
            let _ = self.save_config(vault);
            if was_running {
                let _ = self.restart(vault);
            }
            return Err(err);
        }
        Ok(self.config.clone())
    }

    pub fn start(&mut self, vault: &Vault) -> ServiceResult<ProxyStatus> {
        if self
            .handle
            .as_ref()
            .is_some_and(|handle| handle.status().running)
        {
            return Err(ServiceError::new(
                aipass_agent_protocol::AgentErrorCode::Conflict,
                "proxy server is already running",
            ));
        }
        self.capture_ws_events();
        self.handle.take();
        self.load_config(vault)?;
        // Prune references to credentials removed elsewhere before validating;
        // a stale target must not block startup.
        self.reconcile_missing_credentials(vault)?;
        validate_config(&self.config)?;
        if !self.config.routes.iter().any(|route| route.enabled) {
            return Err(ServiceError::new(
                aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                "enable at least one proxy route group before starting the proxy",
            ));
        }
        if self
            .config
            .routes
            .iter()
            .any(|route| route.enabled && route.token.is_empty())
        {
            return Err(ServiceError::new(
                aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                "every proxy route group needs a local token",
            ));
        }
        let runtime = self.runtime_config(vault)?;
        let handle = ProxyHandle::start(runtime, self.usage.clone())
            .map_err(|err| ServiceError::internal(anyhow::anyhow!(err)))?;
        handle.set_subscription_backend(self.subscription_backend());
        let previous_enabled = self.config.enabled;
        self.config.enabled = true;
        if let Err(err) = self.save_config(vault) {
            self.config.enabled = previous_enabled;
            drop(handle);
            return Err(err);
        }
        for event in &self.pending_ws_events {
            handle.restore_websocket_capability_event(event);
        }
        self.handle = Some(handle);
        Ok(self.status())
    }

    /// Restore a proxy that was enabled before the agent last stopped.
    ///
    /// The encrypted config is the durable source of truth for this intent;
    /// an explicit stop persists `enabled = false`, so it is not restarted.
    pub fn start_if_enabled(&mut self, vault: &Vault) -> ServiceResult<Option<ProxyStatus>> {
        if self
            .handle
            .as_ref()
            .is_some_and(|handle| handle.status().running)
        {
            return Ok(Some(self.status()));
        }
        let config = self.load_config(vault)?;
        if !config.enabled {
            return Ok(None);
        }
        self.start(vault).map(Some)
    }

    pub fn stop(&mut self) -> ServiceResult<ProxyStatus> {
        self.capture_ws_events();
        self.handle.take();
        self.config.enabled = false;
        Ok(self.status())
    }

    /// Activate a route group for the local proxy.
    pub fn select_route(&mut self, vault: &Vault, route_id: Uuid) -> ServiceResult<ProxyConfig> {
        self.set_route_enabled(vault, route_id, true)
    }

    /// Enable or disable one route group atomically, leaving every other
    /// route — and the rest of the config — untouched.
    pub fn set_route_enabled(
        &mut self,
        vault: &Vault,
        route_id: Uuid,
        enabled: bool,
    ) -> ServiceResult<ProxyConfig> {
        self.load_config(vault)?;
        let route_index = self
            .config
            .routes
            .iter()
            .position(|route| route.id == route_id)
            .ok_or_else(|| {
                ServiceError::new(
                    aipass_agent_protocol::AgentErrorCode::NotFound,
                    "proxy route not found",
                )
            })?;
        if enabled && self.config.routes[route_index].token.is_empty() {
            return Err(ServiceError::new(
                aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                "selected proxy route needs a local token",
            ));
        }
        let previous = self.config.clone();
        self.config.routes[route_index].enabled = enabled;
        if enabled {
            self.config.enabled = true;
        }
        if let Err(err) = self.save_config(vault).and_then(|()| {
            self.handle
                .is_some()
                .then(|| self.restart(vault))
                .transpose()
                .map(|_| ())
        }) {
            self.config = previous;
            let _ = self.save_config(vault);
            if self.handle.is_some() {
                let _ = self.restart(vault);
            }
            return Err(err);
        }
        Ok(self.config.clone())
    }

    /// Stop the runtime while the vault is locked. The encrypted enabled flag
    /// is reconciled on the next unlocked config load.
    pub fn stop_while_locked(&mut self) -> ServiceResult<ProxyStatus> {
        self.pending_disabled_persist = true;
        self.stop()
    }

    pub fn stop_and_save(&mut self, vault: &Vault) -> ServiceResult<ProxyStatus> {
        // Locking wipes management tokens from memory. Reload before saving so
        // an unlock-then-stop sequence cannot persist those blank placeholders.
        self.load_config(vault)?;
        let previous_enabled = self.config.enabled;
        self.config.enabled = false;
        if let Err(err) = self.save_config(vault) {
            self.config.enabled = previous_enabled;
            return Err(err);
        }
        self.capture_ws_events();
        self.handle.take();
        Ok(self.status())
    }

    pub fn lock_for_session(&mut self) {
        // ProxyHandle owns a separate runtime snapshot containing the resolved
        // route credentials. Keep that snapshot alive so an already-running
        // proxy remains available while the vault session is locked, but wipe
        // the redundant route tokens cached by the management service.
        for route in &mut self.config.routes {
            route.token.zeroize();
        }
    }

    pub fn reset(&mut self) -> ServiceResult<()> {
        self.pending_ws_events.clear();
        self.pending_ws_refresh.clear();
        self.handle.take();
        self.config = ProxyConfig::default();
        self.usage
            .clear()
            .map_err(|err| ServiceError::internal(anyhow::anyhow!(err)))
    }

    pub fn restart(&mut self, vault: &Vault) -> ServiceResult<ProxyStatus> {
        let runtime = self.runtime_config(vault)?;
        if let Some(handle) = &self.handle {
            let status = handle.status();
            if status.running && status.bind_addr == runtime.bind_addr {
                handle
                    .update_config(runtime)
                    .map_err(|err| ServiceError::internal(anyhow::anyhow!(err)))?;
                return Ok(self.status());
            }
        }
        self.capture_ws_events();
        self.handle.take();
        let next = ProxyHandle::start(runtime, self.usage.clone())
            .map_err(|err| ServiceError::internal(anyhow::anyhow!(err)))?;
        next.set_subscription_backend(self.subscription_backend());
        for event in &self.pending_ws_events {
            next.restore_websocket_capability_event(event);
        }
        self.handle = Some(next);
        Ok(self.status())
    }

    fn apply_runtime_config(&mut self, vault: &Vault) -> ServiceResult<()> {
        if self.handle.is_none() {
            return Ok(());
        }
        if self.config.enabled && self.config.routes.iter().any(|route| route.enabled) {
            self.restart(vault).map(|_| ())
        } else {
            self.stop_and_save(vault).map(|_| ())
        }
    }

    pub fn rotate_token(
        &mut self,
        vault: &Vault,
        route_id: Uuid,
    ) -> ServiceResult<ServerTokenResponse> {
        self.load_config(vault)?;
        let token = generate_local_token();
        let route_index = self
            .config
            .routes
            .iter()
            .position(|route| route.id == route_id)
            .ok_or_else(|| {
                ServiceError::new(
                    aipass_agent_protocol::AgentErrorCode::NotFound,
                    "proxy route not found",
                )
            })?;
        let previous_token =
            std::mem::replace(&mut self.config.routes[route_index].token, token.clone());
        let was_running = self.handle.is_some();
        let result = self.save_config(vault).and_then(|()| {
            was_running
                .then(|| self.restart(vault))
                .transpose()
                .map(|_| ())
        });
        if let Err(err) = result {
            self.config.routes[route_index].token = previous_token;
            let _ = self.save_config(vault);
            if was_running {
                let _ = self.restart(vault);
            }
            return Err(err);
        }
        Ok(ServerTokenResponse {
            route_id,
            token: token.into(),
        })
    }

    pub fn remove_provider_references(
        &mut self,
        vault: &Vault,
        entry_id: Uuid,
        secret_id: Option<&str>,
    ) -> ServiceResult<bool> {
        self.load_config(vault)?;
        let mut changed = false;
        self.config.routes.retain_mut(|route| {
            let before = route.targets.len();
            route.targets.retain(|target| {
                target.provider_entry_id != entry_id
                    || secret_id.is_some_and(|secret_id| target.secret_id != secret_id)
            });
            changed |= route.targets.len() != before;
            !route.targets.is_empty() || before == 0
        });
        if let Err(err) = self.remove_pricing_assignments(vault, entry_id, secret_id) {
            write_component_log(
                AGENT_LOG,
                "WARN",
                &format!(
                    "failed to remove pricing assignments for provider {entry_id} code={:?}",
                    err.code
                ),
            );
        }
        if !changed {
            return Ok(false);
        }
        self.save_config(vault)?;
        // Match synced deletions: unavailable siblings and an empty target set
        // must not prevent invalidating the live credential snapshot.
        self.reload_if_running(vault)?;
        Ok(true)
    }

    /// Drop route targets whose provider entry or credential is gone for good —
    /// a record deleted on another device and synced in, or a key removed while
    /// the agent was offline. An archived entry keeps its targets: archiving is
    /// recoverable and the credential still exists. Targets that fail to resolve
    /// for any other reason stay untouched, and routes that lose their last
    /// target are dropped.
    pub fn reconcile_missing_credentials(&mut self, vault: &Vault) -> ServiceResult<bool> {
        self.load_config(vault)?;
        let mut removed: Vec<(Uuid, String)> = Vec::new();
        self.config.routes.retain_mut(|route| {
            let before = route.targets.len();
            route.targets.retain(|target| {
                let gone = match vault.get_provider_summary(target.provider_entry_id) {
                    Ok(entry) => {
                        entry.deleted_at.is_some()
                            || !entry
                                .secret_refs
                                .iter()
                                .any(|secret| secret.id == target.secret_id)
                    }
                    Err(VaultError::RecordNotFound) => true,
                    Err(_) => false,
                };
                if gone {
                    removed.push((target.provider_entry_id, target.secret_id.clone()));
                }
                !gone
            });
            !route.targets.is_empty() || before == 0
        });
        if removed.is_empty() {
            return Ok(false);
        }
        for (entry_id, secret_id) in removed {
            if let Err(err) = self.remove_pricing_assignments(vault, entry_id, Some(&secret_id)) {
                write_component_log(
                    AGENT_LOG,
                    "WARN",
                    &format!(
                        "failed to remove pricing assignments for removed credential {secret_id} code={:?}",
                        err.code
                    ),
                );
            }
        }
        self.save_config(vault)?;
        // Reconcile never stops the listener: a synced credential removal must
        // keep the proxy serving its remaining routes, and an emptied route set
        // simply rejects requests until the user configures new targets.
        self.reload_if_running(vault)?;
        Ok(true)
    }

    pub fn refresh_provider_credentials(
        &mut self,
        vault: &Vault,
        entry_id: Uuid,
    ) -> ServiceResult<bool> {
        let running = self
            .handle
            .as_ref()
            .is_some_and(|handle| handle.status().running);
        let result = (|| -> ServiceResult<bool> {
            self.load_config(vault)?;
            // A credential deleted through another path (for example synced
            // in from a peer device) must drop its target rather than fail the
            // whole refresh.
            self.reconcile_missing_credentials(vault)?;
            let referenced = self.config.routes.iter().any(|route| {
                route
                    .targets
                    .iter()
                    .any(|target| target.provider_entry_id == entry_id)
            });
            if !referenced {
                return Ok(false);
            }
            let entry = vault
                .get_provider_summary(entry_id)
                .map_err(map_vault_error)?;
            let base_url = entry
                .endpoints
                .iter()
                .find(|endpoint| endpoint.kind == EndpointKind::Api)
                .and_then(|endpoint| endpoint.url.as_deref())
                .or_else(|| {
                    entry
                        .endpoints
                        .iter()
                        .find_map(|endpoint| endpoint.url.as_deref())
                });
            let mut config_changed = false;
            for target in self
                .config
                .routes
                .iter_mut()
                .flat_map(|route| route.targets.iter_mut())
                .filter(|target| target.provider_entry_id == entry_id)
            {
                let secret = entry
                    .secret_refs
                    .iter()
                    .find(|secret| secret.id == target.secret_id)
                    .ok_or_else(|| {
                        ServiceError::new(
                            aipass_agent_protocol::AgentErrorCode::NotFound,
                            "proxy target credential no longer exists",
                        )
                    })?;
                let auth = secret.effective_auth(&entry.interface_type, &entry.auth_scheme);
                let auth_scheme = proxy_auth_scheme(&auth).ok_or_else(|| {
                    ServiceError::new(
                        aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                        "credential uses an unsupported proxy authentication scheme",
                    )
                })?;
                let next_group = secret.group.clone().or_else(|| {
                    entry
                        .gateway
                        .as_ref()
                        .and_then(|gateway| gateway.group.clone())
                });
                if target.label != secret.label {
                    target.label.clone_from(&secret.label);
                    config_changed = true;
                }
                let key_url = secret.endpoint.as_deref().or(base_url).ok_or_else(|| {
                    ServiceError::new(
                        aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                        "credential needs an API endpoint",
                    )
                })?;
                if target.base_url != key_url {
                    target.base_url = key_url.to_string();
                    config_changed = true;
                }
                if target.auth_scheme != auth_scheme {
                    target.auth_scheme = auth_scheme.to_string();
                    config_changed = true;
                }
                if target.group != next_group {
                    target.group = next_group;
                    config_changed = true;
                }
                // A wire format bound to the key is the upstream protocol for
                // this credential — relay keys in one entry may speak
                // different protocols. Keys without an override keep the
                // configured/route-level protocol untouched.
                if let Some(protocol) = secret
                    .interface_type
                    .as_ref()
                    .and_then(|interface| key_upstream_protocol(interface, &entry))
                {
                    if target.protocol != Some(protocol) {
                        target.protocol = Some(protocol);
                        config_changed = true;
                    }
                }
            }
            if config_changed {
                self.save_config(vault)?;
            }
            if running {
                self.reload_if_running(vault)?;
            }
            Ok(true)
        })();
        if result.is_err() && running {
            // The vault update has already committed. Never keep serving the
            // previous credential snapshot when the replacement cannot load.
            self.capture_ws_events();
            self.handle.take();
            self.config.enabled = false;
            let _ = self.save_config(vault);
        }
        result
    }

    /// Rebuild the runtime credential snapshot from the current vault state
    /// when the proxy is running. A stopped proxy is a no-op: it reads fresh
    /// credentials from the vault on its next start.
    pub fn reload_if_running(&mut self, vault: &Vault) -> ServiceResult<()> {
        let running = self
            .handle
            .as_ref()
            .is_some_and(|handle| handle.status().running);
        if !running {
            return Ok(());
        }
        self.load_config(vault)?;
        let runtime = self.runtime_config_inner(vault, true)?;
        // Sync does not own the listening socket or enabled preference. Missing
        // remote credentials remove only their runtime routes/targets; the
        // remaining proxy stays live and the stored selection can recover later.
        self.handle
            .as_ref()
            .unwrap()
            .update_config(runtime)
            .map_err(|err| ServiceError::internal(anyhow::anyhow!(err)))
    }
}

mod config;
mod pricing;
mod routing;
mod runtime;
use config::*;
use routing::{account_quota, managed_oauth_token, provider_profile};
pub(crate) use routing::{
    key_upstream_protocol, pinned_official_oauth_endpoint, proxy_auth_scheme, upstream_kind,
};

#[cfg(test)]
mod tests;
