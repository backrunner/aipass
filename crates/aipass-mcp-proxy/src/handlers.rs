use crate::params::*;
use crate::ProxyMcpServer;
use aipass_agent_protocol::{
    AgentRequest, EntrySummary, ProviderEntryInput, ProviderEntryUpdateInput, ProxyStatus,
    ServerTokenResponse, ServerUsageSummary, UsageGranularity,
};
use aipass_proxy::{ProxyConfig, ProxyRouteConfig, ProxyTargetConfig, RouteStrategy};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use uuid::Uuid;

impl ProxyMcpServer {
    pub(crate) async fn proxy_status(&self) -> Result<Value> {
        let agent = self.agent_client()?;
        let status: ProxyStatus = agent.request(&AgentRequest::ServerStatus)?;
        Ok(serde_json::to_value(&status)?)
    }

    pub(crate) async fn proxy_start(&self) -> Result<Value> {
        let agent = self.agent_client()?;
        let status: ProxyStatus = agent.request(&AgentRequest::ServerStart)?;
        Ok(json!({ "ok": true, "status": status }))
    }

    pub(crate) async fn proxy_stop(&self) -> Result<Value> {
        let agent = self.agent_client()?;
        let _: Value = agent.request(&AgentRequest::ServerStop)?;
        Ok(json!({ "ok": true }))
    }

    pub(crate) async fn proxy_restart(&self) -> Result<Value> {
        let agent = self.agent_client()?;
        let _: Value = agent.request(&AgentRequest::ServerStop)?;
        let status: ProxyStatus = agent.request(&AgentRequest::ServerStart)?;
        Ok(json!({ "ok": true, "status": status }))
    }

    pub(crate) async fn proxy_list_routes(&self) -> Result<Value> {
        let agent = self.agent_client()?;
        let config: ProxyConfig = agent.request(&AgentRequest::ServerConfigGet)?;
        Ok(serde_json::to_value(&config.routes)?)
    }

    pub(crate) async fn proxy_create_route(&self, params: CreateRouteParams) -> Result<Value> {
        let agent = self.agent_client()?;

        let route = ProxyRouteConfig {
            id: Uuid::new_v4(),
            name: params.name,
            token: format!("sk-aipass-{}", Uuid::new_v4().simple()),
            inbound_protocol: params.inbound_protocol,
            upstream_protocol: params.upstream_protocol,
            conversion_enabled: params.conversion_enabled.unwrap_or(false),
            strategy: params.strategy.unwrap_or(RouteStrategy::Fallback),
            targets: params.targets,
            retry: aipass_proxy::RetryPolicy::default(),
            enabled: params.enabled.unwrap_or(true),
        };

        let mut config: ProxyConfig = agent.request(&AgentRequest::ServerConfigGet)?;
        config.routes.push(route.clone());
        let _: Value = agent.request(&AgentRequest::ServerConfigSet { config })?;

        Ok(json!({ "ok": true, "route": route }))
    }

    pub(crate) async fn proxy_update_route(&self, params: UpdateRouteParams) -> Result<Value> {
        let agent = self.agent_client()?;
        let mut config: ProxyConfig = agent.request(&AgentRequest::ServerConfigGet)?;

        let route_id = Uuid::parse_str(&params.route_id)?;
        let route = config
            .routes
            .iter_mut()
            .find(|r| r.id == route_id)
            .context("Route not found")?;

        if let Some(name) = params.name {
            route.name = name;
        }
        if let Some(enabled) = params.enabled {
            route.enabled = enabled;
        }
        if let Some(inbound_protocol) = params.inbound_protocol {
            route.inbound_protocol = inbound_protocol;
        }
        if let Some(upstream_protocol) = params.upstream_protocol {
            route.upstream_protocol = upstream_protocol;
        }
        if let Some(conversion_enabled) = params.conversion_enabled {
            route.conversion_enabled = conversion_enabled;
        }
        if let Some(targets) = params.targets {
            route.targets = targets;
        }
        if let Some(strategy) = params.strategy {
            route.strategy = strategy;
        }

        let updated_route = route.clone();
        let _: Value = agent.request(&AgentRequest::ServerConfigSet { config })?;
        Ok(json!({ "ok": true, "route": updated_route }))
    }

    pub(crate) async fn proxy_delete_route(&self, params: DeleteRouteParams) -> Result<Value> {
        let agent = self.agent_client()?;
        let mut config: ProxyConfig = agent.request(&AgentRequest::ServerConfigGet)?;

        let route_id = Uuid::parse_str(&params.route_id)?;
        config.routes.retain(|r| r.id != route_id);
        let _: Value = agent.request(&AgentRequest::ServerConfigSet { config })?;

        Ok(json!({ "ok": true }))
    }

