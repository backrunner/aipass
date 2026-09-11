mod methods;

use methods::ToolCallError;
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

fn main() {
    let vault_dir = match aipass_agent::default_vault_dir() {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("Failed to get vault directory: {}", e);
            std::process::exit(1);
        }
    };
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("Failed to start runtime: {}", e);
            std::process::exit(1);
        }
    };

    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut reader = stdin.lock();

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let request: Value = match serde_json::from_str(trimmed) {
                    Ok(req) => req,
                    Err(e) => {
                        let response = json!({
                            "jsonrpc": "2.0",
                            "id": null,
                            "error": {
                                "code": -32700,
                                "message": format!("Parse error: {e}")
                            }
                        });
                        if write_response(&mut stdout, &response).is_err() {
                            break;
                        }
                        continue;
                    }
                };

                // JSON-RPC notifications carry no id and must not be answered.
                if request.get("id").is_none() {
                    continue;
                }

                let response = handle_request(&runtime, vault_dir.clone(), request);

                if write_response(&mut stdout, &response).is_err() {
                    break;
                }
            }
            Err(e) => {
                eprintln!("Failed to read from stdin: {}", e);
                break;
            }
        }
    }
}

fn write_response(stdout: &mut impl Write, response: &Value) -> io::Result<()> {
    let payload = serde_json::to_string(response)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    writeln!(stdout, "{}", payload)?;
    stdout.flush()
}

fn handle_request(
    runtime: &tokio::runtime::Runtime,
    vault_dir: std::path::PathBuf,
    request: Value,
) -> Value {
    let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let id = request.get("id");

    let result = match method {
        "ping" => Ok(json!({})),
        "initialize" => Ok(methods::handle_initialize(
            request.get("params").cloned().unwrap_or(Value::Null),
        )),
        "tools/list" => Ok(methods::handle_tools_list()),
        "resources/list" => Ok(json!({ "resources": [] })),
        "prompts/list" => Ok(json!({ "prompts": [] })),
        "tools/call" => {
            let params = request.get("params").cloned().unwrap_or(Value::Null);
            match methods::handle_tools_call(runtime, vault_dir, params) {
                Ok(result) => Ok(json!({
                    "content": [{
                        "type": "text",
                        "text": serde_json::to_string_pretty(&result)
                            .unwrap_or_else(|_| result.to_string())
                    }]
                })),
                Err(ToolCallError::Failed(message)) => Ok(json!({
                    "content": [{ "type": "text", "text": message }],
                    "isError": true
                })),
                Err(ToolCallError::UnknownTool(name)) => {
                    Err((-32602, format!("Unknown tool: {name}")))
                }
                Err(ToolCallError::MissingName) => Err((-32602, "Missing tool name".to_string())),
            }
        }
        _ => Err((-32601, format!("Method not found: {}", method))),
    };

    match result {
        Ok(result) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        }),
        Err((code, message)) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": code,
                "message": message
            }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Runtime::new().expect("runtime")
    }

    #[test]
    fn ping_returns_empty_result() {
        let response = handle_request(
            &test_runtime(),
            PathBuf::new(),
            json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }),
        );
        assert_eq!(response["id"], 1);
        assert_eq!(response["result"], json!({}));
        assert!(response.get("error").is_none());
    }

    #[test]
    fn initialize_advertises_tool_capability() {
        let response = handle_request(
            &test_runtime(),
            PathBuf::new(),
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }),
        );
        let result = &response["result"];
        assert!(result["capabilities"]["tools"].is_object());
        assert_eq!(
            result["serverInfo"]["name"].as_str(),
            Some(env!("CARGO_PKG_NAME"))
        );
        assert!(result["protocolVersion"].is_string());
    }

    #[test]
    fn tools_list_matches_dispatchable_tools() {
        let response = handle_request(
            &test_runtime(),
            PathBuf::new(),
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
        );
        let tools = response["result"]["tools"].as_array().expect("tools array");
        assert!(!tools.is_empty());
        for tool in tools {
            assert!(tool["name"].is_string());
            assert!(tool["inputSchema"].is_object());
        }
    }

    #[test]
    fn unknown_method_is_a_protocol_error() {
        let response = handle_request(
            &test_runtime(),
            PathBuf::new(),
            json!({ "jsonrpc": "2.0", "id": 7, "method": "resources/read" }),
        );
        assert_eq!(response["error"]["code"], -32601);
        assert_eq!(response["id"], 7);
    }

    #[test]
    fn unknown_tool_is_an_invalid_params_error() {
        let response = handle_request(
            &test_runtime(),
            PathBuf::new(),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": { "name": "proxy_nope", "arguments": {} }
            }),
        );
        assert_eq!(response["error"]["code"], -32602);
    }

    #[test]
    fn missing_tool_name_is_an_invalid_params_error() {
        let response = handle_request(
            &test_runtime(),
            PathBuf::new(),
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": { "arguments": {} }
            }),
        );
        assert_eq!(response["error"]["code"], -32602);
    }

    #[test]
    fn tool_execution_failure_is_iserror_content() {
        // proxy_status is a known tool; with a nonexistent vault dir it fails
        // while talking to the agent, which must surface as isError content.
        let response = handle_request(
            &test_runtime(),
            PathBuf::from("/nonexistent-aipass-vault"),
            json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": { "name": "proxy_status", "arguments": {} }
            }),
        );
        assert_eq!(response["id"], 4);
        assert_eq!(response["result"]["isError"], true);
        assert!(response["result"]["content"][0]["text"].is_string());
        assert!(response.get("error").is_none());
    }
}
