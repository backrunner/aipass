use crate::logging::{write_component_log, AGENT_LOG};
use crate::session::{map_vault_error, ServiceError, ServiceResult};
use aipass_agent_protocol::{
    CredentialAssignment, ModelPriceRule, PricingApplyScope, PricingConfig, PricingGroup,
    ServerTokenResponse, ServerUsageSummary,
};
use aipass_crypto::Ciphertext;
use aipass_provider_registry::{AuthScheme, EndpointKind, InterfaceType};
use aipass_proxy::{
    ProxyConfig, ProxyHandle, ProxyStatus, ResolvedRoute, ResolvedTarget, RuntimeConfig, UsageRow,
    UsageStore, UsageTimeseriesPoint,
};
use aipass_storage::atomic_write_bytes;
use aipass_vault::Vault;
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
    vault_dir: PathBuf,
    config: ProxyConfig,
    handle: Option<ProxyHandle>,
    usage: Arc<UsageStore>,
    /// A stop requested while locked cannot rewrite encrypted configuration.
    /// Keep the intent until the next unlocked config load can persist it.
    pending_disabled_persist: bool,
}

impl ProxyService {
    pub fn new(vault_dir: &Path) -> anyhow::Result<Self> {
        let usage = Arc::new(UsageStore::open(vault_dir.join("proxy-usage.sqlite"))?);
        Ok(Self {
            vault_dir: vault_dir.to_path_buf(),
            config: ProxyConfig::default(),
            handle: None,
            usage,
            pending_disabled_persist: false,
        })
    }

