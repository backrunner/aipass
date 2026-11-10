//! Plain reasoning exposed by compatible APIs, including Mistral typed parts.
use serde_json::Value;

/// Only nonblank signatures carry provider-owned state. Preserve malformed
/// non-string signatures as bound instead of interpreting them as portable.
pub fn thinking_is_bound(block: &Value) -> bool {
    block
        .get("signature")
        .is_some_and(|v| !v.is_null() && v.as_str().is_none_or(|s| !s.trim().is_empty()))
}

/// Claude's Messages wire cannot replay unsigned thinking from another model.
/// The caller may explicitly identify Anthropic; otherwise only an explicitly
/// named Claude model is normalized. Generic compatible models stay untouched.
pub fn normalize_claude_thinking(payload: &mut Value, anthropic: bool) -> bool {
    let model = payload["model"].as_str().unwrap_or("").to_ascii_lowercase();
    if !anthropic
        && !model
            .split(['/', '.', ':'])
            .any(|part| part.starts_with("claude-"))
    {
        return false;
    }
    let Some(messages) = payload.get_mut("messages").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut changed = false;
    messages.retain_mut(|message| {
        if message["role"] != "assistant" {
            return true;
        }
        let Some(parts) = message.get_mut("content").and_then(Value::as_array_mut) else {
            return true;
        };
        let before = parts.len();
        parts.retain(|part| part["type"] != "thinking" || thinking_is_bound(part));
        let removed = parts.len() != before;
        changed |= removed;
        !removed || !parts.is_empty()
    });
    changed
}

/// Ciphertext in Responses wire items, never arbitrary JSON tool payloads.
/// Read both item-level state and explicitly typed encrypted content blocks.
pub fn response_item_ciphertexts(item: &Value) -> Vec<&str> {
    let mut values = Vec::new();
    if let Some(value) = item["encrypted_content"].as_str().filter(|s| !s.is_empty()) {
        values.push(value);
    }
    if item["type"] == "encrypted_content" {
        if let Some(value) = item["data"].as_str().filter(|s| !s.is_empty()) {
            values.push(value);
        }
    }
    let parts = match item["type"].as_str() {
        Some("message" | "agent_message") => item.get("content"),
        Some("function_call_output" | "custom_tool_call_output") => item.get("output"),
        None if item["role"].is_string() => item.get("content"),
        _ => None,
    };
    for part in parts.and_then(Value::as_array).into_iter().flatten() {
        if part["type"] == "encrypted_content" {
            for key in ["data", "encrypted_content"] {
                if let Some(value) = part[key].as_str().filter(|s| !s.is_empty()) {
                    values.push(value);
                }
            }
        }
    }
    values
}

pub(crate) fn chat_reasoning(message: &Value) -> String {
    let mut text = message
        .get("reasoning_content")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| message.get("reasoning").and_then(Value::as_str))
        .unwrap_or("")
        .to_owned();
    if let Some(parts) = message.get("content").and_then(Value::as_array) {
        for part in parts {
            if part["type"] == "thinking" {
                match part.get("thinking") {
                    Some(Value::String(value)) => text.push_str(value),
                    Some(Value::Array(values)) => {
                        for value in values {
                            if let Some(value) = value.get("text").and_then(Value::as_str) {
                                text.push_str(value);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    text
}

pub(crate) fn anthropic_reasoning(content: Option<&Value>) -> String {
    content
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter(|p| p["type"] == "thinking")
                .filter_map(|p| p.get("thinking").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn claude_history_removes_only_unsigned_assistant_thinking() {
        let content = json!([
            {"type":"thinking","thinking":"no signature"},
            {"type":"thinking","thinking":"blank","signature":" \n"},
            {"type":"thinking","thinking":"signed","signature":"opaque"},
            {"type":"redacted_thinking","data":"opaque"},
            {"type":"tool_use","id":"call","name":"lookup","input":{"type":"thinking","signature":"","text":"user data"}}
        ]);
        for model in [
            "claude-fixture",
            "anthropic.claude-fixture-v1:0",
            "provider/claude-fixture",
        ] {
            let mut body = json!({"model":model,"messages":[
                {"role":"assistant","content":[{"type":"thinking","thinking":"only reasoning"}]},
                {"role":"assistant","content":content},
                {"role":"user","content":[{"type":"tool_result","tool_use_id":"call","content":"found"}]}
            ]});
            assert!(normalize_claude_thinking(&mut body, false));
            assert_eq!(body["messages"].as_array().unwrap().len(), 2);
            assert_eq!(
                body["messages"][0]["content"],
                json!(content.as_array().unwrap()[2..])
            );
            assert!(!normalize_claude_thinking(&mut body, false));
        }
        let mut other = json!({"model":"compatible-reasoner","messages":[{"role":"assistant","content":content}]});
        let original = other.clone();
        assert!(!normalize_claude_thinking(&mut other, false));
        assert_eq!(other, original);
        assert!(normalize_claude_thinking(&mut other, true));
        for signature in [json!(""), json!(" \t"), Value::Null] {
            assert!(!thinking_is_bound(&json!({"signature":signature})));
        }
        for signature in [json!("opaque"), json!(42)] {
            assert!(thinking_is_bound(&json!({"signature":signature})));
        }
    }
}
