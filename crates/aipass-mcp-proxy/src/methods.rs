use aipass_mcp_proxy::{tool_definitions, ProxyMcpServer};
use serde_json::{json, Value};

pub fn handle_initialize(_params: Value) -> Value {
    json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": env!("CARGO_PKG_NAME"),
            "version": env!("CARGO_PKG_VERSION"),
        }
    })
}

pub fn handle_tools_list() -> Value {
    json!({
        "tools": tool_definitions()
    })
}

#[derive(Debug)]
pub enum ToolCallError {
    /// params.name was absent or not a string.
    MissingName,
    /// The name is not one of the advertised tools — a protocol-level error.
    UnknownTool(String),
    /// The tool ran and failed — reported as isError content to the client.
    Failed(String),
}

pub fn handle_tools_call(
    runtime: &tokio::runtime::Runtime,
    vault_dir: std::path::PathBuf,
    params: Value,
) -> Result<Value, ToolCallError> {
    let tool_name = params
        .get("name")
        .and_then(|n| n.as_str())
        .ok_or(ToolCallError::MissingName)?;

    let known = tool_definitions()
        .iter()
        .any(|tool| tool.get("name").and_then(|n| n.as_str()) == Some(tool_name));
    if !known {
        return Err(ToolCallError::UnknownTool(tool_name.to_string()));
    }

    let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);

    let server = ProxyMcpServer::new(vault_dir);

    runtime
        .block_on(server.handle_tool_call(tool_name, arguments))
        .map_err(|e| ToolCallError::Failed(e.to_string()))
}
