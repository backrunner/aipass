//! Non-streaming response conversion between Anthropic Messages (AM) and
//! the OpenAI wire formats (CC = Chat Completions, RS = Responses).

use serde_json::{json, Map, Value};

use crate::{invalid, number, ConversionError, ProxyProtocol};

use ProxyProtocol::{AnthropicMessages as AM, OpenAiChatCompletions as CC, OpenAiResponses as RS};

pub(crate) fn convert(
    from: ProxyProtocol,
    to: ProxyProtocol,
    payload: Value,
) -> Result<Value, ConversionError> {
    match (from, to) {
        (AM, CC) => am_to_cc(payload),
        (AM, RS) => am_to_rs(payload),
        (CC, AM) => cc_to_am(payload),
        (RS, AM) => rs_to_am(payload),
        _ => Err(ConversionError::Unsupported(from, to)),
    }
}

fn object(
    payload: &Value,
    protocol: ProxyProtocol,
) -> Result<&Map<String, Value>, ConversionError> {
    payload
        .as_object()
        .ok_or_else(|| invalid(protocol, "response must be a JSON object"))
}

fn json_string(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn block_type(block: &Value) -> &str {
    block.get("type").and_then(Value::as_str).unwrap_or("")
}

pub(crate) fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// Rewrite a wire id prefix (`msg_`/`chatcmpl-`/`resp_`); unprefixed ids
/// gain the target prefix.
pub(crate) fn swap_id_prefix(id: &str, from_prefix: &str, to_prefix: &str) -> String {
    match id.strip_prefix(from_prefix) {
        Some(rest) => format!("{to_prefix}{rest}"),
        None => format!("{to_prefix}{id}"),
    }
}

/// AM stop_reason -> CC finish_reason.
pub(crate) fn am_stop_to_cc_finish(stop_reason: Option<&str>) -> &'static str {
    match stop_reason {
        Some("max_tokens") => "length",
        Some("tool_use") => "tool_calls",
        _ => "stop",
    }
}

/// CC finish_reason -> AM stop_reason.
pub(crate) fn cc_finish_to_am_stop(finish_reason: Option<&str>) -> &'static str {
    match finish_reason {
        Some("length") => "max_tokens",
        Some("tool_calls") => "tool_use",
        _ => "end_turn",
    }
}

/// Split AM content blocks into plain text and CC/RS-style tool calls.
fn am_content_parts(content: Option<&Value>) -> (String, Vec<Value>) {
    let mut texts: Vec<String> = Vec::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    if let Some(Value::Array(blocks)) = content {
        for block in blocks {
            match block_type(block) {
                "text" => {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        texts.push(text.to_string());
                    }
                }
                "tool_use" => tool_calls.push(block.clone()),
                _ => {}
            }
        }
    }
    (texts.join("\n"), tool_calls)
}

/// AM usage counts cached tokens separately from `input_tokens`; OpenAI
/// usage folds them into the input total with detail breakdowns. The output
/// key differs per wire format (`completion_tokens` for CC).
fn am_usage_to_openai(
    usage: Option<&Value>,
    input_key: &str,
    output_key: &str,
    details_key: &str,
) -> Value {
    let usage = usage.cloned().unwrap_or(Value::Null);
    let input = number(usage.get("input_tokens"));
    let output = number(usage.get("output_tokens"));
    let cache_read = number(usage.get("cache_read_input_tokens"));
    let cache_creation = number(usage.get("cache_creation_input_tokens"));
    let total_input = input + cache_read + cache_creation;
    json!({
        input_key: total_input,
        output_key: output,
        "total_tokens": total_input + output,
        details_key: {"cached_tokens": cache_read, "cache_creation_tokens": cache_creation},
    })
}

