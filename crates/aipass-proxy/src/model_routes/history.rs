//! Completed turns in a group are portable; active native state is not.
use super::*;

/// Canonicalize only completed turns preceding a fresh user message. Keeping
/// this independent of the selected member makes retries and return trips see
/// the same history. Opaque compaction cannot be reconstructed and stays bound.
pub(crate) fn make_portable(route: &ResolvedRoute, payload: &mut Value) -> bool {
    if model_id(route).is_none()
        || route.targets.iter().filter(|t| t.config.enabled).count() < 2
        || ["previous_response_id", "conversation"].iter().any(|key| {
            payload
                .get(key)
                .is_some_and(|v| !v.is_null() && v.as_str() != Some(""))
        })
    {
        return false;
    }
    let key = if payload.get("messages").is_some() {
        "messages"
    } else {
        "input"
    };
    let Some(messages) = payload.get_mut(key).and_then(Value::as_array_mut) else {
        return false;
    };
    // A compaction or encrypted message may be the only surviving history.
    // Never erase it merely because a new user message follows it.
    if messages.iter().any(|m| {
        m["type"] == "item_reference"
            || m["type"] != "reasoning"
                && !aipass_proxy_conversion::response_item_ciphertexts(m).is_empty()
    }) {
        return false;
    }
    let boundary = completed_prefix(messages);
    if boundary == 0 {
        return false;
    }
    let mut changed = false;
    let mut empty = HashSet::new();
    for (index, message) in messages[..boundary].iter_mut().enumerate() {
        if message["role"] == "assistant" {
            if let Some(parts) = message.get_mut("content").and_then(Value::as_array_mut) {
                let before = parts.len();
                parts.retain(|p| {
                    !matches!(p["type"].as_str(), Some("thinking" | "redacted_thinking"))
                });
                changed |= before != parts.len();
                if before != parts.len() && parts.is_empty() {
                    empty.insert(index);
                }
                for part in parts {
                    if part["type"] == "tool_use" {
                        changed |= portable_id(part, "id");
                    }
                }
            }
            if let Some(calls) = message.get_mut("tool_calls").and_then(Value::as_array_mut) {
                for call in calls {
                    changed |= portable_id(call, "id");
                }
            }
        }
        if message["role"] == "tool" {
            changed |= portable_id(message, "tool_call_id");
        }
        if message["role"] == "user" {
            for part in message
                .get_mut("content")
                .and_then(Value::as_array_mut)
                .into_iter()
                .flatten()
            {
                if part["type"] == "tool_result" {
                    changed |= portable_id(part, "tool_use_id");
                }
            }
        }
        if matches!(
            message["type"].as_str(),
            Some("function_call" | "function_call_output")
        ) {
            changed |= portable_id(message, "call_id");
        }
    }
    let mut index = 0;
    messages.retain(|message| {
        let keep = index >= boundary
            || message["type"] != "reasoning"
                && (!empty.contains(&index)
                    || message["tool_calls"]
                        .as_array()
                        .is_some_and(|calls| !calls.is_empty()));
        index += 1;
        changed |= !keep;
        keep
    });
    changed
}

fn portable_id(value: &mut Value, field: &str) -> bool {
    let Some(id) = value[field].as_str() else {
        return false;
    };
    let Some(id) = portable_subscription_call_id(id) else {
        return false;
    };
    // Bounded, deterministic IDs preserve call/result associations in every
    // protocol without retaining authority over a completed native session.
    value[field] = Value::String(id);
    true
}

fn completed_prefix(messages: &[Value]) -> usize {
    let mut pending = HashSet::new();
    let mut complete = true;
    let mut boundary = 0;
    for (index, message) in messages.iter().enumerate() {
        let parts = message["content"].as_array();
        let has_results = message["role"] == "tool"
            || message["type"] == "function_call_output"
            || parts.is_some_and(|p| p.iter().any(|b| b["type"] == "tool_result"));
        if message["role"] == "user" && !has_results {
            if complete && pending.is_empty() {
                boundary = index;
            }
            complete = false;
        }
        let calls: Vec<_> = if message["type"] == "function_call" {
            message["call_id"].as_str().into_iter().collect()
        } else if message["role"] == "assistant" {
            message["tool_calls"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|c| c["id"].as_str())
                .chain(
                    parts
                        .into_iter()
                        .flatten()
                        .filter(|p| p["type"] == "tool_use")
                        .filter_map(|p| p["id"].as_str()),
                )
                .collect()
        } else {
            Vec::new()
        };
        if !calls.is_empty() {
            complete = false;
            pending.extend(calls);
        } else if message["role"] == "assistant" && pending.is_empty() {
            complete = true;
        }
        for id in message["tool_call_id"]
            .as_str()
            .into_iter()
            .chain(
                (message["type"] == "function_call_output")
                    .then(|| message["call_id"].as_str())
                    .flatten(),
            )
            .chain(
                parts
                    .into_iter()
                    .flatten()
                    .filter(|p| p["type"] == "tool_result")
                    .filter_map(|p| p["tool_use_id"].as_str()),
            )
        {
            pending.remove(id);
        }
    }
    boundary
}