    pub(crate) async fn proxy_enable_route(&self, params: RouteIdParams) -> Result<Value> {
        let agent = self.agent_client()?;
        let mut config: ProxyConfig = agent.request(&AgentRequest::ServerConfigGet)?;

        let route_id = Uuid::parse_str(&params.route_id)?;
        let route = config
            .routes
            .iter_mut()
            .find(|r| r.id == route_id)
            .context("Route not found")?;

        route.enabled = true;
        let _: Value = agent.request(&AgentRequest::ServerConfigSet { config })?;

        Ok(json!({ "ok": true }))
    }

    pub(crate) async fn proxy_disable_route(&self, params: RouteIdParams) -> Result<Value> {
        let agent = self.agent_client()?;
        let mut config: ProxyConfig = agent.request(&AgentRequest::ServerConfigGet)?;

        let route_id = Uuid::parse_str(&params.route_id)?;
        let route = config
            .routes
            .iter_mut()
            .find(|r| r.id == route_id)
            .context("Route not found")?;

        route.enabled = false;
        let _: Value = agent.request(&AgentRequest::ServerConfigSet { config })?;

        Ok(json!({ "ok": true }))
    }

    pub(crate) async fn proxy_token_rotate(&self, params: RouteIdParams) -> Result<Value> {
        let agent = self.agent_client()?;
        let route_id = Uuid::parse_str(&params.route_id)?;
        let response: ServerTokenResponse =
            agent.request(&AgentRequest::ServerTokenRotate { route_id })?;
        Ok(json!({
            "ok": true,
            "token": response.token.expose()
        }))
    }

    pub(crate) async fn proxy_usage(&self, params: UsageParams) -> Result<Value> {
        let agent = self.agent_client()?;
        let summary: ServerUsageSummary = agent.request(&AgentRequest::ServerUsageSummary {
            days: params.days,
            timezone_offset_minutes: params.timezone_offset_minutes.unwrap_or(0),
            granularity: UsageGranularity::Day,
        })?;
        Ok(serde_json::to_value(&summary)?)
    }

    pub(crate) async fn proxy_logs(&self, params: LogsParams) -> Result<Value> {
        let agent = self.agent_client()?;
        let logs: Vec<aipass_agent_protocol::ProxyLogEntry> =
            agent.request(&AgentRequest::ServerLogs)?;
        let display_logs = logs
            .into_iter()
            .take(params.limit.unwrap_or(100))
            .collect::<Vec<_>>();
        Ok(serde_json::to_value(&display_logs)?)
    }

    pub(crate) async fn proxy_list_providers(&self) -> Result<Value> {
        let agent = self.agent_client()?;
        let entries: Vec<EntrySummary> =
            agent.request(&AgentRequest::EntriesList { archived: false })?;
        // Filter only provider entries (those with a provider_id)
        let providers: Vec<_> = entries
            .into_iter()
            .filter(|e| e.provider_id.is_some())
            .collect();
        Ok(serde_json::to_value(&providers)?)
    }

    pub(crate) async fn proxy_get_provider(&self, params: ProviderIdParams) -> Result<Value> {
        let agent = self.agent_client()?;
        let id = Uuid::parse_str(&params.provider_id)?;
        let provider: EntrySummary = agent.request(&AgentRequest::ProviderGet { id })?;
        Ok(serde_json::to_value(&provider)?)
    }

    pub(crate) async fn proxy_create_provider(
        &self,
        params: CreateProviderParams,
    ) -> Result<Value> {
        let agent = self.agent_client()?;
        let input: ProviderEntryInput = serde_json::from_value(params.provider_data)?;
        let id: Uuid = agent.request(&AgentRequest::ProviderAdd { input })?;
        Ok(json!({ "ok": true, "entry_id": id.to_string() }))
    }

    pub(crate) async fn proxy_update_provider(
        &self,
        params: UpdateProviderParams,
    ) -> Result<Value> {
        let agent = self.agent_client()?;
        let id = Uuid::parse_str(&params.entry_id)?;
        let input: ProviderEntryUpdateInput = serde_json::from_value(params.provider_data)?;
        let _: Value = agent.request(&AgentRequest::ProviderUpdate { id, input })?;
        Ok(json!({ "ok": true }))
    }

    pub(crate) async fn proxy_delete_provider(&self, params: ProviderIdParams) -> Result<Value> {
        let agent = self.agent_client()?;
        let id = Uuid::parse_str(&params.provider_id)?;
        let _: Value = agent.request(&AgentRequest::ProviderArchive { id })?;
        Ok(json!({ "ok": true }))
    }