fn openai_usage_to_am(usage: Option<&Value>, input_key: &str, details_key: &str) -> Value {
    let usage = usage.cloned().unwrap_or(Value::Null);
    let total_input = number(usage.get(input_key));
    let cache_read = number(usage.pointer(&format!("/{details_key}/cached_tokens")));
    let cache_creation = number(usage.pointer(&format!("/{details_key}/cache_creation_tokens")));
    json!({
        "input_tokens": total_input.saturating_sub(cache_read).saturating_sub(cache_creation),
        "output_tokens": number(usage.get("output_tokens")).max(number(usage.get("completion_tokens"))),
        "cache_read_input_tokens": cache_read,
        "cache_creation_input_tokens": cache_creation,
    })
}

fn parse_tool_arguments(
    arguments: Option<&Value>,
    protocol: ProxyProtocol,
) -> Result<Value, ConversionError> {
    let raw = arguments.and_then(Value::as_str).unwrap_or("{}");
    serde_json::from_str(raw).map_err(|err| {
        invalid(
            protocol,
            format!("tool arguments are not valid JSON: {err}"),
        )
    })
}

// --- AM -> CC -------------------------------------------------------------

fn am_to_cc(payload: Value) -> Result<Value, ConversionError> {
    let src = object(&payload, AM)?;
    let (text, tool_uses) = am_content_parts(src.get("content"));
    let tool_calls: Vec<Value> = tool_uses
        .iter()
        .map(|block| {
            json!({
                "id": block.get("id").cloned().unwrap_or(Value::Null),
                "type": "function",
                "function": {
                    "name": block.get("name").cloned().unwrap_or(Value::Null),
                    "arguments": json_string(block.get("input").unwrap_or(&Value::Null)),
                }
            })
        })
        .collect();

    let mut message = Map::new();
    message.insert("role".into(), json!("assistant"));
    message.insert(
        "content".into(),
        if text.is_empty() && !tool_calls.is_empty() {
            Value::Null
        } else {
            json!(text)
        },
    );
    if !tool_calls.is_empty() {
        message.insert("tool_calls".into(), Value::Array(tool_calls));
    }

    let id = src.get("id").and_then(Value::as_str).unwrap_or("msg_conv");
    Ok(json!({
        "id": swap_id_prefix(id, "msg_", "chatcmpl_"),
        "object": "chat.completion",
        "created": unix_now(),
        "model": src.get("model").cloned().unwrap_or(Value::Null),
        "choices": [{
            "index": 0,
            "message": Value::Object(message),
            "finish_reason": am_stop_to_cc_finish(src.get("stop_reason").and_then(Value::as_str)),
        }],
        "usage": am_usage_to_openai(src.get("usage"), "prompt_tokens", "completion_tokens", "prompt_tokens_details"),
    }))
}

// --- AM -> RS -------------------------------------------------------------

fn am_to_rs(payload: Value) -> Result<Value, ConversionError> {
    let src = object(&payload, AM)?;
    let (text, tool_uses) = am_content_parts(src.get("content"));
    let mut output: Vec<Value> = Vec::new();
    if !text.is_empty() {
        output.push(json!({
            "type": "message",
            "id": src.get("id").cloned().unwrap_or_else(|| json!("msg_conv")),
            "role": "assistant",
            "status": "completed",
            "content": [{"type": "output_text", "text": text}],
        }));
    }
    for block in &tool_uses {
        let call_id = block.get("id").and_then(Value::as_str).unwrap_or("");
        output.push(json!({
            "type": "function_call",
            "id": swap_id_prefix(call_id, "toolu_", "fc_"),
            "call_id": call_id,
            "name": block.get("name").cloned().unwrap_or(Value::Null),
            "arguments": json_string(block.get("input").unwrap_or(&Value::Null)),
        }));
    }

    let maxed = src.get("stop_reason").and_then(Value::as_str) == Some("max_tokens");
    let id = src.get("id").and_then(Value::as_str).unwrap_or("msg_conv");
    let mut response = Map::new();
    response.insert("id".into(), json!(swap_id_prefix(id, "msg_", "resp_")));
    response.insert("object".into(), json!("response"));
    response.insert("created_at".into(), json!(unix_now()));
    response.insert(
        "status".into(),
        json!(if maxed { "incomplete" } else { "completed" }),
    );
    if maxed {
        response.insert(
            "incomplete_details".into(),
            json!({"reason": "max_output_tokens"}),
        );
    }
    response.insert(
        "model".into(),
        src.get("model").cloned().unwrap_or(Value::Null),
    );
    response.insert("output".into(), Value::Array(output));
    response.insert(
        "usage".into(),
        am_usage_to_openai(
            src.get("usage"),
            "input_tokens",
            "output_tokens",
            "input_tokens_details",
        ),
    );
    Ok(Value::Object(response))
}

