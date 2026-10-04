//! Provider-specific wire normalization. Pure Rust, with no account access or IO.
//! Protocol behavior is independently ported from the pinned Magpie references
//! listed in the repository NOTICE.
use serde_json::{json, Value};
pub mod commandcode;
pub mod connect;
pub mod cursor;
pub mod devin;
pub mod kiro;
pub mod qoder;
pub mod zcode;

/// Common incremental boundary for provider envelopes normalized to SSE.
/// HTTP status is emitted only after the first useful event, allowing a quota
/// refusal in a successful HTTP response to participate in account failover.
pub trait ProviderStream {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>, String>;
    fn finish(&mut self) -> Result<Vec<Vec<u8>>, String>;
    /// Whether generation produced content, even if a later event in the same
    /// network chunk made `push` fail. Such attempts must never be replayed.
    fn has_output(&self) -> bool;
    /// A status only for an explicit upstream refusal. Malformed/truncated
    /// replies leave the submitted generation's outcome unknown.
    fn failure_status(&self) -> Option<u16> {
        None
    }
}
pub fn failure_status(message: &str) -> u16 {
    let text = message.to_lowercase();
    if [
        "quota",
        "rate_limit",
        "rate limit",
        "too many requests",
        "throttl",
        "resource_exhausted",
        "credit",
        "balance",
        "额度",
        "限流",
        "余额",
    ]
    .iter()
    .any(|word| text.contains(word))
    {
        429
    } else if ["unauthenticated", "expired token", "invalid token"]
        .iter()
        .any(|word| text.contains(word))
    {
        401
    } else if ["permission_denied", "access denied"]
        .iter()
        .any(|word| text.contains(word))
    {
        403
    } else if [
        "invalid_argument",
        "context length",
        "too many tokens",
        "too long",
    ]
    .iter()
    .any(|word| text.contains(word))
    {
        400
    } else {
        502
    }
}
pub mod zed;
/// Validate shapes before provider-specific mutation. Opaque native fields are
/// preserved, while malformed arrays/blocks return an error rather than panic.
pub fn validate_request(body: &Value) -> Result<(), String> {
    if !body.is_object() {
        return Err("subscription request must be an object".into());
    }
    for field in ["metadata", "reasoning", "thinking"] {
        if body
            .get(field)
            .is_some_and(|v| !v.is_null() && !v.is_object())
        {
            return Err(format!("subscription {field} must be an object"));
        }
    }
    for field in ["messages", "tools", "contents"] {
        if let Some(v) = body.get(field) {
            let list = v
                .as_array()
                .ok_or_else(|| format!("subscription {field} must be an array"))?;
            if list.len() > 65536 || list.iter().any(|v| !v.is_object()) {
                return Err(format!("invalid subscription {field}"));
            }
        }
    }
    for turn in body["messages"].as_array().into_iter().flatten() {
        if !matches!(
            turn["role"].as_str(),
            Some("system" | "developer" | "user" | "assistant" | "tool")
        ) {
            return Err("invalid subscription message role".into());
        }
        if let Some(v) = turn.get("content") {
            if !(v.is_null()
                || v.is_string()
                || v.as_array().is_some_and(|a| a.iter().all(Value::is_object)))
            {
                return Err("invalid subscription message content".into());
            }
        }
        if let Some(calls) = turn.get("tool_calls") {
            if !calls
                .as_array()
                .is_some_and(|a| a.iter().all(|v| v.is_object() && v["function"].is_object()))
            {
                return Err("invalid subscription tool calls".into());
            }
        }
    }
    if let Some(system) = body.get("system") {
        if !(system.is_null()
            || system.is_string()
            || system
                .as_array()
                .is_some_and(|a| a.iter().all(Value::is_object)))
        {
            return Err("invalid subscription system prompt".into());
        }
    }
    Ok(())
}
pub const DROID: &str = "You are Droid, an AI software engineering agent built by Factory.";
pub fn factory_request(wire: &str, body: &mut Value) -> Result<(), String> {
    match wire {
        "responses" => {
            let instructions = body["instructions"].as_str().unwrap_or("");
            if !instructions.starts_with(DROID) {
                body["instructions"] = json!(if instructions.trim().is_empty() {
                    DROID.to_owned()
                } else {
                    format!("{DROID}\n{instructions}")
                });
            }
        }
        "chat" => {
            let messages = body["messages"]
                .as_array_mut()
                .ok_or("Factory messages must be an array")?;
            if let Some(first) = messages.first_mut().filter(|m| m["role"] == "system") {
                if let Some(s) = first["content"].as_str() {
                    if !s.starts_with(DROID) {
                        first["content"] = json!(format!("{DROID}\n{s}"));
                    }
                } else if let Some(parts) = first["content"].as_array_mut() {
                    if !parts
                        .first()
                        .and_then(|p| p["text"].as_str())
                        .is_some_and(|s| s.starts_with(DROID))
                    {
                        parts.insert(0, json!({"type":"text","text":DROID}));
                    }
                } else {
                    return Err("unsupported Factory system content".into());
                }
            } else {
                messages.insert(0, json!({"role":"system","content":DROID}));
            }
        }
        "anthropic" => {
            let mut blocks = match &body["system"] {
                Value::Null => vec![],
                Value::String(s) => vec![json!({"type":"text","text":s})],
                Value::Array(a) => a.clone(),
                _ => return Err("unsupported Factory system content".into()),
            };
            blocks.retain(|b| {
                !b["text"].as_str().is_some_and(|s| {
                    s.trim().is_empty() || s.starts_with("x-anthropic-billing-header: cc_version=")
                })
            });
            for block in &mut blocks {
                if ["You are Claude Code, Anthropic's official CLI for Claude.","You are Claude Code, Anthropic's official CLI for Claude, running within the Claude Agent SDK.","You are a Claude agent, built on Anthropic's Claude Agent SDK."].contains(&block["text"].as_str().unwrap_or("")) { block["text"]=json!(DROID); }
            }
            if let Some(i) = blocks
                .iter()
                .position(|b| b["text"].as_str().is_some_and(|s| s.starts_with(DROID)))
            {
                let b = blocks.remove(i);
                blocks.insert(0, b);
            } else {
                blocks.insert(0, json!({"type":"text","text":DROID}));
            }
            let mut first = true;
            blocks.retain(|b| {
                if first {
                    first = false;
                    true
                } else {
                    b["text"] != DROID
                }
            });
            body["system"] = json!(blocks);
            for message in body["messages"]
                .as_array_mut()
                .into_iter()
                .flatten()
                .filter(|m| m["role"] == "user")
            {
                for block in message["content"].as_array_mut().into_iter().flatten() {
                    let Some(text) = block["text"].as_str() else {
                        continue;
                    };
                    if let Some(inner) = text
                        .strip_prefix("<system-reminder>\n")
                        .and_then(|s| s.strip_suffix("\n</system-reminder>"))
                    {
                        let lines: Vec<_> = inner.lines().collect();
                        if lines.len() > 2
                            && lines[0] == "# Environment"
                            && lines[1].trim_end()
                                == "You have been invoked in the following environment:"
                            && lines[2..]
                                .iter()
                                .all(|s| s.starts_with(" - ") || s.starts_with("  - "))
                        {
                            block["text"] =
                                json!(text.replace("# Environment", "# Runtime context").replace(
                                    "You have been invoked in the following environment:",
                                    "The session environment is:"
                                ));
                        } else if lines.len() == 1
                            && inner.starts_with("You are powered by the model ")
                            && inner.ends_with('.')
                            && !inner.contains(['<', '>'])
                        {
                            block["text"] = json!(text
                                .replace(
                                    "You are powered by the model named",
                                    "Current model name:"
                                )
                                .replace("You are powered by the model", "Current model:")
                                .replace("The exact model ID is", "Model ID:")
                                .replace(
                                    "Assistant knowledge cutoff is",
                                    "Model knowledge cutoff:"
                                ));
                        }
                    }
                }
            }
        }
        _ => return Err("unsupported Factory wire protocol".into()),
    }
    Ok(())
}
pub fn workbuddy_request(body: &mut Value) -> Result<(), String> {
    let messages = body["messages"]
        .as_array_mut()
        .ok_or("WorkBuddy messages must be an array")?;
    if !messages.is_empty() && messages[0]["role"] != "system" {
        messages.insert(
            0,
            json!({"role":"system","content":"You are a helpful assistant."}),
        );
    }
    body["stream"] = json!(true);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decoded_content_is_remembered_when_later_events_in_the_same_chunk_fail() {
        use connect::{frame, string};
        let qoder_line = |v: Value| {
            format!(
                "data: {}\n\n",
                json!({"statusCodeValue":200,"body":v.to_string()})
            )
        };
        let cases: Vec<(Box<dyn ProviderStream>, Vec<u8>)> = vec![
            (Box::new(devin::DevinStream::new("m", "id")), [
                frame(0, &string(3, "content")),
                frame(2, br#"{"error":{"code":"resource_exhausted"}}"#)
            ].concat()),
            (Box::new(commandcode::GoStream::new("m", "id")), b"{\"type\":\"text-delta\",\"text\":\"content\"}\n{\"type\":\"error\",\"error\":\"quota\"}\n".to_vec()),
            (Box::new(qoder::QoderStream::new("m", "id")), (qoder_line(json!({"choices":[{"delta":{"content":"content"}}]})) + "data: {\"statusCodeValue\":429}\n\n").into_bytes()),
            (Box::new(zed::ZedStream::new("anthropic", true)), b"{\"event\":{\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"content\"}}}\n{\"status\":{\"failed\":{\"code\":\"upstream_http_429\"}}}\n".to_vec()),
        ];
        for (mut decoder, body) in cases {
            assert!(!decoder.has_output());
            assert!(decoder.push(&body).is_err());
            assert!(
                decoder.has_output(),
                "decoded generation must prevent replay"
            );
            assert_eq!(decoder.failure_status(), Some(429));
        }
    }
    #[test]
    fn malformed_request_shapes_fail_before_provider_mutation() {
        for body in [
            json!([]),
            json!({"messages":[1]}),
            json!({"messages":[{"role":"user","content":["bad"]}]}),
            json!({"metadata":"bad"}),
            json!({"tools":[false]}),
        ] {
            assert!(validate_request(&body).is_err());
        }
        validate_request(&json!({"messages":[{"role":"assistant","content":null,"tool_calls":[{"function":{"name":"calc","arguments":"{}"}}]}]})).unwrap();
    }
    #[test]
    fn factory_preserves_user_prompt_tools_and_large_integers() {
        let mut v = json!({"system":[{"type":"text","text":"You are Claude Code, Anthropic's official CLI for Claude."},{"type":"text","text":"Keep my rules."}],"messages":[{"role":"user","content":[{"type":"text","text":"<system-reminder>\nYou are powered by the model Foo.\n</system-reminder>\nIs this true?"}]}],"tools":[{"name":"calc","input_schema":{"const":9007199254740993u64}}]});
        let before = v.clone();
        factory_request("anthropic", &mut v).unwrap();
        assert_eq!(v["system"][0]["text"], DROID);
        assert_eq!(v["system"][1], before["system"][1]);
        assert_eq!(v["messages"], before["messages"]);
        assert_eq!(v["tools"], before["tools"]);
        let normalized = v.clone();
        factory_request("anthropic", &mut v).unwrap();
        assert_eq!(v, normalized);
    }
    #[test]
    fn workbuddy_forces_wire_stream_without_replacing_instructions() {
        let mut v = json!({"messages":[{"role":"system","content":"My rules"}],"stream":false});
        workbuddy_request(&mut v).unwrap();
        assert_eq!(v["messages"][0]["content"], "My rules");
        assert_eq!(v["stream"], true);
    }
}