    pub(crate) async fn proxy_reorder_targets(
        &self,
        params: ReorderTargetsParams,
    ) -> Result<Value> {
        let agent = self.agent_client()?;
        let mut config: ProxyConfig = agent.request(&AgentRequest::ServerConfigGet)?;

        let route_id = Uuid::parse_str(&params.route_id)?;
        let route = config
            .routes
            .iter_mut()
            .find(|r| r.id == route_id)
            .context("Route not found")?;

        // Reorder targets based on the provided order; targets omitted from
        // the list are dropped.
        let mut new_targets = Vec::new();
        for target_order in params.targets {
            let target_id = Uuid::parse_str(&target_order.target_id)
                .context("Invalid target_id in reorder list")?;
            if let Some(target) = route.targets.iter().find(|t| t.id == target_id) {
                let mut new_target = target.clone();
                if let Some(weight) = target_order.weight {
                    new_target.weight = weight;
                }
                new_targets.push(new_target);
            }
        }

        route.targets = new_targets;
        let updated_route = route.clone();
        let _: Value = agent.request(&AgentRequest::ServerConfigSet { config })?;
        Ok(json!({ "ok": true, "route": updated_route }))
    }

    pub(crate) async fn proxy_list_groups(&self, params: RouteIdParams) -> Result<Value> {
        let agent = self.agent_client()?;
        let config: ProxyConfig = agent.request(&AgentRequest::ServerConfigGet)?;

        let route_id = Uuid::parse_str(&params.route_id)?;
        let route = config
            .routes
            .iter()
            .find(|r| r.id == route_id)
            .context("Route not found")?;

        let mut groups = std::collections::HashSet::new();
        for target in &route.targets {
            if let Some(ref group) = target.group {
                groups.insert(group.clone());
            }
        }

        let groups_vec: Vec<String> = groups.into_iter().collect();
        Ok(serde_json::to_value(&groups_vec)?)
    }

    pub(crate) async fn proxy_enable_group(&self, params: GroupParams) -> Result<Value> {
        let agent = self.agent_client()?;
        let mut config: ProxyConfig = agent.request(&AgentRequest::ServerConfigGet)?;

        let route_id = Uuid::parse_str(&params.route_id)?;
        let route = config
            .routes
            .iter_mut()
            .find(|r| r.id == route_id)
            .context("Route not found")?;

        let mut count = 0;
        for target in &mut route.targets {
            if target.group.as_ref() == Some(&params.group) {
                target.enabled = true;
                count += 1;
            }
        }

        let _: Value = agent.request(&AgentRequest::ServerConfigSet { config })?;
        Ok(json!({ "ok": true, "count": count }))
    }

    pub(crate) async fn proxy_disable_group(&self, params: GroupParams) -> Result<Value> {
        let agent = self.agent_client()?;
        let mut config: ProxyConfig = agent.request(&AgentRequest::ServerConfigGet)?;

        let route_id = Uuid::parse_str(&params.route_id)?;
        let route = config
            .routes
            .iter_mut()
            .find(|r| r.id == route_id)
            .context("Route not found")?;

        let mut count = 0;
        for target in &mut route.targets {
            if target.group.as_ref() == Some(&params.group) {
                target.enabled = false;
                count += 1;
            }
        }

        let _: Value = agent.request(&AgentRequest::ServerConfigSet { config })?;
        Ok(json!({ "ok": true, "count": count }))
    }

    pub(crate) async fn proxy_list_targets(&self, params: RouteIdParams) -> Result<Value> {
        let agent = self.agent_client()?;
        let config: ProxyConfig = agent.request(&AgentRequest::ServerConfigGet)?;

        let route_id = Uuid::parse_str(&params.route_id)?;
        let route = config
            .routes
            .iter()
            .find(|r| r.id == route_id)
            .context("Route not found")?;

        Ok(serde_json::to_value(&route.targets)?)
    }

    pub(crate) async fn proxy_enable_target(&self, params: TargetParams) -> Result<Value> {
        let agent = self.agent_client()?;
        let mut config: ProxyConfig = agent.request(&AgentRequest::ServerConfigGet)?;

        let route_id = Uuid::parse_str(&params.route_id)?;
        let target_id = Uuid::parse_str(&params.target_id)?;

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
        let _: Value = agent.request(&AgentRequest::ServerConfigSet { config })?;
        Ok(json!({ "ok": true }))
    }