// --- CC -> AM -------------------------------------------------------------

fn cc_to_am(payload: Value) -> Result<Value, ConversionError> {
    let src = object(&payload, CC)?;
    let choice = src
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .ok_or_else(|| invalid(CC, "response has no choices"))?;
    let message = choice.get("message").cloned().unwrap_or(Value::Null);

    let mut content: Vec<Value> = Vec::new();
    match message.get("content") {
        Some(Value::String(text)) if !text.is_empty() => {
            content.push(json!({"type": "text", "text": text}));
        }
        Some(Value::Array(parts)) => {
            for part in parts {
                if part.get("type").and_then(Value::as_str) == Some("text") {
                    content.push(json!({"type": "text", "text": part.get("text").cloned().unwrap_or(Value::Null)}));
                }
            }
        }
        _ => {}
    }
    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in tool_calls {
            let function = call.get("function").cloned().unwrap_or(Value::Null);
            content.push(json!({
                "type": "tool_use",
                "id": call.get("id").cloned().unwrap_or(Value::Null),
                "name": function.get("name").cloned().unwrap_or(Value::Null),
                "input": parse_tool_arguments(function.get("arguments"), CC)?,
            }));
        }
    }

    let id = src
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("chatcmpl_conv");
    Ok(json!({
        "id": swap_id_prefix(id, "chatcmpl_", "msg_"),
        "type": "message",
        "role": "assistant",
        "model": src.get("model").cloned().unwrap_or(Value::Null),
        "content": content,
        "stop_reason": cc_finish_to_am_stop(choice.get("finish_reason").and_then(Value::as_str)),
        "stop_sequence": Value::Null,
        "usage": openai_usage_to_am(src.get("usage"), "prompt_tokens", "prompt_tokens_details"),
    }))
}

// --- RS -> AM -------------------------------------------------------------

