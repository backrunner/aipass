//! Runtime route construction from pure vault reads.
use super::*;

impl ProxyService {
    pub(super) fn remove_pricing_assignments(
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

    pub(super) fn runtime_config(&self, vault: &Vault) -> ServiceResult<RuntimeConfig> {
        self.runtime_config_inner(vault, false)
    }

    pub(super) fn runtime_config_inner(
        &self,
        vault: &Vault,
        skip_unavailable: bool,
    ) -> ServiceResult<RuntimeConfig> {
        let mut routes = Vec::new();
        for route in self.config.routes.iter().filter(|route| route.enabled) {
            let mut targets = Vec::new();
            for target in route.targets.iter().filter(|target| target.enabled) {
                let resolved = (|| -> ServiceResult<ResolvedTarget> {
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
                    if credential.effective_auth(&entry.interface_type, &entry.auth_scheme)
                        == AuthScheme::GoogleApiKey
                        && *interface != InterfaceType::Gemini
                    {
                        return Err(ServiceError::new(
                            aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                            "Google API credentials require the Gemini interface",
                        ));
                    }
                    if !matches!(
                        interface,
                        InterfaceType::AnthropicMessages
                            | InterfaceType::OpenAiCompatible
                            | InterfaceType::AzureOpenAi
                            | InterfaceType::Gemini
                    ) {
                        return Err(ServiceError::new(
                            aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                            format!(
                                "proxy target {} uses an unsupported {:?} interface",
                                target.label, interface
                            ),
                        ));
                    }
                    // The route owns the wire format. A self-hosted endpoint may
                    // support both OpenAI and Anthropic protocols, so inferring a
                    // target's "native" protocol from provider metadata would
                    // reject valid configurations and silently enable conversion
                    // for the wrong format. An explicit target protocol remains
                    // available for legacy/advanced configs; otherwise use the
                    // route's configured upstream protocol verbatim.
                    let upstream_kind = if crate::community::is_account(vault, entry.id) {
                        aipass_proxy::UpstreamKind::CommunitySubscription
                    } else if upstream_kind(&entry) == aipass_proxy::UpstreamKind::Copilot
                        && vault
                            .provider_runtime_extension(entry.id, "copilot_auth_v1")
                            .map_err(map_vault_error)?
                            .is_some_and(|v| {
                                serde_json::from_str::<serde_json::Value>(v.expose())
                                    .is_ok_and(|v| v["client"] == "cli")
                            })
                    {
                        aipass_proxy::UpstreamKind::CopilotCli
                    } else if *interface == InterfaceType::Gemini {
                        aipass_proxy::UpstreamKind::GeminiNative
                    } else {
                        upstream_kind(&entry)
                    };
                    let target_protocol =
                        if upstream_kind == aipass_proxy::UpstreamKind::CodexSubscription {
                            aipass_proxy::Protocol::OpenAiResponses
                        } else if upstream_kind == aipass_proxy::UpstreamKind::ClaudeSubscription {
                            aipass_proxy::Protocol::AnthropicMessages
                        } else if matches!(
                            upstream_kind,
                            aipass_proxy::UpstreamKind::GeminiNative
                                | aipass_proxy::UpstreamKind::CommunitySubscription
                        ) {
                            aipass_proxy::Protocol::OpenAiChatCompletions
                        } else {
                            if target.model.is_some() {
                                key_upstream_protocol(interface, &entry)
                                    .unwrap_or(route.upstream_protocol)
                            } else {
                                target.protocol.unwrap_or(route.upstream_protocol)
                            }
                        };
                    let mut credentials = vault
                        .runtime_provider_credentials(target.provider_entry_id, &target.secret_id)
                        .map_err(map_vault_error)?;
                    let api_key = managed_oauth_token(vault, &entry, &target.secret_id)?
                        .unwrap_or_else(|| credentials.secret.expose().to_owned());
                    let provider_headers = std::mem::take(&mut *credentials.headers);
                    let mut target_config = target.clone();
                    target_config.protocol = Some(target_protocol);
                    if let Some(url) = credential.endpoint.as_deref().or_else(|| {
                        entry
                            .endpoints
                            .iter()
                            .find(|endpoint| endpoint.kind == EndpointKind::Api)
                            .and_then(|endpoint| endpoint.url.as_deref())
                    }) {
                        target_config.base_url = url.to_owned();
                    }
                    if let Some(auth) = proxy_auth_scheme(
                        &credential.effective_auth(&entry.interface_type, &entry.auth_scheme),
                    ) {
                        target_config.auth_scheme = auth.to_owned();
                    }

                    if let Some(pinned) = pinned_official_oauth_endpoint(
                        &entry.provider_kind,
                        &entry.credential_kind,
                        entry.provider_id.as_deref(),
                    ) {
                        target_config.base_url = pinned.to_string();
                    } else if entry.provider_kind == ProviderKind::Official
                        && entry.credential_kind == CredentialKind::OAuth
                    {
                        write_component_log(
                        AGENT_LOG,
                        "WARN",
                        &format!(
                            "official OAuth entry {} has no pinned upstream; keeping entry endpoint",
                            entry.id
                        ),
                    );
                    }
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
                    let preferences = crate::provider_runtime::load(vault, entry.id)?;
                    let provider_proxy =
                        crate::provider_runtime::outbound(&preferences).map_err(|e| {
                            ServiceError::new(
                                aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                                e,
                            )
                        })?;
                    Ok(ResolvedTarget {
                        upstream_proxy: if matches!(
                            upstream_kind,
                            aipass_proxy::UpstreamKind::CommunitySubscription
                                | aipass_proxy::UpstreamKind::ClaudeSubscription
                        ) {
                            Some(
                                provider_proxy
                                    .clone()
                                    .unwrap_or_else(|| self.config.upstream_proxy.clone()),
                            )
                        } else {
                            provider_proxy.clone()
                        },
                        upstream_kind,
                        quota: if upstream_kind == aipass_proxy::UpstreamKind::CommunitySubscription
                        {
                            crate::community::quota(vault, entry.id)
                        } else {
                            account_quota(&entry)
                        },
                        profile: provider_profile(&entry),
                        model_override: target_config.model.clone(),
                        max_concurrent_requests: entry.max_concurrent_requests,
                        supports_websockets: entry.supports_websockets.unwrap_or(true)
                            && provider_proxy.is_none()
                            && !matches!(
                                upstream_kind,
                                aipass_proxy::UpstreamKind::CommunitySubscription
                                    | aipass_proxy::UpstreamKind::ClaudeSubscription
                            ),
                        config: target_config,
                        api_key,
                    })
                })();
                match resolved {
                    Ok(target) => targets.push(target),
                    Err(error) if skip_unavailable => {
                        write_component_log(
                            AGENT_LOG,
                            "WARN",
                            &format!(
                                "proxy target unavailable during refresh target_id={} code={:?}",
                                target.id, error.code
                            ),
                        );
                    }
                    Err(error) => return Err(error),
                }
            }
            if targets.is_empty() {
                if skip_unavailable {
                    continue;
                }
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
        runtime.upstream_proxy = self.config.upstream_proxy.clone();
        Ok(runtime)
    }
}
