//! Shared Rust-owned search executor for model tools and authenticated MCP.
use crate::*;
use serde_json::{json, Value};
mod api;
mod config;
mod fetch;
pub use fetch::public_fetch_address;
mod gateway;
mod history;
pub(crate) use history::History;
mod mcp;
mod model;
mod render;
mod wire;
pub use config::*;
pub(crate) use gateway::forward;
pub(crate) fn needs_websocket_bridge(state: &RuntimeState, route: &ResolvedRoute) -> bool {
    available(state) && !gateway::native(route, ProxyProtocol::OpenAiResponses)
}
pub(crate) use mcp::handle;
#[cfg(test)]
mod tests;

pub(super) const MCP_PATH: &str = "/mcp/aipass/web-search";
const MAX_RESULT: usize = 128 * 1024;
async fn execute(state: &RuntimeState, query: &str) -> Result<Value, String> {
    if query.trim().is_empty() || query.len() > 4096 {
        return Err("search query must contain 1–4096 bytes".into());
    }
    let config = state
        .config
        .read()
        .map_err(|_| "search configuration unavailable")?
        .search
        .clone();
    if config.api_first {
        if let Ok(result) = api::search(state, query).await {
            return Ok(result);
        }
    }
    if let Some(model) = config.model {
        if let Ok(result) = model::search(state, &model, query).await {
            return Ok(result);
        }
    }
    if !config.api_first {
        return api::search(state, query).await;
    }
    Err("web search failed; check the configured search sources".into())
}
fn available(state: &RuntimeState) -> bool {
    state
        .config
        .read()
        .is_ok_and(|c| !c.search_apis.is_empty() || c.search.model.is_some())
}
