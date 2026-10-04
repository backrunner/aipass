//! Plain reasoning exposed by compatible APIs, including Mistral typed parts.
use serde_json::Value;

pub(crate) fn chat_reasoning(message: &Value) -> String {
    let mut text = message
        .get("reasoning_content")
        .or_else(|| message.get("reasoning"))
        .and_then(Value::as_str)
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