    pub fn status(&self) -> ProxyStatus {
        let mut status = self
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
                recent_requests: 0,
                recent_tokens: 0,
            });
        let since = OffsetDateTime::now_utc().unix_timestamp() - 60;
        if let Ok((requests, tokens)) = self.usage.recent_totals(since) {
            status.recent_requests = requests;
            status.recent_tokens = tokens;
        }
        status
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
        let normalized = normalize_unavailable_conversion(&mut self.config)
            | normalize_enabled_routes(&mut self.config)
            | ensure_enabled_route_tokens(&mut self.config);
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
        let _ = ensure_enabled_route_tokens(&mut config);
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
        self.handle.take();
        self.load_config(vault)?;
        validate_config(&self.config)?;
        if self
            .config
            .routes
            .iter()
            .any(|route| route.enabled && route.token.is_empty())
        {
            return Err(ServiceError::new(
                aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                "every enabled route needs a local token",
            ));
        }
        let runtime = self.runtime_config(vault)?;
        let handle = ProxyHandle::start(runtime, self.usage.clone())
            .map_err(|err| ServiceError::internal(anyhow::anyhow!(err)))?;
        let previous_enabled = self.config.enabled;
        self.config.enabled = true;
        if let Err(err) = self.save_config(vault) {
            self.config.enabled = previous_enabled;
            drop(handle);
            return Err(err);
        }
        self.handle = Some(handle);
        Ok(self.status())
    }

    pub fn stop(&mut self) -> ServiceResult<ProxyStatus> {
        self.handle.take();
        self.config.enabled = false;
        Ok(self.status())
    }

    /// Select exactly one route as the active local-proxy group.
    pub fn select_route(&mut self, vault: &Vault, route_id: Uuid) -> ServiceResult<ProxyConfig> {
        self.load_config(vault)?;
        if !self.config.routes.iter().any(|route| route.id == route_id) {
            return Err(ServiceError::new(
                aipass_agent_protocol::AgentErrorCode::NotFound,
                "proxy route not found",
            ));
        }
        if self
            .config
            .routes
            .iter()
            .find(|route| route.id == route_id)
            .is_some_and(|route| route.token.is_empty())
        {
            return Err(ServiceError::new(
                aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                "selected proxy route needs a local token",
            ));
        }
        for route in &mut self.config.routes {
            route.enabled = route.id == route_id;
        }
        self.config.enabled = true;
        self.save_config(vault)?;
        if self.handle.is_some() {
            self.restart(vault)?;
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
        let next = ProxyHandle::start(runtime, self.usage.clone())
            .map_err(|err| ServiceError::internal(anyhow::anyhow!(err)))?;
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

    pub fn usage_summary(
        &self,
        pricing: &PricingConfig,
        list_prices: &[ModelPriceRule],
    ) -> ServiceResult<ServerUsageSummary> {
        let summary = self
            .usage
            .summary(self.cost_resolver(pricing, list_prices))
            .map_err(|err| ServiceError::internal(anyhow::anyhow!(err)))?;
        Ok(ServerUsageSummary {
            request_count: summary.request_count,
            input_tokens: summary.input_tokens,
            output_tokens: summary.output_tokens,
            cache_read_tokens: summary.cache_read_tokens,
            cache_creation_tokens: summary.cache_creation_tokens,
            estimated_cost_micros: summary.estimated_cost_micros,
            attempt_count: summary.attempt_count,
            completed_attempts: summary.completed_attempts,
            successful_attempts: summary.successful_attempts,
            success_rate_bps: summary.success_rate_bps,
            average_first_token_ms: summary.average_first_token_ms,
            providers: summary.providers,
            models: summary.models,
        })
    }

    pub fn usage_timeseries(
        &self,
        days: u32,
        pricing: &PricingConfig,
        list_prices: &[ModelPriceRule],
    ) -> ServiceResult<Vec<UsageTimeseriesPoint>> {
        self.usage
            .timeseries(days, self.cost_resolver(pricing, list_prices))
            .map_err(|err| ServiceError::internal(anyhow::anyhow!(err)))
    }

    fn cost_resolver(
        &self,
        pricing: &PricingConfig,
        list_prices: &[ModelPriceRule],
    ) -> impl Fn(&UsageRow) -> u64 {
        let config = pricing.clone();
        let overrides = self.config.pricing.clone();
        let list_prices = list_prices.to_vec();
        move |row: &UsageRow| {
            crate::pricing::resolve_cost(
                &config,
                &overrides,
                &list_prices,
                row.provider_entry_id,
                &row.secret_id,
                row.model.as_deref(),
                row.started_at,
                row.input_tokens,
                row.output_tokens,
                row.cache_read_tokens,
                row.cache_creation_tokens,
            )
        }
    }

    pub fn pricing_config(&self, vault: &Vault) -> ServiceResult<PricingConfig> {
        crate::pricing::load_pricing_config(&self.vault_dir, vault)
    }

    pub fn set_pricing_assignment(
        &self,
        vault: &Vault,
        entry_id: Uuid,
        secret_id: String,
        group_id: Option<Uuid>,
        multiplier: f64,
    ) -> ServiceResult<PricingConfig> {
        let mut config = crate::pricing::load_pricing_config(&self.vault_dir, vault)?;
        match config
            .assignments
            .iter_mut()
            .find(|item| item.entry_id == entry_id && item.secret_id == secret_id)
        {
            Some(existing) => {
                existing.group_id = group_id;
                existing.multiplier = multiplier;
            }
            None => config.assignments.push(CredentialAssignment {
                entry_id,
                secret_id,
                group_id,
                multiplier,
            }),
        }
        crate::pricing::save_pricing_config(&self.vault_dir, vault, &config)?;
        Ok(config)
    }

    pub fn upsert_pricing_group(
        &self,
        vault: &Vault,
        group: PricingGroup,
        apply_scope: PricingApplyScope,
    ) -> ServiceResult<PricingConfig> {
        let mut config = crate::pricing::load_pricing_config(&self.vault_dir, vault)?;
        let mut group = group;
        match apply_scope {
            PricingApplyScope::AllHistory => {
                // All history is repriced with the incoming rule set: collapse
                // every supplied version to the epoch and replace the group.
                for version in &mut group.versions {
                    version.effective_from = 0;
                }
                normalize_versions(&mut group.versions);
                match config.groups.iter_mut().find(|item| item.id == group.id) {
                    Some(existing) => *existing = group,
                    None => config.groups.push(group),
                }
            }
            PricingApplyScope::FromNow => {
                // History keeps its prices: the incoming rules take effect now
                // and are appended to the group's version timeline.
                let now = OffsetDateTime::now_utc().unix_timestamp();
                for version in &mut group.versions {
                    version.effective_from = now;
                }
                match config.groups.iter_mut().find(|item| item.id == group.id) {
                    Some(existing) => {
                        existing.name = group.name;
                        existing.versions.extend(group.versions);
                        normalize_versions(&mut existing.versions);
                    }
                    None => {
                        normalize_versions(&mut group.versions);
                        config.groups.push(group);
                    }
                }
            }
        }
        crate::pricing::save_pricing_config(&self.vault_dir, vault, &config)?;
        Ok(config)
    }

    pub fn delete_pricing_group(
        &self,
        vault: &Vault,
        group_id: Uuid,
    ) -> ServiceResult<PricingConfig> {
        let mut config = crate::pricing::load_pricing_config(&self.vault_dir, vault)?;
        config.groups.retain(|group| group.id != group_id);
        for assignment in &mut config.assignments {
            if assignment.group_id == Some(group_id) {
                assignment.group_id = None;
            }
        }
        crate::pricing::save_pricing_config(&self.vault_dir, vault, &config)?;
        Ok(config)
    }

    pub fn delete_pricing_group_version(
        &self,
        vault: &Vault,
        group_id: Uuid,
        effective_from: i64,
    ) -> ServiceResult<PricingConfig> {
        let mut config = crate::pricing::load_pricing_config(&self.vault_dir, vault)?;
        let group = config
            .groups
            .iter_mut()
            .find(|group| group.id == group_id)
            .ok_or_else(|| {
                ServiceError::new(
                    aipass_agent_protocol::AgentErrorCode::NotFound,
                    "pricing group not found",
                )
            })?;
        group
            .versions
            .retain(|version| version.effective_from != effective_from);
        crate::pricing::save_pricing_config(&self.vault_dir, vault, &config)?;
        Ok(config)
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
                    "failed to remove pricing assignments for provider {entry_id}: {}",
                    err.message
                ),
            );
        }
        if !changed {
            return Ok(false);
        }
        self.save_config(vault)?;
        if self.handle.is_some() {
            if self.config.enabled && self.config.routes.iter().any(|route| route.enabled) {
                self.restart(vault)?;
            } else {
                self.stop_and_save(vault)?;
            }
        }
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
                })
                .ok_or_else(|| {
                    ServiceError::new(
                        aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                        format!("provider {} has no API endpoint", entry.title),
                    )
                })?;
            let auth_scheme = proxy_auth_scheme(&entry.auth_scheme).ok_or_else(|| {
                ServiceError::new(
                    aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                    format!(
                        "provider {} uses an unsupported proxy authentication scheme",
                        entry.title
                    ),
                )
            });
            let auth_scheme = auth_scheme?;
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
                if target.base_url != base_url {
                    target.base_url = base_url.to_string();
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
            }
            if config_changed {
                self.save_config(vault)?;
            }
            if running {
                self.restart(vault)?;
            }
            Ok(true)
        })();
        if result.is_err() && running {
            // The vault update has already committed. Never keep serving the
            // previous credential snapshot when the replacement cannot load.
            self.handle.take();
            self.config.enabled = false;
            let _ = self.save_config(vault);
        }
        result
    }

    fn remove_pricing_assignments(
        &self,
        vault: &Vault,
        entry_id: Uuid,
        secret_id: Option<&str>,
    ) -> ServiceResult<()> {
        let mut config = crate::pricing::load_pricing_config(&self.vault_dir, vault)?;
        let before = config.assignments.len();
        config.assignments.retain(|assignment| {
            assignment.entry_id != entry_id
                || secret_id.is_some_and(|secret_id| assignment.secret_id != secret_id)
        });
        if config.assignments.len() != before {
            crate::pricing::save_pricing_config(&self.vault_dir, vault, &config)?;
        }
        Ok(())
    }

    fn runtime_config(&self, vault: &Vault) -> ServiceResult<RuntimeConfig> {
        let mut routes = Vec::new();
        for route in self.config.routes.iter().filter(|route| route.enabled) {
            let mut targets = Vec::new();
            for target in route.targets.iter().filter(|target| target.enabled) {
                let entry = vault
                    .get_provider_summary(target.provider_entry_id)
                    .map_err(map_vault_error)?;
                let credential = entry
                    .secret_refs
                    .iter()
                    .find(|secret| secret.id == target.secret_id)
                    .ok_or_else(|| {
                        ServiceError::new(
                            aipass_agent_protocol::AgentErrorCode::NotFound,
                            "proxy target credential no longer exists",
                        )
                    })?;
                let interface = credential
                    .interface_type
                    .as_ref()
                    .unwrap_or(&entry.interface_type);
                if !interface_supports_proxy_protocol(interface, route.upstream_protocol) {
                    return Err(ServiceError::new(
                        aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                        format!(
                            "proxy target {} no longer supports the route protocol",
                            target.label
                        ),
                    ));
                }
                let mut provider_headers = vault
                    .reveal_provider_headers(target.provider_entry_id)
                    .map_err(map_vault_error)?;
                let api_key =
                    match vault.reveal_secret_field(target.provider_entry_id, &target.secret_id) {
                        Ok(api_key) => api_key,
                        Err(err) => {
                            for (_, value) in &mut provider_headers {
                                value.zeroize();
                            }
                            return Err(map_vault_error(err));
                        }
                    };
                let mut target_config = target.clone();
                for (name, value) in provider_headers {
                    if let Some((_, existing)) = target_config
                        .headers
                        .iter_mut()
                        .find(|(existing, _)| existing.eq_ignore_ascii_case(&name))
                    {
                        existing.zeroize();
                        *existing = value;
                    } else {
                        target_config.headers.push((name, value));
                    }
                }
                targets.push(ResolvedTarget {
                    config: target_config,
                    api_key,
                });
            }
            if targets.is_empty() {
                return Err(ServiceError::new(
                    aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                    format!("route {} has no enabled targets", route.name),
                ));
            }
            let runtime_route = aipass_proxy::ProxyRouteConfig {
                id: route.id,
                name: route.name.clone(),
                token: String::new(),
                inbound_protocol: route.inbound_protocol,
                upstream_protocol: route.upstream_protocol,
                conversion_enabled: route.conversion_enabled,
                strategy: route.strategy,
                targets: Vec::new(),
                retry: route.retry.clone(),
                enabled: route.enabled,
            };
            routes.push(ResolvedRoute {
                config: runtime_route,
                local_token: route.token.clone(),
                targets,
            });
        }
        let mut runtime = RuntimeConfig::from_routes(self.config.bind_addr.clone(), routes);
        runtime.pricing = self.config.pricing.clone();
        Ok(runtime)
    }
}

