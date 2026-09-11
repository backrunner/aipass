use aipass_proxy::{Protocol as ProxyProtocol, ProxyTargetConfig, RouteStrategy};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub(crate) struct CreateRouteParams {
    pub name: String,
    pub enabled: Option<bool>,
    pub inbound_protocol: ProxyProtocol,
    pub upstream_protocol: ProxyProtocol,
    pub conversion_enabled: Option<bool>,
    pub strategy: Option<RouteStrategy>,
    pub targets: Vec<ProxyTargetConfig>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpdateRouteParams {
    pub route_id: String,
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub inbound_protocol: Option<ProxyProtocol>,
    pub upstream_protocol: Option<ProxyProtocol>,
    pub conversion_enabled: Option<bool>,
    pub strategy: Option<RouteStrategy>,
    pub targets: Option<Vec<ProxyTargetConfig>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DeleteRouteParams {
    pub route_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RouteIdParams {
    pub route_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UsageParams {
    pub days: Option<u32>,
    pub timezone_offset_minutes: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LogsParams {
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ProviderIdParams {
    pub provider_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CreateProviderParams {
    pub provider_data: Value, // Raw JSON matching ProviderEntryInput
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpdateProviderParams {
    pub entry_id: String,
    pub provider_data: Value, // Raw JSON matching ProviderEntryUpdateInput
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReorderTargetsParams {
    pub route_id: String,
    pub targets: Vec<TargetOrder>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TargetOrder {
    pub target_id: String,
    pub weight: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GroupParams {
    pub route_id: String,
    pub group: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TargetParams {
    pub route_id: String,
    pub target_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TargetPriorityParams {
    pub route_id: String,
    pub target_id: String,
    pub priority: i32,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TargetWeightParams {
    pub route_id: String,
    pub target_id: String,
    pub weight: u32,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ApplyToToolParams {
    pub route_id: String,
    pub tool: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AddTargetParams {
    pub route_id: String,
    pub provider_id: String,
    pub secret_id: String,
    pub group: Option<String>,
    pub priority: Option<i32>,
    pub weight: Option<u32>,
}
