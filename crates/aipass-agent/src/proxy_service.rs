use crate::logging::{write_component_log, AGENT_LOG};
use crate::session::{map_vault_error, ServiceError, ServiceResult};
use aipass_agent_protocol::{
    CredentialAssignment, ModelPriceRule, PricingApplyScope, PricingConfig, PricingGroup,
    ServerTokenResponse, ServerUsageSummary,
};
use aipass_crypto::Ciphertext;
use aipass_proxy::{
    fingerprint_token, ProxyConfig, ProxyHandle, ProxyStatus, ResolvedRoute, ResolvedTarget,
    RuntimeConfig, UsageRow, UsageStore, UsageTimeseriesPoint,
};
use aipass_storage::atomic_write_bytes;
use aipass_vault::Vault;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

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
}

impl ProxyService {
    pub fn new(vault_dir: &Path) -> anyhow::Result<Self> {
        let usage = Arc::new(UsageStore::open(vault_dir.join("proxy-usage.sqlite"))?);
        Ok(Self {
            vault_dir: vault_dir.to_path_buf(),
            config: ProxyConfig::default(),
            handle: None,
            usage,
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
            return Ok(self.config.clone());
        }
        let persisted: PersistedProxyConfig =
            serde_json::from_slice(&std::fs::read(path).map_err(ServiceError::internal)?)
                .map_err(ServiceError::internal)?;
        let bytes = vault
            .decrypt_local_state(CONFIG_PURPOSE, &persisted.payload)
            .map_err(map_vault_error)?;
        self.config = serde_json::from_slice(&bytes).map_err(ServiceError::internal)?;
        if normalize_unavailable_conversion(&mut self.config) {
            self.save_config(vault)?;
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
        self.load_config(vault).map(redact_route_tokens)
    }

    pub fn set_config(
        &mut self,
        vault: &Vault,
        mut config: ProxyConfig,
    ) -> ServiceResult<ProxyConfig> {
        self.load_config(vault)?;
        for route in &mut config.routes {
            if route.token.is_empty() {
                if let Some(current) = self.config.routes.iter().find(|current| {
                    current.id == route.id && current.token_fingerprint == route.token_fingerprint
                }) {
                    route.token.clone_from(&current.token);
                }
            }
        }
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
        Ok(redact_route_tokens(self.config.clone()))
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
            .any(|route| route.enabled && route.token_fingerprint.is_empty())
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

    pub fn stop_and_save(&mut self, vault: &Vault) -> ServiceResult<ProxyStatus> {
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
        let _ = self.stop();
        for route in &mut self.config.routes {
            route.token.clear();
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
        let token = format!(
            "aipass_{}_{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        let fingerprint = fingerprint_token(&token);
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
        let previous_fingerprint = std::mem::replace(
            &mut self.config.routes[route_index].token_fingerprint,
            fingerprint.clone(),
        );
        let was_running = self.handle.is_some();
        let result = self.save_config(vault).and_then(|()| {
            was_running
                .then(|| self.restart(vault))
                .transpose()
                .map(|_| ())
        });
        if let Err(err) = result {
            self.config.routes[route_index].token = previous_token;
            self.config.routes[route_index].token_fingerprint = previous_fingerprint;
            let _ = self.save_config(vault);
            if was_running {
                let _ = self.restart(vault);
            }
            return Err(err);
        }
        Ok(ServerTokenResponse {
            route_id,
            fingerprint,
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
            providers: summary.providers,
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
                let api_key = vault
                    .reveal_secret_field(target.provider_entry_id, &target.secret_id)
                    .map_err(map_vault_error)?;
                targets.push(ResolvedTarget {
                    config: target.clone(),
                    api_key,
                });
            }
            if targets.is_empty() {
                return Err(ServiceError::new(
                    aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                    format!("route {} has no enabled targets", route.name),
                ));
            }
            routes.push(ResolvedRoute {
                config: route.clone(),
                local_token: String::new(),
                targets,
            });
        }
        let mut runtime = RuntimeConfig::from_routes(self.config.bind_addr.clone(), routes);
        runtime.pricing = self.config.pricing.clone();
        Ok(runtime)
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

fn redact_route_tokens(mut config: ProxyConfig) -> ProxyConfig {
    for route in &mut config.routes {
        route.token.clear();
    }
    config
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

fn validate_config(config: &ProxyConfig) -> ServiceResult<()> {
    if config.bind_addr.parse::<std::net::SocketAddr>().is_err() {
        return Err(ServiceError::new(
            aipass_agent_protocol::AgentErrorCode::ValidationFailed,
            "proxy bind address must be host:port",
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
    if config.routes.iter().any(|route| {
        route.token.is_empty() != route.token_fingerprint.is_empty()
            || (!route.token.is_empty()
                && fingerprint_token(&route.token) != route.token_fingerprint)
    }) {
        return Err(ServiceError::new(
            aipass_agent_protocol::AgentErrorCode::ValidationFailed,
            "proxy route token does not match its fingerprint",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aipass_crypto::SecretString;
    use aipass_proxy::{ProxyRouteConfig, RetryPolicy, RouteStrategy};

    fn config_with_token(token: &str, fingerprint: &str) -> ProxyConfig {
        ProxyConfig {
            routes: vec![ProxyRouteConfig {
                id: Uuid::new_v4(),
                name: "test".into(),
                token: token.into(),
                token_fingerprint: fingerprint.into(),
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

    #[test]
    fn config_rejects_mismatched_token_fingerprint() {
        let config = config_with_token("new-token", &fingerprint_token("old-token"));
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn config_accepts_matching_token_fingerprint() {
        let token = "matching-token";
        let config = config_with_token(token, &fingerprint_token(token));
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn client_config_never_contains_route_tokens() {
        let token = "matching-token";
        let redacted = redact_route_tokens(config_with_token(token, &fingerprint_token(token)));
        assert!(redacted.routes[0].token.is_empty());
        assert_eq!(
            redacted.routes[0].token_fingerprint,
            fingerprint_token(token)
        );
    }

    #[test]
    fn redacted_client_config_preserves_stored_token_on_save() {
        let temp = tempfile::tempdir().expect("tempdir");
        let creation = Vault::create(
            temp.path(),
            &SecretString::new("correct horse battery staple"),
        )
        .expect("create vault");
        let mut service = ProxyService::new(temp.path()).expect("proxy service");
        let token = "matching-token";

        let mut client_config = service
            .set_config(
                &creation.vault,
                config_with_token(token, &fingerprint_token(token)),
            )
            .expect("save config");
        assert!(client_config.routes[0].token.is_empty());

        client_config.bind_addr = "127.0.0.1:9876".into();
        service
            .set_config(&creation.vault, client_config)
            .expect("save redacted config");
        let stored = service.config(&creation.vault).expect("load stored config");
        assert_eq!(stored.routes[0].token, token);
        assert_eq!(stored.routes[0].token_fingerprint, fingerprint_token(token));
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
    fn config_rejects_fingerprint_without_token() {
        let config = config_with_token("", &fingerprint_token("missing-token"));
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn config_rejects_unfinished_protocol_conversion() {
        let token = "matching-token";
        let mut config = config_with_token(token, &fingerprint_token(token));
        config.routes[0].conversion_enabled = true;
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn config_rejects_cross_protocol_passthrough() {
        let token = "matching-token";
        let mut config = config_with_token(token, &fingerprint_token(token));
        config.routes[0].upstream_protocol = aipass_proxy::Protocol::AnthropicMessages;
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn legacy_conversion_config_is_downgraded_to_same_protocol() {
        let token = "matching-token";
        let mut config = config_with_token(token, &fingerprint_token(token));
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