fn proxy_auth_scheme(auth_scheme: &AuthScheme) -> Option<&'static str> {
    match auth_scheme {
        AuthScheme::Bearer => Some("bearer"),
        AuthScheme::CustomHeader => Some("custom_header"),
        AuthScheme::XApiKey => Some("x_api_key"),
        AuthScheme::AzureApiKey => Some("azure_api_key"),
        AuthScheme::GoogleApiKey | AuthScheme::AwsProfile => None,
    }
}

fn interface_supports_proxy_protocol(
    interface: &InterfaceType,
    protocol: aipass_proxy::Protocol,
) -> bool {
    match interface {
        InterfaceType::AnthropicMessages => protocol == aipass_proxy::Protocol::AnthropicMessages,
        InterfaceType::OpenAiCompatible | InterfaceType::AzureOpenAi => matches!(
            protocol,
            aipass_proxy::Protocol::OpenAiResponses | aipass_proxy::Protocol::OpenAiChatCompletions
        ),
        InterfaceType::Gemini | InterfaceType::Bedrock | InterfaceType::CustomHttp => false,
    }
}

fn normalize_versions(versions: &mut Vec<aipass_agent_protocol::GroupPriceVersion>) {
    versions.sort_by_key(|version| version.effective_from);
    let mut deduped: Vec<aipass_agent_protocol::GroupPriceVersion> =
        Vec::with_capacity(versions.len());
    for version in versions.drain(..) {
        match deduped.last_mut() {
            Some(last) if last.effective_from == version.effective_from => *last = version,
            _ => deduped.push(version),
        }
    }
    *versions = deduped;
}