    pub(crate) async fn proxy_disable_target(&self, params: TargetParams) -> Result<Value> {
        let agent = self.agent_client()?;
        let mut config: ProxyConfig = agent.request(&AgentRequest::ServerConfigGet)?;

        let route_id = Uuid::parse_str(&params.route_id)?;
        let target_id = Uuid::parse_str(&params.target_id)?;

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
        let _: Value = agent.request(&AgentRequest::ServerConfigSet { config })?;
        Ok(json!({ "ok": true }))
    }

    pub(crate) async fn proxy_set_target_priority(
        &self,
        params: TargetPriorityParams,
    ) -> Result<Value> {
        let agent = self.agent_client()?;
        let mut config: ProxyConfig = agent.request(&AgentRequest::ServerConfigGet)?;

        let route_id = Uuid::parse_str(&params.route_id)?;
        let target_id = Uuid::parse_str(&params.target_id)?;

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

        target.priority = params
            .priority
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid priority value"))?;
        let _: Value = agent.request(&AgentRequest::ServerConfigSet { config })?;
        Ok(json!({ "ok": true }))
    }

    pub(crate) async fn proxy_set_target_weight(
        &self,
        params: TargetWeightParams,
    ) -> Result<Value> {
        let agent = self.agent_client()?;
        let mut config: ProxyConfig = agent.request(&AgentRequest::ServerConfigGet)?;

        let route_id = Uuid::parse_str(&params.route_id)?;
        let target_id = Uuid::parse_str(&params.target_id)?;

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

        target.weight = params.weight;
        let _: Value = agent.request(&AgentRequest::ServerConfigSet { config })?;
        Ok(json!({ "ok": true }))
    }

    pub(crate) async fn proxy_apply_to_tool(&self, params: ApplyToToolParams) -> Result<Value> {
        let agent = self.agent_client()?;
        let route_id = Uuid::parse_str(&params.route_id)?;

        let tool: aipass_config_writers::ToolId =
            serde_json::from_value(json!(params.tool)).context("Invalid tool identifier")?;

        let request = aipass_agent_protocol::ToolConfigProxyRequest { tool, route_id };

        let response: aipass_agent_protocol::ToolConfigApplyResponse =
            agent.request(&AgentRequest::ToolConfigProxyApply { request })?;

        Ok(serde_json::to_value(&response)?)
    }

    pub(crate) async fn proxy_switch_group(&self, params: GroupParams) -> Result<Value> {
        let agent = self.agent_client()?;
        let mut config: ProxyConfig = agent.request(&AgentRequest::ServerConfigGet)?;

        let route_id = Uuid::parse_str(&params.route_id)?;
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
            if target.group.as_ref() == Some(&params.group) {
                target.enabled = true;
                count += 1;
            }
        }

        let _: Value = agent.request(&AgentRequest::ServerConfigSet { config })?;
        Ok(json!({ "ok": true, "count": count, "group": params.group }))
    }

    pub(crate) async fn proxy_add_target(&self, params: AddTargetParams) -> Result<Value> {
        let agent = self.agent_client()?;
        let mut config: ProxyConfig = agent.request(&AgentRequest::ServerConfigGet)?;

        let route_id = Uuid::parse_str(&params.route_id)?;
        let provider_id = Uuid::parse_str(&params.provider_id)?;

        let entry: EntrySummary = agent.request(&AgentRequest::ProviderGet { id: provider_id })?;
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
            secret_id: params.secret_id,
            label: entry.title.clone(),
            base_url,
            auth_scheme,
            headers: Vec::new(),
            group: params.group,
            priority: params
                .priority
                .unwrap_or(100)
                .try_into()
                .map_err(|_| anyhow::anyhow!("Invalid priority value"))?,
            weight: params.weight.unwrap_or(1),
            enabled: true,
            protocol: None,
            prefer_ws: entry.supports_websockets.unwrap_or(false),
        };

        let target_id = target.id;
        route.targets.push(target);

        let _: Value = agent.request(&AgentRequest::ServerConfigSet { config })?;
        Ok(json!({ "ok": true, "target_id": target_id.to_string() }))
    }

    pub(crate) async fn proxy_remove_target(&self, params: TargetParams) -> Result<Value> {
        let agent = self.agent_client()?;
        let mut config: ProxyConfig = agent.request(&AgentRequest::ServerConfigGet)?;

        let route_id = Uuid::parse_str(&params.route_id)?;
        let target_id = Uuid::parse_str(&params.target_id)?;

        let route = config
            .routes
            .iter_mut()
            .find(|r| r.id == route_id)
            .context("Route not found")?;

        route.targets.retain(|t| t.id != target_id);

        let _: Value = agent.request(&AgentRequest::ServerConfigSet { config })?;
        Ok(json!({ "ok": true }))
    }
}
