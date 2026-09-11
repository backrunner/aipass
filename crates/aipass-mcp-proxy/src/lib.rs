mod handlers;
mod params;
mod tools;

use crate::params::*;
use aipass_agent::AgentClient;
use anyhow::{Context, Result};
use serde_json::Value;
use std::path::PathBuf;

pub use tools::tool_definitions;

pub struct ProxyMcpServer {
    vault_dir: PathBuf,
}

impl ProxyMcpServer {
    pub fn new(vault_dir: PathBuf) -> Self {
        Self { vault_dir }
    }

    fn agent_client(&self) -> Result<AgentClient> {
        AgentClient::for_vault(self.vault_dir.clone()).context("Failed to connect to agent")
    }

    pub async fn handle_tool_call(&self, name: &str, arguments: Value) -> Result<Value> {
        match name {
            "proxy_status" => self.proxy_status().await,
            "proxy_start" => self.proxy_start().await,
            "proxy_stop" => self.proxy_stop().await,
            "proxy_restart" => self.proxy_restart().await,
            "proxy_list_routes" => self.proxy_list_routes().await,
            "proxy_create_route" => {
                let params: CreateRouteParams = serde_json::from_value(arguments)?;
                self.proxy_create_route(params).await
            }
            "proxy_update_route" => {
                let params: UpdateRouteParams = serde_json::from_value(arguments)?;
                self.proxy_update_route(params).await
            }
            "proxy_delete_route" => {
                let params: DeleteRouteParams = serde_json::from_value(arguments)?;
                self.proxy_delete_route(params).await
            }
            "proxy_enable_route" => {
                let params: RouteIdParams = serde_json::from_value(arguments)?;
                self.proxy_enable_route(params).await
            }
            "proxy_disable_route" => {
                let params: RouteIdParams = serde_json::from_value(arguments)?;
                self.proxy_disable_route(params).await
            }
            "proxy_token_rotate" => {
                let params: RouteIdParams = serde_json::from_value(arguments)?;
                self.proxy_token_rotate(params).await
            }
            "proxy_usage" => {
                let params: UsageParams = serde_json::from_value(arguments)?;
                self.proxy_usage(params).await
            }
            "proxy_logs" => {
                let params: LogsParams = serde_json::from_value(arguments)?;
                self.proxy_logs(params).await
            }
            "proxy_list_providers" => self.proxy_list_providers().await,
            "proxy_get_provider" => {
                let params: ProviderIdParams = serde_json::from_value(arguments)?;
                self.proxy_get_provider(params).await
            }
            "proxy_create_provider" => {
                let params: CreateProviderParams = serde_json::from_value(arguments)?;
                self.proxy_create_provider(params).await
            }
            "proxy_update_provider" => {
                let params: UpdateProviderParams = serde_json::from_value(arguments)?;
                self.proxy_update_provider(params).await
            }
            "proxy_delete_provider" => {
                let params: ProviderIdParams = serde_json::from_value(arguments)?;
                self.proxy_delete_provider(params).await
            }
            "proxy_reorder_targets" => {
                let params: ReorderTargetsParams = serde_json::from_value(arguments)?;
                self.proxy_reorder_targets(params).await
            }
            "proxy_list_groups" => {
                let params: RouteIdParams = serde_json::from_value(arguments)?;
                self.proxy_list_groups(params).await
            }
            "proxy_enable_group" => {
                let params: GroupParams = serde_json::from_value(arguments)?;
                self.proxy_enable_group(params).await
            }
            "proxy_disable_group" => {
                let params: GroupParams = serde_json::from_value(arguments)?;
                self.proxy_disable_group(params).await
            }
            "proxy_list_targets" => {
                let params: RouteIdParams = serde_json::from_value(arguments)?;
                self.proxy_list_targets(params).await
            }
            "proxy_enable_target" => {
                let params: TargetParams = serde_json::from_value(arguments)?;
                self.proxy_enable_target(params).await
            }
            "proxy_disable_target" => {
                let params: TargetParams = serde_json::from_value(arguments)?;
                self.proxy_disable_target(params).await
            }
            "proxy_set_target_priority" => {
                let params: TargetPriorityParams = serde_json::from_value(arguments)?;
                self.proxy_set_target_priority(params).await
            }
            "proxy_set_target_weight" => {
                let params: TargetWeightParams = serde_json::from_value(arguments)?;
                self.proxy_set_target_weight(params).await
            }
            "proxy_apply_to_tool" => {
                let params: ApplyToToolParams = serde_json::from_value(arguments)?;
                self.proxy_apply_to_tool(params).await
            }
            "proxy_switch_group" => {
                let params: GroupParams = serde_json::from_value(arguments)?;
                self.proxy_switch_group(params).await
            }
            "proxy_add_target" => {
                let params: AddTargetParams = serde_json::from_value(arguments)?;
                self.proxy_add_target(params).await
            }
            "proxy_remove_target" => {
                let params: TargetParams = serde_json::from_value(arguments)?;
                self.proxy_remove_target(params).await
            }
            _ => Err(anyhow::anyhow!("Unknown tool: {}", name)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every tool advertised in tools/list must be reachable through
    /// handle_tool_call — a drift between the schema and the dispatch match
    /// would leave clients calling into "Unknown tool" errors.
    #[test]
    fn every_advertised_tool_is_dispatchable() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let server = ProxyMcpServer::new(PathBuf::from("/nonexistent-aipass-vault"));
        for tool in tool_definitions() {
            let name = tool["name"].as_str().expect("tool name");
            let result = runtime.block_on(server.handle_tool_call(name, Value::Null));
            if let Err(err) = result {
                assert!(
                    !err.to_string().contains("Unknown tool"),
                    "advertised tool '{name}' is not dispatched"
                );
            }
        }
    }
}