fn normalize_unavailable_conversion(config: &mut ProxyConfig) -> bool {
    let mut changed = false;
    for route in &mut config.routes {
        if route.conversion_enabled || route.upstream_protocol != route.inbound_protocol {
            route.conversion_enabled = false;
            route.upstream_protocol = route.inbound_protocol;
            changed = true;
        }
    }
    changed
}

fn normalize_enabled_routes(config: &mut ProxyConfig) -> bool {
    let mut found_enabled = false;
    let mut changed = false;
    for route in &mut config.routes {
        if !route.enabled {
            continue;
        }
        if found_enabled {
            route.enabled = false;
            changed = true;
        } else {
            found_enabled = true;
        }
    }
    changed
}

fn generate_local_token() -> String {
    format!("sk-{}", Uuid::new_v4().simple())
}

fn ensure_enabled_route_tokens(config: &mut ProxyConfig) -> bool {
    let mut changed = false;
    for route in config
        .routes
        .iter_mut()
        .filter(|route| route.enabled && route.token.trim().is_empty())
    {
        route.token = generate_local_token();
        changed = true;
    }
    changed
}

fn validate_config(config: &ProxyConfig) -> ServiceResult<()> {
    let bind_addr = config
        .bind_addr
        .parse::<std::net::SocketAddr>()
        .map_err(|_| {
            ServiceError::new(
                aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                "proxy bind address must be host:port",
            )
        })?;
    if bind_addr.port() == 0 {
        return Err(ServiceError::new(
            aipass_agent_protocol::AgentErrorCode::ValidationFailed,
            "proxy bind port must be greater than zero",
        ));
    }
    if config
        .routes
        .iter()
        .any(|route| route.conversion_enabled || route.inbound_protocol != route.upstream_protocol)
    {
        return Err(ServiceError::new(
            aipass_agent_protocol::AgentErrorCode::ValidationFailed,
            "protocol conversion is not available in this release",
        ));
    }
    if config.routes.iter().filter(|route| route.enabled).count() > 1 {
        return Err(ServiceError::new(
            aipass_agent_protocol::AgentErrorCode::ValidationFailed,
            "only one proxy route group can be enabled",
        ));
    }
    if config
        .routes
        .iter()
        .any(|route| route.enabled && route.token.trim().is_empty())
    {
        return Err(ServiceError::new(
            aipass_agent_protocol::AgentErrorCode::ValidationFailed,
            "every enabled proxy route needs a local token",
        ));
    }
    let mut route_ids = HashSet::new();
    let mut target_ids = HashSet::new();
    for route in &config.routes {
        if !route_ids.insert(route.id) {
            return Err(ServiceError::new(
                aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                "proxy route ids must be unique",
            ));
        }
        for target in &route.targets {
            if !target_ids.insert(target.id) {
                return Err(ServiceError::new(
                    aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                    "proxy target ids must be unique",
                ));
            }
            if !target.enabled {
                continue;
            }
            let url = reqwest::Url::parse(&target.base_url).map_err(|_| {
                ServiceError::new(
                    aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                    format!("proxy target {} has an invalid base URL", target.label),
                )
            })?;
            if !matches!(url.scheme(), "http" | "https") {
                return Err(ServiceError::new(
                    aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                    format!(
                        "proxy target {} must use an HTTP or HTTPS base URL",
                        target.label
                    ),
                ));
            }
            if !matches!(
                target.auth_scheme.as_str(),
                "bearer" | "custom_header" | "x_api_key" | "azure_api_key"
            ) {
                return Err(ServiceError::new(
                    aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                    format!(
                        "proxy target {} uses an unsupported authentication scheme",
                        target.label
                    ),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aipass_crypto::SecretString;
    use aipass_provider_registry::{AuthScheme, InterfaceType, ProviderEndpoint, ProviderKind};
    use aipass_proxy::{ProxyRouteConfig, ProxyTargetConfig, RetryPolicy, RouteStrategy};
    use aipass_vault::{ProviderEntryInput, ProviderEntryUpdateInput, SecretMetadataInput};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    fn config_with_token(token: &str) -> ProxyConfig {
        ProxyConfig {
            routes: vec![ProxyRouteConfig {
                id: Uuid::new_v4(),
                name: "test".into(),
                token: token.into(),
                inbound_protocol: aipass_proxy::Protocol::OpenAiResponses,
                upstream_protocol: aipass_proxy::Protocol::OpenAiResponses,
                conversion_enabled: false,
                strategy: RouteStrategy::Fallback,
                targets: Vec::new(),
                retry: RetryPolicy::default(),
                enabled: true,
            }],
            ..ProxyConfig::default()
        }
    }

    fn provider_input(api_key: &str, endpoint: String, header: &str) -> ProviderEntryInput {
        ProviderEntryInput {
            title: "Proxy upstream".into(),
            provider_kind: ProviderKind::Unknown,
            provider_id: None,
            domains: Vec::new(),
            favicon_url: None,
            endpoints: vec![ProviderEndpoint::api(endpoint)],
            interface_type: InterfaceType::OpenAiCompatible,
            auth_scheme: AuthScheme::Bearer,
            api_key: api_key.into(),
            secret_label: None,
            default_model: None,
            model_aliases: Vec::new(),
            headers: vec![("x-provider-header".into(), header.into())],
            quota: None,
            gateway: None,
            tags: Vec::new(),
            notes: None,
            secret_metadata: SecretMetadataInput::default(),
        }
    }

    #[test]
    fn config_accepts_plaintext_token() {
        let config = config_with_token("matching-token");
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn saving_an_enabled_route_generates_a_missing_token() {
        let temp = tempfile::tempdir().expect("tempdir");
        let creation = Vault::create(
            temp.path(),
            &SecretString::new("correct horse battery staple"),
        )
        .expect("create vault");
        let mut service = ProxyService::new(temp.path()).expect("proxy service");

        let saved = service
            .set_config(&creation.vault, config_with_token(""))
            .expect("save config");

        assert!(saved.routes[0].token.starts_with("sk-"));
        assert!(!saved.routes[0].token.trim().is_empty());
    }

    #[test]
    fn loading_a_legacy_enabled_route_generates_and_persists_a_token() {
        let temp = tempfile::tempdir().expect("tempdir");
        let creation = Vault::create(
            temp.path(),
            &SecretString::new("correct horse battery staple"),
        )
        .expect("create vault");
        let mut service = ProxyService::new(temp.path()).expect("proxy service");
        service.config = config_with_token("");
        service
            .save_config(&creation.vault)
            .expect("save legacy config");

        let mut reloaded = ProxyService::new(temp.path()).expect("reloaded proxy service");
        let migrated = reloaded
            .load_config(&creation.vault)
            .expect("migrate legacy config");
        assert!(migrated.routes[0].token.starts_with("sk-"));

        let generated = migrated.routes[0].token.clone();
        let mut persisted = ProxyService::new(temp.path()).expect("persisted proxy service");
        assert_eq!(
            persisted
                .load_config(&creation.vault)
                .expect("load persisted config")
                .routes[0]
                .token,
            generated
        );
    }

    #[test]
    fn client_config_returns_stored_plaintext_token() {
        let temp = tempfile::tempdir().expect("tempdir");
        let creation = Vault::create(
            temp.path(),
            &SecretString::new("correct horse battery staple"),
        )
        .expect("create vault");
        let mut service = ProxyService::new(temp.path()).expect("proxy service");
        let token = "matching-token";

        let saved_config = service
            .set_config(&creation.vault, config_with_token(token))
            .expect("save config");
        assert_eq!(saved_config.routes[0].token, token);

        let client_config = service
            .client_config(&creation.vault)
            .expect("load client config");
        assert_eq!(client_config.routes[0].token, token);
    }

    #[test]
    fn locking_session_keeps_runtime_credentials_available_to_proxy() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut service = ProxyService::new(temp.path()).expect("proxy service");

        let upstream = TcpListener::bind("127.0.0.1:0").expect("bind upstream");
        let upstream_addr = upstream.local_addr().expect("upstream address");
        let (request_tx, request_rx) = std::sync::mpsc::channel();
        let upstream_thread = std::thread::spawn(move || {
            let (mut stream, _) = upstream.accept().expect("accept proxy request");
            let mut request = vec![0_u8; 8192];
            let count = stream.read(&mut request).expect("read proxy request");
            request.truncate(count);
            request_tx
                .send(String::from_utf8_lossy(&request).to_string())
                .expect("capture proxy request");
            let body = r#"{"id":"response-test","status":"completed","output":[]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("write upstream response");
        });

        let proxy_probe = TcpListener::bind("127.0.0.1:0").expect("reserve proxy address");
        let proxy_addr = proxy_probe.local_addr().expect("proxy address");
        drop(proxy_probe);
        let local_token = "aipass-local-token";
        let upstream_api_key = "upstream-secret";
        service.config = config_with_token(local_token);
        let runtime_route = ResolvedRoute {
            config: service.config.routes[0].clone(),
            local_token: local_token.into(),
            targets: vec![ResolvedTarget {
                config: ProxyTargetConfig {
                    id: Uuid::new_v4(),
                    provider_entry_id: Uuid::new_v4(),
                    secret_id: "primary".into(),
                    label: "primary".into(),
                    base_url: format!("http://{upstream_addr}/v1"),
                    auth_scheme: "bearer".into(),
                    headers: Vec::new(),
                    group: None,
                    priority: 0,
                    weight: 1,
                    enabled: true,
                },
                api_key: upstream_api_key.into(),
            }],
        };
        service.handle = Some(
            ProxyHandle::start(
                RuntimeConfig::from_routes(proxy_addr.to_string(), vec![runtime_route]),
                service.usage.clone(),
            )
            .expect("start proxy"),
        );

        service.lock_for_session();

        assert!(service.status().running);
        assert!(service.config.routes[0].token.is_empty());

        let response = reqwest::blocking::Client::new()
            .post(format!("http://{proxy_addr}/v1/responses"))
            .bearer_auth(local_token)
            .json(&serde_json::json!({"model": "gpt-test", "input": "hello"}))
            .send()
            .expect("request through locked proxy");
        assert!(response.status().is_success());
        let upstream_request = request_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("upstream request");
        assert!(upstream_request
            .to_ascii_lowercase()
            .contains("authorization: bearer upstream-secret"));
        upstream_thread.join().expect("upstream thread");
    }

    #[test]
    fn provider_update_refreshes_running_credentials_and_headers() {
        let temp = tempfile::tempdir().expect("tempdir");
        let creation = Vault::create(
            temp.path(),
            &SecretString::new("correct horse battery staple"),
        )
        .expect("create vault");
        let upstream = TcpListener::bind("127.0.0.1:0").expect("bind upstream");
        let upstream_addr = upstream.local_addr().expect("upstream address");
        let (request_tx, request_rx) = std::sync::mpsc::channel();
        let upstream_thread = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = upstream.accept().expect("accept proxy request");
                let mut request = vec![0_u8; 8192];
                let count = stream.read(&mut request).expect("read proxy request");
                request.truncate(count);
                request_tx
                    .send(String::from_utf8_lossy(&request).to_string())
                    .expect("capture proxy request");
                let body = r#"{"id":"response-test","status":"completed","output":[]}"#;
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .expect("write upstream response");
            }
        });

        let provider_id = creation
            .vault
            .add_provider(provider_input(
                "old-upstream-key",
                format!("http://{upstream_addr}/v1"),
                "old-header",
            ))
            .expect("add provider");
        let secret_id = creation
            .vault
            .get_provider_summary(provider_id)
            .expect("provider summary")
            .secret_refs[0]
            .id
            .clone();
        let proxy_probe = TcpListener::bind("127.0.0.1:0").expect("reserve proxy address");
        let proxy_addr = proxy_probe.local_addr().expect("proxy address");
        drop(proxy_probe);
        let local_token = "aipass-provider-refresh";
        let mut service = ProxyService::new(temp.path()).expect("proxy service");
        service.config = config_with_token(local_token);
        service.config.bind_addr = proxy_addr.to_string();
        service.config.routes[0].targets = vec![ProxyTargetConfig {
            id: Uuid::new_v4(),
            provider_entry_id: provider_id,
            secret_id,
            label: "primary".into(),
            base_url: format!("http://{upstream_addr}/v1"),
            auth_scheme: "bearer".into(),
            headers: Vec::new(),
            group: None,
            priority: 0,
            weight: 1,
            enabled: true,
        }];
        service
            .save_config(&creation.vault)
            .expect("save proxy config");
        service.start(&creation.vault).expect("start proxy");

        let request = || {
            reqwest::blocking::Client::new()
                .post(format!("http://{proxy_addr}/v1/responses"))
                .bearer_auth(local_token)
                .body("{}")
                .send()
                .expect("proxy request")
                .error_for_status()
                .expect("proxy status")
                .text()
                .expect("proxy body");
        };
        request();

        creation
            .vault
            .update_provider(
                provider_id,
                ProviderEntryUpdateInput {
                    title: "Proxy upstream".into(),
                    provider_kind: ProviderKind::Unknown,
                    provider_id: None,
                    domains: Vec::new(),
                    favicon_url: None,
                    endpoints: vec![ProviderEndpoint::api(format!("http://{upstream_addr}/v1"))],
                    interface_type: InterfaceType::OpenAiCompatible,
                    auth_scheme: AuthScheme::Bearer,
                    api_key: Some("new-upstream-key".into()),
                    secret_label: None,
                    default_model: None,
                    model_aliases: Vec::new(),
                    headers: Some(vec![("x-provider-header".into(), "new-header".into())]),
                    quota: None,
                    gateway: None,
                    tags: Vec::new(),
                    notes: None,
                },
            )
            .expect("update provider");
        assert!(service
            .refresh_provider_credentials(&creation.vault, provider_id)
            .expect("refresh proxy"));
        request();

        let first = request_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("first upstream request")
            .to_ascii_lowercase();
        let second = request_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("second upstream request")
            .to_ascii_lowercase();
        assert!(first.contains("authorization: bearer old-upstream-key"));
        assert!(first.contains("x-provider-header: old-header"));
        assert!(second.contains("authorization: bearer new-upstream-key"));
        assert!(second.contains("x-provider-header: new-header"));
        upstream_thread.join().expect("upstream thread");
    }

    #[test]
    fn failed_provider_refresh_stops_the_stale_runtime() {
        let temp = tempfile::tempdir().expect("tempdir");
        let creation = Vault::create(
            temp.path(),
            &SecretString::new("correct horse battery staple"),
        )
        .expect("create vault");
        let provider_id = creation
            .vault
            .add_provider(provider_input(
                "old-upstream-key",
                "http://127.0.0.1:9/v1".into(),
                "old-header",
            ))
            .expect("add provider");
        let secret_id = creation
            .vault
            .get_provider_summary(provider_id)
            .expect("provider summary")
            .secret_refs[0]
            .id
            .clone();
        let proxy_probe = TcpListener::bind("127.0.0.1:0").expect("reserve proxy address");
        let proxy_addr = proxy_probe.local_addr().expect("proxy address");
        drop(proxy_probe);
        let mut service = ProxyService::new(temp.path()).expect("proxy service");
        service.config = config_with_token("aipass-stale-refresh");
        service.config.bind_addr = proxy_addr.to_string();
        service.config.routes[0].targets = vec![ProxyTargetConfig {
            id: Uuid::new_v4(),
            provider_entry_id: provider_id,
            secret_id,
            label: "primary".into(),
            base_url: "http://127.0.0.1:9/v1".into(),
            auth_scheme: "bearer".into(),
            headers: Vec::new(),
            group: None,
            priority: 0,
            weight: 1,
            enabled: true,
        }];
        service
            .save_config(&creation.vault)
            .expect("save proxy config");
        service.start(&creation.vault).expect("start proxy");
        assert!(service.status().running);

        creation
            .vault
            .update_provider(
                provider_id,
                ProviderEntryUpdateInput {
                    title: "Proxy upstream".into(),
                    provider_kind: ProviderKind::Unknown,
                    provider_id: None,
                    domains: Vec::new(),
                    favicon_url: None,
                    endpoints: vec![ProviderEndpoint::api("http://127.0.0.1:9/v1")],
                    interface_type: InterfaceType::OpenAiCompatible,
                    auth_scheme: AuthScheme::GoogleApiKey,
                    api_key: None,
                    secret_label: None,
                    default_model: None,
                    model_aliases: Vec::new(),
                    headers: None,
                    quota: None,
                    gateway: None,
                    tags: Vec::new(),
                    notes: None,
                },
            )
            .expect("update provider");

        assert!(service
            .refresh_provider_credentials(&creation.vault, provider_id)
            .is_err());
        assert!(!service.status().running);
        assert!(!service.config.enabled);

        let mut reloaded = ProxyService::new(temp.path()).expect("reloaded proxy service");
        assert!(
            !reloaded
                .load_config(&creation.vault)
                .expect("load disabled config")
                .enabled
        );
    }

    #[test]
    fn runtime_config_rejects_provider_protocol_drift() {
        let temp = tempfile::tempdir().expect("tempdir");
        let creation = Vault::create(
            temp.path(),
            &SecretString::new("correct horse battery staple"),
        )
        .expect("create vault");
        let provider_id = creation
            .vault
            .add_provider(provider_input(
                "upstream-key",
                "http://127.0.0.1:9/v1".into(),
                "header",
            ))
            .expect("add provider");
        let secret_id = creation
            .vault
            .get_provider_summary(provider_id)
            .expect("provider summary")
            .secret_refs[0]
            .id
            .clone();
        let mut service = ProxyService::new(temp.path()).expect("proxy service");
        service.config = config_with_token("aipass-protocol-drift");
        service.config.routes[0].targets = vec![ProxyTargetConfig {
            id: Uuid::new_v4(),
            provider_entry_id: provider_id,
            secret_id,
            label: "primary".into(),
            base_url: "http://127.0.0.1:9/v1".into(),
            auth_scheme: "bearer".into(),
            headers: Vec::new(),
            group: None,
            priority: 0,
            weight: 1,
            enabled: true,
        }];
        assert!(service.runtime_config(&creation.vault).is_ok());

        creation
            .vault
            .update_provider(
                provider_id,
                ProviderEntryUpdateInput {
                    title: "Proxy upstream".into(),
                    provider_kind: ProviderKind::Unknown,
                    provider_id: None,
                    domains: Vec::new(),
                    favicon_url: None,
                    endpoints: vec![ProviderEndpoint::api("http://127.0.0.1:9/v1")],
                    interface_type: InterfaceType::AnthropicMessages,
                    auth_scheme: AuthScheme::XApiKey,
                    api_key: None,
                    secret_label: None,
                    default_model: None,
                    model_aliases: Vec::new(),
                    headers: None,
                    quota: None,
                    gateway: None,
                    tags: Vec::new(),
                    notes: None,
                },
            )
            .expect("update provider protocol");

        let error = service
            .runtime_config(&creation.vault)
            .expect_err("protocol drift must be rejected");
        assert_eq!(
            error.code,
            aipass_agent_protocol::AgentErrorCode::ValidationFailed
        );
    }

    #[test]
    fn stop_while_locked_is_persisted_after_the_next_unlock() {
        let temp = tempfile::tempdir().expect("tempdir");
        let creation = aipass_vault::Vault::create(
            temp.path(),
            &SecretString::new("correct horse battery staple"),
        )
        .expect("create vault");
        let mut service = ProxyService::new(temp.path()).expect("proxy service");
        service.config = config_with_token("local-token");
        service.config.enabled = true;
        service
            .save_config(&creation.vault)
            .expect("save enabled config");

        let stopped = service.stop_while_locked().expect("stop while locked");
        assert!(!stopped.running);
        assert!(!stopped.enabled);
        assert!(service.pending_disabled_persist);

        service
            .load_config(&creation.vault)
            .expect("reconcile after unlock");
        assert!(!service.config.enabled);
        assert!(!service.pending_disabled_persist);

        let mut reloaded = ProxyService::new(temp.path()).expect("reloaded service");
        assert!(
            !reloaded
                .load_config(&creation.vault)
                .expect("load persisted config")
                .enabled
        );
    }

    #[test]
    fn stopping_after_unlock_does_not_persist_lock_scrubbed_tokens() {
        let temp = tempfile::tempdir().expect("tempdir");
        let creation = aipass_vault::Vault::create(
            temp.path(),
            &SecretString::new("correct horse battery staple"),
        )
        .expect("create vault");
        let mut service = ProxyService::new(temp.path()).expect("proxy service");
        service.config = config_with_token("local-token");
        service.config.enabled = true;
        service
            .save_config(&creation.vault)
            .expect("save enabled config");

        service.lock_for_session();
        assert!(service.config.routes[0].token.is_empty());
        service
            .stop_and_save(&creation.vault)
            .expect("stop after unlock");

        let mut reloaded = ProxyService::new(temp.path()).expect("reloaded service");
        let config = reloaded
            .load_config(&creation.vault)
            .expect("load stopped config");
        assert!(!config.enabled);
        assert_eq!(config.routes[0].token, "local-token");
    }

    #[test]
    fn stopping_after_the_last_enabled_route_persists_disabled_state() {
        let temp = tempfile::tempdir().expect("tempdir");
        let creation = Vault::create(
            temp.path(),
            &SecretString::new("correct horse battery staple"),
        )
        .expect("create vault");
        let mut service = ProxyService::new(temp.path()).expect("proxy service");
        service.config.enabled = true;
        service.save_config(&creation.vault).expect("save config");
        service.handle = Some(
            ProxyHandle::start(
                RuntimeConfig::from_routes("127.0.0.1:0", Vec::new()),
                service.usage.clone(),
            )
            .expect("start proxy"),
        );

        service
            .apply_runtime_config(&creation.vault)
            .expect("stop proxy");

        assert!(!service.status().running);
        assert!(!service.status().enabled);
        let stored = service.config(&creation.vault).expect("load stored config");
        assert!(!stored.enabled);
    }

    #[test]
    fn config_rejects_multiple_enabled_routes() {
        let mut config = config_with_token("first-token");
        let mut second = config.routes[0].clone();
        second.id = Uuid::new_v4();
        second.token = "second-token".into();
        config.routes.push(second);
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn legacy_config_keeps_only_first_enabled_route() {
        let mut config = config_with_token("first-token");
        let mut second = config.routes[0].clone();
        second.id = Uuid::new_v4();
        second.token = "second-token".into();
        config.routes.push(second);

        assert!(normalize_enabled_routes(&mut config));
        assert!(config.routes[0].enabled);
        assert!(!config.routes[1].enabled);
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn config_rejects_unfinished_protocol_conversion() {
        let token = "matching-token";
        let mut config = config_with_token(token);
        config.routes[0].conversion_enabled = true;
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn config_rejects_cross_protocol_passthrough() {
        let token = "matching-token";
        let mut config = config_with_token(token);
        config.routes[0].upstream_protocol = aipass_proxy::Protocol::AnthropicMessages;
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn legacy_conversion_config_is_downgraded_to_same_protocol() {
        let token = "matching-token";
        let mut config = config_with_token(token);
        config.routes[0].upstream_protocol = aipass_proxy::Protocol::AnthropicMessages;
        config.routes[0].conversion_enabled = true;

        assert!(normalize_unavailable_conversion(&mut config));
        assert!(!config.routes[0].conversion_enabled);
        assert_eq!(
            config.routes[0].upstream_protocol,
            config.routes[0].inbound_protocol
        );
        assert!(validate_config(&config).is_ok());
    }
}
