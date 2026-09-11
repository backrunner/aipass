use crate::*;

pub(crate) fn handle_proxy_command(
    json: bool,
    vault: Option<PathBuf>,
    cli_password: Option<String>,
    command: ProxyCommand,
) -> Result<()> {
    match command {
        ProxyCommand::Status => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let status: ProxyStatus = agent.request(AgentRequest::ServerStatus)?;
            let message = if status.running {
                format!("Proxy running on {}", status.bind_addr)
            } else {
                "Proxy stopped".to_string()
            };
            output(json, serde_json::to_value(&status)?, &message)
        }
        ProxyCommand::Start => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let status: ProxyStatus = agent.request(AgentRequest::ServerStart)?;
            output(
                json,
                serde_json::to_value(&status)?,
                &format!("Proxy started on {}", status.bind_addr),
            )
        }
        ProxyCommand::Stop => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let status: ProxyStatus = agent.request(AgentRequest::ServerStop)?;
            output(json, serde_json::to_value(&status)?, "Proxy stopped")
        }
        ProxyCommand::ConfigGet => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let config: ProxyConfig = agent.request(AgentRequest::ServerConfigGet)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&config)?);
            } else {
                println!("Proxy Configuration:");
                println!("  Enabled: {}", config.enabled);
                println!("  Bind Address: {}", config.bind_addr);
                println!("  Routes: {}", config.routes.len());
                println!("  Pricing Rules: {}", config.pricing.len());
            }
            Ok(())
        }
        ProxyCommand::ConfigSet { file } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let config_json = fs::read_to_string(file)?;
            let config: ProxyConfig = serde_json::from_str(&config_json)?;
            let _: serde_json::Value = agent.request(AgentRequest::ServerConfigSet { config })?;
            output(
                json,
                serde_json::json!({ "ok": true }),
                "Proxy configuration updated",
            )
        }
        ProxyCommand::RouteList => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let config: ProxyConfig = agent.request(AgentRequest::ServerConfigGet)?;
            output(
                json,
                serde_json::to_value(&config.routes)?,
                &format!("{} routes", config.routes.len()),
            )
        }
        ProxyCommand::RouteCreate {
            name,
            provider_id,
            secret_id,
            inbound_protocol,
            upstream_protocol,
            conversion_enabled,
        } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let mut config: ProxyConfig = agent.request(AgentRequest::ServerConfigGet)?;
            let entry: aipass_vault::EntrySummary =
                agent.request(AgentRequest::ProviderGet { id: provider_id })?;
            let base_url = aipass_config_writers::endpoint_url(&entry.endpoints)
                .context("provider has no API endpoint")?;
            let auth_scheme = format!("{:?}", entry.auth_scheme);
            let route = ProxyRouteConfig {
                id: Uuid::new_v4(),
                name,
                token: format!("sk-aipass-{}", Uuid::new_v4().simple()),
                inbound_protocol: inbound_protocol.into(),
                upstream_protocol: upstream_protocol.into(),
                conversion_enabled,
                strategy: RouteStrategy::Fallback,
                targets: vec![ProxyTargetConfig {
                    id: Uuid::new_v4(),
                    provider_entry_id: provider_id,
                    secret_id,
                    label: entry.title.clone(),
                    base_url,
                    auth_scheme,
                    headers: Vec::new(),
                    group: None,
                    priority: 0,
                    weight: 1,
                    enabled: true,
                    protocol: None,
                    prefer_ws: false,
                }],
                retry: aipass_proxy::RetryPolicy::default(),
                enabled: true,
            };
            config.routes.push(route.clone());
            let _: serde_json::Value = agent.request(AgentRequest::ServerConfigSet { config })?;
            output(
                json,
                serde_json::json!({ "ok": true, "routeId": route.id, "token": route.token }),
                &format!("Route created: {} (token: {})", route.id, route.token),
            )
        }
        ProxyCommand::RouteSetEnabled { route_id, enabled } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let _: serde_json::Value =
                agent.request(AgentRequest::ServerRouteSetEnabled { route_id, enabled })?;
            output(
                json,
                serde_json::json!({ "ok": true, "routeId": route_id, "enabled": enabled }),
                if enabled {
                    "Route enabled"
                } else {
                    "Route disabled"
                },
            )
        }
        ProxyCommand::RouteSelect { route_id } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let _: serde_json::Value =
                agent.request(AgentRequest::ServerRouteSelect { route_id })?;
            output(
                json,
                serde_json::json!({ "ok": true, "routeId": route_id }),
                "Route selected",
            )
        }
        ProxyCommand::TokenRotate { route_id } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let response: ServerTokenResponse =
                agent.request(AgentRequest::ServerTokenRotate { route_id })?;
            let token_str = response.token.expose();
            output(
                json,
                serde_json::json!({ "ok": true, "routeId": route_id, "token": token_str }),
                &format!("Token rotated: {}", token_str),
            )
        }
        ProxyCommand::Logs { limit } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let logs: Vec<aipass_agent_protocol::ProxyLogEntry> =
                agent.request(AgentRequest::ServerLogs)?;
            let display_logs = logs.into_iter().take(limit).collect::<Vec<_>>();
            output(
                json,
                serde_json::to_value(&display_logs)?,
                &format!("{} log entries", display_logs.len()),
            )
        }
        ProxyCommand::Usage {
            days,
            timezone_offset_minutes,
        } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let summary: ServerUsageSummary = agent.request(AgentRequest::ServerUsageSummary {
                days,
                timezone_offset_minutes,
                granularity: aipass_agent_protocol::UsageGranularity::Day,
            })?;
            output(
                json,
                serde_json::to_value(&summary)?,
                &format!(
                    "Requests: {}, Tokens: {} in / {} out",
                    summary.request_count, summary.input_tokens, summary.output_tokens
                ),
            )
        }
        ProxyCommand::UsageClear => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let _: serde_json::Value = agent.request(AgentRequest::ServerUsageClear)?;
            output(
                json,
                serde_json::json!({ "ok": true }),
                "Usage data cleared",
            )
        }
        ProxyCommand::RouteUpdateTarget {
            route_id,
            target_index,
            weight,
        } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let mut config: ProxyConfig = agent.request(AgentRequest::ServerConfigGet)?;
            let route = config
                .routes
                .iter_mut()
                .find(|r| r.id == route_id)
                .context("Route not found")?;
            let target = route
                .targets
                .get_mut(target_index)
                .context("Target index out of bounds")?;

            if let Some(w) = weight {
                target.weight = w;
            }

            let _: serde_json::Value = agent.request(AgentRequest::ServerConfigSet { config })?;
            output(
                json,
                serde_json::json!({ "ok": true, "routeId": route_id, "targetIndex": target_index }),
                "Route target updated",
            )
        }
        ProxyCommand::RouteApply { tool, route_id } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let tool_id = tool.into();
            let request = aipass_agent_protocol::ToolConfigProxyRequest {
                tool: tool_id,
                route_id,
            };
            let response: ToolConfigApplyResponse =
                agent.request(AgentRequest::ToolConfigProxyApply { request })?;
            output(
                json,
                serde_json::to_value(&response)?,
                "Proxy route applied to agent tool",
            )
        }
        ProxyCommand::GroupList => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let config: ProxyConfig = agent.request(AgentRequest::ServerConfigGet)?;
            let mut groups = std::collections::HashSet::new();
            for route in &config.routes {
                for target in &route.targets {
                    if let Some(ref group) = target.group {
                        groups.insert(group.clone());
                    }
                }
            }
            let groups_vec: Vec<String> = groups.into_iter().collect();
            output(
                json,
                serde_json::to_value(&groups_vec)?,
                &format!("{} groups found", groups_vec.len()),
            )
        }
        ProxyCommand::GroupEnable { route_id, group } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let mut config: ProxyConfig = agent.request(AgentRequest::ServerConfigGet)?;
            let route = config
                .routes
                .iter_mut()
                .find(|r| r.id == route_id)
                .context("Route not found")?;
            let mut count = 0;
            for target in &mut route.targets {
                if target.group.as_ref() == Some(&group) {
                    target.enabled = true;
                    count += 1;
                }
            }
            let _: serde_json::Value = agent.request(AgentRequest::ServerConfigSet { config })?;
            output(
                json,
                serde_json::json!({ "ok": true, "routeId": route_id, "group": group, "count": count }),
                &format!("Enabled {} targets in group '{}'", count, group),
            )
        }
        ProxyCommand::GroupDisable { route_id, group } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let mut config: ProxyConfig = agent.request(AgentRequest::ServerConfigGet)?;
            let route = config
                .routes
                .iter_mut()
                .find(|r| r.id == route_id)
                .context("Route not found")?;
            let mut count = 0;
            for target in &mut route.targets {
                if target.group.as_ref() == Some(&group) {
                    target.enabled = false;
                    count += 1;
                }
            }
            let _: serde_json::Value = agent.request(AgentRequest::ServerConfigSet { config })?;
            output(
                json,
                serde_json::json!({ "ok": true, "routeId": route_id, "group": group, "count": count }),
                &format!("Disabled {} targets in group '{}'", count, group),
            )
        }
        ProxyCommand::TargetList { route_id } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let config: ProxyConfig = agent.request(AgentRequest::ServerConfigGet)?;
            let route = config
                .routes
                .iter()
                .find(|r| r.id == route_id)
                .context("Route not found")?;
            output(
                json,
                serde_json::to_value(&route.targets)?,
                &format!("{} targets in route", route.targets.len()),
            )
        }
        ProxyCommand::TargetEnable {
            route_id,
            target_id,
        } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let mut config: ProxyConfig = agent.request(AgentRequest::ServerConfigGet)?;
            let route = config
                .routes
                .iter_mut()
                .find(|r| r.id == route_id)
                .context("Route not found")?;
            let target = route
                .targets
                .iter_mut()
                .find(|t| t.id == target_id)
                .context("Target not found")?;
            target.enabled = true;
            let _: serde_json::Value = agent.request(AgentRequest::ServerConfigSet { config })?;
            output(
                json,
                serde_json::json!({ "ok": true, "routeId": route_id, "targetId": target_id }),
                "Target enabled",
            )
        }
        ProxyCommand::TargetDisable {
            route_id,
            target_id,
        } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let mut config: ProxyConfig = agent.request(AgentRequest::ServerConfigGet)?;
            let route = config
                .routes
                .iter_mut()
                .find(|r| r.id == route_id)
                .context("Route not found")?;
            let target = route
                .targets
                .iter_mut()
                .find(|t| t.id == target_id)
                .context("Target not found")?;
            target.enabled = false;
            let _: serde_json::Value = agent.request(AgentRequest::ServerConfigSet { config })?;
            output(
                json,
                serde_json::json!({ "ok": true, "routeId": route_id, "targetId": target_id }),
                "Target disabled",
            )
        }
        ProxyCommand::TargetSetPriority {
            route_id,
            target_id,
            priority,
        } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let mut config: ProxyConfig = agent.request(AgentRequest::ServerConfigGet)?;
            let route = config
                .routes
                .iter_mut()
                .find(|r| r.id == route_id)
                .context("Route not found")?;
            let target = route
                .targets
                .iter_mut()
                .find(|t| t.id == target_id)
                .context("Target not found")?;
            target.priority = priority;
            let _: serde_json::Value = agent.request(AgentRequest::ServerConfigSet { config })?;
            output(
                json,
                serde_json::json!({ "ok": true, "routeId": route_id, "targetId": target_id, "priority": priority }),
                &format!("Target priority set to {}", priority),
            )
        }
        ProxyCommand::TargetSetWeight {
            route_id,
            target_id,
            weight,
        } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let mut config: ProxyConfig = agent.request(AgentRequest::ServerConfigGet)?;
            let route = config
                .routes
                .iter_mut()
                .find(|r| r.id == route_id)
                .context("Route not found")?;
            let target = route
                .targets
                .iter_mut()
                .find(|t| t.id == target_id)
                .context("Target not found")?;
            target.weight = weight;
            let _: serde_json::Value = agent.request(AgentRequest::ServerConfigSet { config })?;
            output(
                json,
                serde_json::json!({ "ok": true, "routeId": route_id, "targetId": target_id, "weight": weight }),
                &format!("Target weight set to {}", weight),
            )
        }
        ProxyCommand::ProviderUpdate {
            provider_id,
            prefer_websocket,
            max_concurrent_requests,
        } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let entry: aipass_vault::EntrySummary =
                agent.request(AgentRequest::ProviderGet { id: provider_id })?;

            // Build update input from existing entry
            let input = aipass_vault::ProviderEntryUpdateInput {
                title: entry.title,
                provider_kind: entry.provider_kind,
                provider_id: entry.provider_id,
                credential_kind: Some(entry.credential_kind),
                account_identity: entry.account_identity,
                domains: entry.domains,
                favicon_url: entry.favicon_url,
                endpoints: entry.endpoints,
                interface_type: entry.interface_type,
                max_concurrent_requests: max_concurrent_requests.or(entry.max_concurrent_requests),
                supports_websockets: prefer_websocket.or(entry.supports_websockets),
                auth_scheme: entry.auth_scheme,
                api_key: None, // Don't update secret
                secret_label: None,
                default_model: entry.default_model,
                model_aliases: entry.model_aliases,
                headers: None, // Headers are stored separately, not updated here
                quota: entry.quota,
                subscription: entry.subscription,
                gateway: entry.gateway,
                tags: entry.tags,
                notes: entry.notes,
                secret_metadata: Default::default(),
            };

            // Update the provider entry
            let _: serde_json::Value = agent.request(AgentRequest::ProviderUpdate {
                id: provider_id,
                input,
            })?;

            output(
                json,
                serde_json::json!({ "ok": true, "providerId": provider_id, "supportsWebsockets": prefer_websocket }),
                "Provider updated",
            )
        }
        ProxyCommand::RouteDelete { route_id } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let mut config: ProxyConfig = agent.request(AgentRequest::ServerConfigGet)?;
            config.routes.retain(|r| r.id != route_id);
            let _: serde_json::Value = agent.request(AgentRequest::ServerConfigSet { config })?;
            output(
                json,
                serde_json::json!({ "ok": true, "routeId": route_id }),
                "Route deleted",
            )
        }
        ProxyCommand::GroupSwitch { route_id, group } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let mut config: ProxyConfig = agent.request(AgentRequest::ServerConfigGet)?;
            let route = config
                .routes
                .iter_mut()
                .find(|r| r.id == route_id)
                .context("Route not found")?;

            // Disable all targets first
            for target in route.targets.iter_mut() {
                target.enabled = false;
            }

            // Enable targets in the specified group
            let mut count = 0;
            for target in route.targets.iter_mut() {
                if target.group.as_ref() == Some(&group) {
                    target.enabled = true;
                    count += 1;
                }
            }

            let _: serde_json::Value = agent.request(AgentRequest::ServerConfigSet { config })?;
            output(
                json,
                serde_json::json!({ "ok": true, "routeId": route_id, "group": group, "count": count }),
                &format!("Switched to group '{}' ({} targets enabled)", group, count),
            )
        }
        ProxyCommand::TargetAdd {
            route_id,
            provider_id,
            secret_id,
            group,
            priority,
            weight,
        } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let mut config: ProxyConfig = agent.request(AgentRequest::ServerConfigGet)?;
            let entry: aipass_vault::EntrySummary =
                agent.request(AgentRequest::ProviderGet { id: provider_id })?;
            let base_url = aipass_config_writers::endpoint_url(&entry.endpoints)
                .context("provider has no API endpoint")?;
            let auth_scheme = format!("{:?}", entry.auth_scheme);

            let route = config
                .routes
                .iter_mut()
                .find(|r| r.id == route_id)
                .context("Route not found")?;

            let target = ProxyTargetConfig {
                id: Uuid::new_v4(),
                provider_entry_id: provider_id,
                secret_id,
                label: entry.title.clone(),
                base_url,
                auth_scheme,
                headers: Vec::new(),
                group,
                priority,
                weight,
                enabled: true,
                protocol: None,
                prefer_ws: entry.supports_websockets.unwrap_or(false),
            };

            route.targets.push(target.clone());

            let _: serde_json::Value = agent.request(AgentRequest::ServerConfigSet { config })?;
            output(
                json,
                serde_json::json!({ "ok": true, "routeId": route_id, "targetId": target.id }),
                &format!("Target added to route (ID: {})", target.id),
            )
        }
        ProxyCommand::TargetRemove {
            route_id,
            target_id,
        } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let mut config: ProxyConfig = agent.request(AgentRequest::ServerConfigGet)?;
            let route = config
                .routes
                .iter_mut()
                .find(|r| r.id == route_id)
                .context("Route not found")?;

            route.targets.retain(|t| t.id != target_id);

            let _: serde_json::Value = agent.request(AgentRequest::ServerConfigSet { config })?;
            output(
                json,
                serde_json::json!({ "ok": true, "routeId": route_id, "targetId": target_id }),
                "Target removed from route",
            )
        }
    }
}
