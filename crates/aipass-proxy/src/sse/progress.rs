//! Output evidence in snapshots matters even when its enclosing event failed.
use super::*;
use serde_json::Value;

pub(super) fn part_output(part: &Value) -> bool {
    part["type"].as_str().is_some_and(|kind| {
        // Unknown output kinds may represent server-side execution or media.
        // Only known empty text/message placeholders are safe to disregard.
        !matches!(
            kind,
            "message"
                | "reasoning"
                | "text"
                | "output_text"
                | "summary_text"
                | "thinking"
                | "refusal"
        )
    }) || ["text", "thinking", "refusal", "data", "encrypted_content"]
        .iter()
        .any(|k| part.get(*k).is_some_and(json_value_has_output))
}
pub(super) fn item_output(item: &Value) -> bool {
    part_output(item)
        || ["content", "summary"].iter().any(|k| {
            item[*k]
                .as_array()
                .is_some_and(|parts| parts.iter().any(part_output))
        })
}
pub(super) fn response_output(value: &Value) -> bool {
    value["output"]
        .as_array()
        .is_some_and(|items| items.iter().any(item_output))
}
pub(crate) fn buffered_response_has_output(protocol: ProxyProtocol, bytes: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return false;
    };
    match protocol {
        ProxyProtocol::OpenAiResponses => {
            response_output(&value) || value.get("response").is_some_and(response_output)
        }
        ProxyProtocol::AnthropicMessages => value["content"]
            .as_array()
            .is_some_and(|parts| parts.iter().any(part_output)),
        ProxyProtocol::OpenAiChatCompletions => {
            value["choices"].as_array().is_some_and(|choices| {
                choices.iter().any(|c| {
                    let message = &c["message"];
                    [
                        "content",
                        "reasoning",
                        "reasoning_content",
                        "refusal",
                        "tool_calls",
                        "function_call",
                        "audio",
                    ]
                    .iter()
                    .any(|k| message.get(*k).is_some_and(json_value_has_output))
                })
            })
        }
    }
}
