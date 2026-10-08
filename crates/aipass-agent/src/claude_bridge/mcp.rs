//! Authenticated Agent IPC for caller-provided tools.
use super::*;

pub fn run_mcp_helper(vault: PathBuf) -> anyhow::Result<()> {
    let mut capability = std::env::var("AIPASS_CLAUDE_CAPABILITY")?;
    let client = crate::AgentClient::for_vault(vault)?;
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let mut stdout = std::io::stdout();
    loop {
        use std::io::Read;
        let mut line = String::new();
        let n = reader.by_ref().take(MAX_LINE + 1).read_line(&mut line)?;
        if n == 0 {
            break;
        }
        if n as u64 > MAX_LINE {
            anyhow::bail!("MCP request exceeds limit");
        }
        let request: Value = serde_json::from_str(&line)?;
        line.zeroize();
        let Some(id) = request.get("id") else {
            continue;
        };
        let call = |request| {
            client.request::<Value>(&AgentRequest::ClaudeBridgeMcp {
                capability: SensitiveString::new(&capability),
                request,
            })
        };
        let result: Result<Value, String> = match request["method"].as_str() {
            Some("initialize") => Ok(
                json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"aipass","version":"1"}}),
            ),
            Some("ping") => Ok(json!({})),
            Some("tools/list") => call(ClaudeBridgeRequest::ListTools).map_err(|e| e.to_string()),
            Some("tools/call") => (|| {
                let deadline = Instant::now() + Duration::from_secs(10);
                let start = loop {
                    let value = call(ClaudeBridgeRequest::Call {
                        name: request["params"]["name"]
                            .as_str()
                            .ok_or("missing tool name")?
                            .into(),
                        arguments: request["params"]["arguments"].clone(),
                    })
                    .map_err(|e| e.to_string())?;
                    if value.get("callId").is_some() {
                        break value;
                    }
                    if Instant::now() >= deadline {
                        return Err("tool call does not match the model request".into());
                    }
                    std::thread::sleep(Duration::from_millis(25));
                };
                let id = start["callId"].as_str().ok_or("missing call ID")?;
                let deadline = Instant::now() + PARK_TTL;
                loop {
                    let value = call(ClaudeBridgeRequest::Poll { call_id: id.into() })
                        .map_err(|e| e.to_string())?;
                    if let Some(result) = value.get("result") {
                        return Ok(result.clone());
                    }
                    if Instant::now() >= deadline {
                        return Err("tool result timed out".into());
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            })(),
            _ => Err("unsupported MCP method".into()),
        };
        let response = match result {
            Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
            Err(message) => {
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":message}})
            }
        };
        writeln!(stdout, "{response}")?;
        stdout.flush()?;
    }
    capability.zeroize();
    Ok(())
}