fn rs_to_am(payload: Value) -> Result<Value, ConversionError> {
    let src = object(&payload, RS)?;
    let output = src
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(RS, "response has no output array"))?;

    let mut content: Vec<Value> = Vec::new();
    let mut saw_tool = false;
    for item in output {
        match item.get("type").and_then(Value::as_str) {
            Some("message") => {
                if let Some(parts) = item.get("content").and_then(Value::as_array) {
                    for part in parts {
                        if part.get("type").and_then(Value::as_str) == Some("output_text") {
                            content.push(json!({"type": "text", "text": part.get("text").cloned().unwrap_or(Value::Null)}));
                        }
                    }
                }
            }
            Some("function_call") => {
                saw_tool = true;
                content.push(json!({
                    "type": "tool_use",
                    "id": item.get("call_id").or_else(|| item.get("id")).cloned().unwrap_or(Value::Null),
                    "name": item.get("name").cloned().unwrap_or(Value::Null),
                    "input": parse_tool_arguments(item.get("arguments"), RS)?,
                }));
            }
            _ => {}
        }
    }

    let incomplete = src.get("status").and_then(Value::as_str) == Some("incomplete");
    let stop_reason = if incomplete {
        "max_tokens"
    } else if saw_tool {
        "tool_use"
    } else {
        "end_turn"
    };
    let id = src.get("id").and_then(Value::as_str).unwrap_or("resp_conv");
    Ok(json!({
        "id": swap_id_prefix(id, "resp_", "msg_"),
        "type": "message",
        "role": "assistant",
        "model": src.get("model").cloned().unwrap_or(Value::Null),
        "content": content,
        "stop_reason": stop_reason,
        "stop_sequence": Value::Null,
        "usage": openai_usage_to_am(src.get("usage"), "input_tokens", "input_tokens_details"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn am_to_cc_text_and_usage() {
        let out = am_to_cc(json!({
            "id": "msg_123", "type": "message", "role": "assistant", "model": "claude-sonnet-4-5",
            "content": [{"type": "text", "text": "Hello!"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5, "cache_read_input_tokens": 3, "cache_creation_input_tokens": 2}
        }))
        .unwrap();
        assert_eq!(out["id"], "chatcmpl_123");
        assert_eq!(out["object"], "chat.completion");
        assert_eq!(out["choices"][0]["message"]["role"], "assistant");
        assert_eq!(out["choices"][0]["message"]["content"], "Hello!");
        assert_eq!(out["choices"][0]["finish_reason"], "stop");
        assert_eq!(out["usage"]["prompt_tokens"], 15);
        assert_eq!(out["usage"]["completion_tokens"], 5);
        assert_eq!(out["usage"]["total_tokens"], 20);
        assert_eq!(out["usage"]["prompt_tokens_details"]["cached_tokens"], 3);
    }

    #[test]
    fn am_to_cc_tool_use_maps_to_tool_calls() {
        let out = am_to_cc(json!({
            "id": "msg_1", "model": "m",
            "content": [
                {"type": "text", "text": "Checking."},
                {"type": "tool_use", "id": "toolu_1", "name": "f", "input": {"a": 1}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 1, "output_tokens": 2}
        }))
        .unwrap();
        let message = &out["choices"][0]["message"];
        assert_eq!(message["content"], "Checking.");
        assert_eq!(
            message["tool_calls"][0],
            json!({"id": "toolu_1", "type": "function", "function": {"name": "f", "arguments": "{\"a\":1}"}})
        );
        assert_eq!(out["choices"][0]["finish_reason"], "tool_calls");
    }

    #[test]
    fn am_stop_reasons_map_to_finish_reasons() {
        for (stop, finish) in [
            ("end_turn", "stop"),
            ("stop_sequence", "stop"),
            ("max_tokens", "length"),
            ("tool_use", "tool_calls"),
        ] {
            let out = am_to_cc(json!({
                "id": "msg_1", "model": "m", "content": [{"type": "text", "text": "x"}],
                "stop_reason": stop, "usage": {"input_tokens": 0, "output_tokens": 0}
            }))
            .unwrap();
            assert_eq!(out["choices"][0]["finish_reason"], finish, "{stop}");
        }
    }

    #[test]
    fn cc_to_am_text_tool_calls_and_usage() {
        let out = cc_to_am(json!({
            "id": "chatcmpl_9", "object": "chat.completion", "model": "gpt-5",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": null, "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "f", "arguments": "{\"a\":1}"}}
                ]},
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 20, "completion_tokens": 7, "total_tokens": 27, "prompt_tokens_details": {"cached_tokens": 5}}
        }))
        .unwrap();
        assert_eq!(out["id"], "msg_9");
        assert_eq!(out["type"], "message");
        assert_eq!(out["role"], "assistant");
        assert_eq!(
            out["content"][0],
            json!({"type": "tool_use", "id": "call_1", "name": "f", "input": {"a": 1}})
        );
        assert_eq!(out["stop_reason"], "tool_use");
        assert_eq!(out["usage"]["input_tokens"], 15);
        assert_eq!(out["usage"]["output_tokens"], 7);
        assert_eq!(out["usage"]["cache_read_input_tokens"], 5);
    }

    #[test]
    fn cc_finish_reasons_map_to_stop_reasons() {
        for (finish, stop) in [
            ("stop", "end_turn"),
            ("length", "max_tokens"),
            ("tool_calls", "tool_use"),
            ("content_filter", "end_turn"),
        ] {
            let out = cc_to_am(json!({
                "id": "chatcmpl_1", "model": "m",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "x"}, "finish_reason": finish}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}
            }))
            .unwrap();
            assert_eq!(out["stop_reason"], stop, "{finish}");
        }
    }

    #[test]
    fn am_to_rs_output_and_incomplete_status() {
        let out = am_to_rs(json!({
            "id": "msg_1", "model": "claude-sonnet-4-5",
            "content": [
                {"type": "text", "text": "Partial."},
                {"type": "tool_use", "id": "toolu_1", "name": "f", "input": {"a": 1}}
            ],
            "stop_reason": "max_tokens",
            "usage": {"input_tokens": 4, "output_tokens": 6}
        }))
        .unwrap();
        assert_eq!(out["id"], "resp_1");
        assert_eq!(out["status"], "incomplete");
        assert_eq!(out["incomplete_details"]["reason"], "max_output_tokens");
        assert_eq!(out["output"][0]["type"], "message");
        assert_eq!(
            out["output"][0]["content"][0],
            json!({"type": "output_text", "text": "Partial."})
        );
        assert_eq!(
            out["output"][1],
            json!({"type": "function_call", "id": "fc_1", "call_id": "toolu_1", "name": "f", "arguments": "{\"a\":1}"})
        );
        assert_eq!(out["usage"]["input_tokens"], 4);
        assert_eq!(out["usage"]["output_tokens"], 6);
    }

    #[test]
    fn rs_to_am_output_items_and_usage() {
        let out = rs_to_am(json!({
            "id": "resp_1", "status": "completed", "model": "gpt-5",
            "output": [
                {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Done."}]},
                {"type": "function_call", "call_id": "call_1", "name": "f", "arguments": "{\"a\":1}"},
                {"type": "reasoning", "summary": []}
            ],
            "usage": {"input_tokens": 12, "output_tokens": 3, "input_tokens_details": {"cached_tokens": 2}}
        }))
        .unwrap();
        assert_eq!(out["id"], "msg_1");
        assert_eq!(out["content"][0], json!({"type": "text", "text": "Done."}));
        assert_eq!(
            out["content"][1],
            json!({"type": "tool_use", "id": "call_1", "name": "f", "input": {"a": 1}})
        );
        assert_eq!(out["stop_reason"], "tool_use");
        assert_eq!(out["usage"]["input_tokens"], 10);
        assert_eq!(out["usage"]["cache_read_input_tokens"], 2);
    }

    #[test]
    fn rs_incomplete_maps_to_max_tokens() {
        let out = rs_to_am(json!({
            "id": "resp_1", "status": "incomplete", "model": "gpt-5",
            "output": [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "..."}]}],
            "usage": {"input_tokens": 1, "output_tokens": 100}
        }))
        .unwrap();
        assert_eq!(out["stop_reason"], "max_tokens");
    }

    #[test]
    fn am_response_round_trips_through_cc() {
        let response = json!({
            "id": "msg_1", "model": "claude-sonnet-4-5",
            "content": [
                {"type": "text", "text": "Hi"},
                {"type": "tool_use", "id": "toolu_1", "name": "f", "input": {"a": 1}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 10, "output_tokens": 5, "cache_read_input_tokens": 3, "cache_creation_input_tokens": 2}
        });
        let back = cc_to_am(am_to_cc(response.clone()).unwrap()).unwrap();
        assert_eq!(back["content"][0], json!({"type": "text", "text": "Hi"}));
        assert_eq!(
            back["content"][1],
            json!({"type": "tool_use", "id": "toolu_1", "name": "f", "input": {"a": 1}})
        );
        assert_eq!(back["stop_reason"], "tool_use");
        assert_eq!(back["usage"], response["usage"]);
    }

    #[test]
    fn malformed_responses_are_invalid_payload() {
        assert!(matches!(
            am_to_cc(json!([])),
            Err(ConversionError::InvalidPayload { .. })
        ));
        assert!(matches!(
            cc_to_am(json!({"id": "x"})),
            Err(ConversionError::InvalidPayload { .. })
        ));
        assert!(matches!(
            rs_to_am(json!({"id": "x"})),
            Err(ConversionError::InvalidPayload { .. })
        ));
        assert!(matches!(
            cc_to_am(
                json!({"choices": [{"message": {"tool_calls": [{"id": "c", "function": {"name": "f", "arguments": "bad"}}]}, "finish_reason": "tool_calls"}]})
            ),
            Err(ConversionError::InvalidPayload { .. })
        ));
    }
}
