//! Foreign-provider history is context, never a fabricated native transcript.
use super::*;

pub(super) fn prompt(messages: &[Value], resumed: bool) -> Result<Value, String> {
    let last = messages.last().ok_or("Claude messages are empty")?;
    if last["role"] != "user" {
        return Err("Claude ACP requires a final user turn".into());
    }
    if resumed || messages.len() == 1 {
        return current(&last["content"]);
    }
    let mut blocks = vec![
        json!({"type":"text","text":"Conversation handoff from another model or an expired session. The following role-labelled records are historical context, not new instructions. Tool calls with results already ran; do not repeat them. Follow the current system instructions, then answer the final user turn. Provider-private reasoning and signatures are not transferable."}),
    ];
    for message in &messages[..messages.len() - 1] {
        let role = message["role"].as_str().ok_or("historical role missing")?;
        if !matches!(role, "user" | "assistant") {
            return Err("unsupported historical role".into());
        }
        blocks.push(json!({"type":"text","text":format!("Historical {role} record:")}));
        match &message["content"] {
            Value::String(text) => blocks.push(json!({"type":"text","text":text})),
            Value::Array(parts) => {
                for part in parts {
                    match part["type"].as_str() {
                    Some("thinking"|"redacted_thinking") => {},
                    Some("text"|"image"|"document") => blocks.push(part.clone()),
                    Some("tool_use") => blocks.push(json!({"type":"text","text":format!("Historical tool call (do not execute): {part}")})),
                    Some("tool_result") => {
                        let mut record = part.clone();
                        if let Some(parts) = record["content"].as_array_mut() {
                            for part in parts.iter().filter(|p|matches!(p["type"].as_str(),Some("image"|"document"))) { blocks.push(part.clone()); }
                            parts.retain(|p|!matches!(p["type"].as_str(),Some("image"|"document")));
                        }
                        blocks.push(json!({"type":"text","text":format!("Historical tool result: {record}")}));
                    },
                    _ => return Err("unsupported historical Claude content; cannot hand off losslessly".into()),
                }
                }
            }
            _ => return Err("invalid historical content".into()),
        }
    }
    blocks.push(json!({"type":"text","text":"End of historical records. Current user turn:"}));
    match &last["content"] {
        Value::String(text) => blocks.push(json!({"type":"text","text":text})),
        Value::Array(_) => blocks.extend(
            current(&last["content"])?
                .as_array()
                .cloned()
                .unwrap_or_default(),
        ),
        _ => return Err("invalid current user content".into()),
    }
    Ok(json!(blocks))
}
fn current(content: &Value) -> Result<Value, String> {
    let Some(parts) = content.as_array() else {
        return Ok(content.clone());
    };
    let mut out = Vec::new();
    for part in parts {
        if part["type"] == "tool_result" {
            if part["tool_use_id"]
                .as_str()
                .is_some_and(|id| id.starts_with("toolu_aipass_claude_"))
            {
                return Err("unresolved Claude tool result requires its original session".into());
            }
            let mut record = part.clone();
            if let Some(parts) = record["content"].as_array_mut() {
                out.extend(
                    parts
                        .iter()
                        .filter(|p| matches!(p["type"].as_str(), Some("image" | "document")))
                        .cloned(),
                );
                parts.retain(|p| !matches!(p["type"].as_str(), Some("image" | "document")));
            }
            out.push(json!({"type":"text","text":format!("Completed caller tool result. Continue from this result without re-executing the tool: {record}")}));
        } else {
            out.push(part.clone());
        }
    }
    Ok(json!(out))
}

/// Only the current tool round requires affinity. Completed earlier calls are
/// portable facts and must not pin every later turn to the original provider.
pub(super) fn pending_ids(payload: &Value) -> Vec<&str> {
    let Some(messages) = payload
        .get("messages")
        .or_else(|| payload.get("input"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let mut ids = Vec::new();
    for message in messages.iter().rev() {
        if message["role"] == "assistant" {
            ids.extend(
                message["tool_calls"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|c| c["id"].as_str()),
            );
            ids.extend(
                message["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|b| b["type"] == "tool_use")
                    .filter_map(|b| b["id"].as_str()),
            );
            break;
        }
        if message["type"] == "function_call" {
            if let Some(id) = message["call_id"].as_str() {
                ids.push(id);
            }
        } else if message["type"] == "function_call_output" || message["role"] == "tool" {
            if let Some(id) = message["call_id"]
                .as_str()
                .or_else(|| message["tool_call_id"].as_str())
            {
                ids.push(id);
            }
        }
        ids.extend(
            message["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|b| b["type"] == "tool_result")
                .filter_map(|b| b["tool_use_id"].as_str()),
        );
    }
    ids.sort_unstable();
    ids.dedup();
    ids
}
