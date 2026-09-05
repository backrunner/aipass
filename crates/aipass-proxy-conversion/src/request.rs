//! Request conversion between Anthropic Messages (AM) and the OpenAI wire
//! formats (CC = Chat Completions, RS = Responses). AM-only fields
//! (`thinking`, `cache_control`, `metadata`) are dropped; OpenAI-only
//! fields are dropped in reverse.

use serde_json::{json, Map, Value};

use crate::{invalid, ConversionError, ProxyProtocol};

use ProxyProtocol::{AnthropicMessages as AM, OpenAiChatCompletions as CC, OpenAiResponses as RS};

/// AM requires `max_tokens`; OpenAI requests may omit it.
pub(crate) const DEFAULT_MAX_TOKENS: u64 = 4096;

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
        .ok_or_else(|| invalid(protocol, "request must be a JSON object"))
}

fn pass(src: &Map<String, Value>, dst: &mut Map<String, Value>, keys: &[&str]) {
    for key in keys {
        if let Some(value) = src.get(*key) {
            dst.insert((*key).to_string(), value.clone());
        }
    }
}

fn json_string(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn block_type(block: &Value) -> &str {
    block.get("type").and_then(Value::as_str).unwrap_or("")
}

/// AM `system` is a string or an array of text blocks.
fn am_system_text(system: Option<&Value>) -> Option<String> {
    match system {
        Some(Value::String(text)) => Some(text.clone()),
        Some(Value::Array(blocks)) => {
            let text = blocks
                .iter()
                .filter(|block| block_type(block) == "text")
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
        _ => None,
    }
}

fn am_image_url(block: &Value) -> Option<String> {
    let source = block.get("source")?;
    match source.get("type").and_then(Value::as_str)? {
        "base64" => Some(format!(
            "data:{};base64,{}",
            source.get("media_type")?.as_str()?,
            source.get("data")?.as_str()?
        )),
        "url" => source
            .get("url")
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

/// AM `tool_result.content` is a string or an array of content blocks.
fn am_tool_result_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|block| block_type(block) == "text")
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        other => json_string(other),
    }
}

/// CC message content: collapse pure-text part lists into a single string,
/// keep mixed part lists (text + images) as an array.
fn cc_content(parts: &[Value]) -> Value {
    let all_text = parts
        .iter()
        .all(|part| part.get("type").and_then(Value::as_str) == Some("text"));
    if all_text {
        Value::String(
            parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    } else {
        Value::Array(parts.to_vec())
    }
}

fn am_tool_choice_to_cc(choice: &Value) -> Value {
    match choice.get("type").and_then(Value::as_str) {
        Some("any") => json!("required"),
        Some("none") => json!("none"),
        Some("tool") => {
            json!({"type": "function", "function": {"name": choice.get("name").cloned().unwrap_or(Value::Null)}})
        }
        _ => json!("auto"),
    }
}

fn am_tool_choice_to_rs(choice: &Value) -> Value {
    match choice.get("type").and_then(Value::as_str) {
        Some("any") => json!("required"),
        Some("none") => json!("none"),
        Some("tool") => {
            json!({"type": "function", "name": choice.get("name").cloned().unwrap_or(Value::Null)})
        }
        _ => json!("auto"),
    }
}

// --- AM -> CC -------------------------------------------------------------

fn am_to_cc(payload: Value) -> Result<Value, ConversionError> {
    let src = object(&payload, AM)?;
    let mut out = Map::new();
    pass(src, &mut out, &["model", "stream", "temperature", "top_p"]);
    if let Some(value) = src.get("max_tokens") {
        out.insert("max_tokens".into(), value.clone());
    }
    if let Some(value) = src.get("stop_sequences") {
        out.insert("stop".into(), value.clone());
    }

    let mut messages = Vec::new();
    if let Some(system) = am_system_text(src.get("system")) {
        messages.push(json!({"role": "system", "content": system}));
    }
    let am_messages = src
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(AM, "missing messages array"))?;
    for message in am_messages {
        am_message_to_cc(message, &mut messages);
    }
    out.insert("messages".into(), Value::Array(messages));

    if let Some(tools) = src.get("tools").and_then(Value::as_array) {
        out.insert(
            "tools".into(),
            Value::Array(
                tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": tool.get("name").cloned().unwrap_or(Value::Null),
                                "description": tool.get("description").cloned().unwrap_or(Value::Null),
                                "parameters": tool.get("input_schema").cloned().unwrap_or_else(|| json!({"type": "object"})),
                            }
                        })
                    })
                    .collect(),
            ),
        );
    }
    if let Some(choice) = src.get("tool_choice") {
        out.insert("tool_choice".into(), am_tool_choice_to_cc(choice));
    }
    Ok(Value::Object(out))
}

fn am_message_to_cc(message: &Value, out: &mut Vec<Value>) {
    let role = message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("user");
    match message.get("content") {
        Some(Value::String(text)) => out.push(json!({"role": role, "content": text})),
        Some(Value::Array(blocks)) => {
            let mut parts: Vec<Value> = Vec::new();
            let mut tool_calls: Vec<Value> = Vec::new();
            let mut tool_results: Vec<Value> = Vec::new();
            for block in blocks {
                match block_type(block) {
                    "text" => {
                        if let Some(text) = block.get("text") {
                            parts.push(json!({"type": "text", "text": text}));
                        }
                    }
                    "image" => {
                        if let Some(url) = am_image_url(block) {
                            parts.push(json!({"type": "image_url", "image_url": {"url": url}}));
                        }
                    }
                    "tool_use" => tool_calls.push(json!({
                        "id": block.get("id").cloned().unwrap_or(Value::Null),
                        "type": "function",
                        "function": {
                            "name": block.get("name").cloned().unwrap_or(Value::Null),
                            "arguments": json_string(block.get("input").unwrap_or(&Value::Null)),
                        }
                    })),
                    "tool_result" => tool_results.push(json!({
                        "role": "tool",
                        "tool_call_id": block.get("tool_use_id").cloned().unwrap_or(Value::Null),
                        "content": am_tool_result_text(block.get("content").unwrap_or(&Value::Null)),
                    })),
                    _ => {}
                }
            }
            if role == "assistant" {
                if parts.is_empty() && tool_calls.is_empty() {
                    return;
                }
                let mut msg = Map::new();
                msg.insert("role".into(), json!("assistant"));
                msg.insert(
                    "content".into(),
                    if parts.is_empty() {
                        Value::Null
                    } else {
                        cc_content(&parts)
                    },
                );
                if !tool_calls.is_empty() {
                    msg.insert("tool_calls".into(), Value::Array(tool_calls));
                }
                out.push(Value::Object(msg));
            } else {
                if !parts.is_empty() {
                    out.push(json!({"role": role, "content": cc_content(&parts)}));
                }
                if !tool_calls.is_empty() {
                    out.push(json!({"role": "assistant", "content": Value::Null, "tool_calls": tool_calls}));
                }
            }
            out.extend(tool_results);
        }
        _ => {}
    }
}

// --- AM -> RS -------------------------------------------------------------

fn am_to_rs(payload: Value) -> Result<Value, ConversionError> {
    let src = object(&payload, AM)?;
    let mut out = Map::new();
    pass(src, &mut out, &["model", "stream", "temperature", "top_p"]);
    if let Some(system) = am_system_text(src.get("system")) {
        out.insert("instructions".into(), json!(system));
    }
    if let Some(value) = src.get("max_tokens") {
        out.insert("max_output_tokens".into(), value.clone());
    }

    let am_messages = src
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(AM, "missing messages array"))?;
    let mut input = Vec::new();
    for message in am_messages {
        am_message_to_rs(message, &mut input);
    }
    out.insert("input".into(), Value::Array(input));

    if let Some(tools) = src.get("tools").and_then(Value::as_array) {
        out.insert(
            "tools".into(),
            Value::Array(
                tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "type": "function",
                            "name": tool.get("name").cloned().unwrap_or(Value::Null),
                            "description": tool.get("description").cloned().unwrap_or(Value::Null),
                            "parameters": tool.get("input_schema").cloned().unwrap_or_else(|| json!({"type": "object"})),
                        })
                    })
                    .collect(),
            ),
        );
    }
    if let Some(choice) = src.get("tool_choice") {
        out.insert("tool_choice".into(), am_tool_choice_to_rs(choice));
    }
    Ok(Value::Object(out))
}

fn am_message_to_rs(message: &Value, out: &mut Vec<Value>) {
    let role = message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("user");
    let assistant = role == "assistant";
    let text_type = if assistant {
        "output_text"
    } else {
        "input_text"
    };
    match message.get("content") {
        Some(Value::String(text)) => out.push(
            json!({"type": "message", "role": role, "content": [{"type": text_type, "text": text}]}),
        ),
        Some(Value::Array(blocks)) => {
            let mut parts: Vec<Value> = Vec::new();
            let mut calls: Vec<Value> = Vec::new();
            for block in blocks {
                match block_type(block) {
                    "text" => {
                        if let Some(text) = block.get("text") {
                            parts.push(json!({"type": text_type, "text": text}));
                        }
                    }
                    "image" => {
                        if let Some(url) = am_image_url(block) {
                            parts.push(json!({"type": "input_image", "image_url": url}));
                        }
                    }
                    "tool_use" => calls.push(json!({
                        "type": "function_call",
                        "call_id": block.get("id").cloned().unwrap_or(Value::Null),
                        "name": block.get("name").cloned().unwrap_or(Value::Null),
                        "arguments": json_string(block.get("input").unwrap_or(&Value::Null)),
                    })),
                    "tool_result" => calls.push(json!({
                        "type": "function_call_output",
                        "call_id": block.get("tool_use_id").cloned().unwrap_or(Value::Null),
                        "output": am_tool_result_text(block.get("content").unwrap_or(&Value::Null)),
                    })),
                    _ => {}
                }
            }
            if !parts.is_empty() {
                out.push(json!({"type": "message", "role": role, "content": parts}));
            }
            out.extend(calls);
        }
        _ => {}
    }
}

// --- CC -> AM -------------------------------------------------------------

fn cc_to_am(payload: Value) -> Result<Value, ConversionError> {
    let src = object(&payload, CC)?;
    let mut out = Map::new();
    pass(src, &mut out, &["model", "stream", "temperature", "top_p"]);
    let max_tokens = src
        .get("max_tokens")
        .or_else(|| src.get("max_completion_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_MAX_TOKENS);
    out.insert("max_tokens".into(), json!(max_tokens));
    if let Some(stop) = src.get("stop") {
        let sequences = match stop {
            Value::String(text) => json!([text]),
            other => other.clone(),
        };
        out.insert("stop_sequences".into(), sequences);
    }

    let cc_messages = src
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(CC, "missing messages array"))?;
    let mut system_parts: Vec<String> = Vec::new();
    let mut messages: Vec<(String, Vec<Value>)> = Vec::new();
    let mut generated_ids = 0u64;
    for message in cc_messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        match role {
            "system" | "developer" => {
                if let Some(text) = cc_content_text(message.get("content")) {
                    if !text.is_empty() {
                        system_parts.push(text);
                    }
                }
            }
            "assistant" => {
                let mut blocks = Vec::new();
                if let Some(text) = cc_content_text(message.get("content")) {
                    if !text.is_empty() {
                        blocks.push(json!({"type": "text", "text": text}));
                    }
                }
                if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
                    for call in tool_calls {
                        let function = call.get("function").cloned().unwrap_or(Value::Null);
                        let arguments = function
                            .get("arguments")
                            .and_then(Value::as_str)
                            .unwrap_or("{}");
                        let input: Value = serde_json::from_str(arguments).map_err(|err| {
                            invalid(CC, format!("tool_call arguments are not valid JSON: {err}"))
                        })?;
                        let id = call
                            .get("id")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .unwrap_or_else(|| {
                                generated_ids += 1;
                                format!("toolu_conv_{generated_ids}")
                            });
                        blocks.push(json!({
                            "type": "tool_use",
                            "id": id,
                            "name": function.get("name").cloned().unwrap_or(Value::Null),
                            "input": input,
                        }));
                    }
                }
                if !blocks.is_empty() {
                    messages.push(("assistant".to_string(), blocks));
                }
            }
            "tool" => {
                messages.push((
                    "user".to_string(),
                    vec![json!({
                        "type": "tool_result",
                        "tool_use_id": message.get("tool_call_id").cloned().unwrap_or(Value::Null),
                        "content": cc_content_text(message.get("content")).unwrap_or_default(),
                    })],
                ));
            }
            _ => messages.push((
                "user".to_string(),
                cc_parts_to_am_blocks(message.get("content")),
            )),
        }
    }
    if !system_parts.is_empty() {
        out.insert("system".into(), json!(system_parts.join("\n")));
    }
    out.insert("messages".into(), Value::Array(merge_consecutive(messages)));

    let mut tools = Vec::new();
    if let Some(cc_tools) = src.get("tools").and_then(Value::as_array) {
        for tool in cc_tools {
            let function = tool.get("function").cloned().unwrap_or(tool.clone());
            tools.push(cc_function_to_am_tool(&function));
        }
    }
    if let Some(functions) = src.get("functions").and_then(Value::as_array) {
        for function in functions {
            tools.push(cc_function_to_am_tool(function));
        }
    }
    if !tools.is_empty() {
        out.insert("tools".into(), Value::Array(tools));
    }
    if let Some(choice) = src.get("tool_choice") {
        out.insert("tool_choice".into(), cc_tool_choice_to_am(choice));
    }
    Ok(Value::Object(out))
}

/// CC content is a string or an array of parts; extract plain text.
fn cc_content_text(content: Option<&Value>) -> Option<String> {
    match content {
        Some(Value::String(text)) => Some(text.clone()),
        Some(Value::Array(parts)) => Some(
            parts
                .iter()
                .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        _ => None,
    }
}

fn cc_parts_to_am_blocks(content: Option<&Value>) -> Vec<Value> {
    match content {
        Some(Value::String(text)) => vec![json!({"type": "text", "text": text})],
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| match part.get("type").and_then(Value::as_str) {
                Some("text") => Some(json!({"type": "text", "text": part.get("text").cloned().unwrap_or(Value::Null)})),
                Some("image_url") => {
                    let url = part
                        .pointer("/image_url/url")
                        .and_then(Value::as_str)?;
                    Some(json!({"type": "image", "source": data_url_to_am_source(url)}))
                }
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn data_url_to_am_source(url: &str) -> Value {
    if let Some(rest) = url.strip_prefix("data:") {
        if let Some((media_type, data)) = rest.split_once(";base64,") {
            return json!({"type": "base64", "media_type": media_type, "data": data});
        }
    }
    json!({"type": "url", "url": url})
}

fn cc_function_to_am_tool(function: &Value) -> Value {
    json!({
        "name": function.get("name").cloned().unwrap_or(Value::Null),
        "description": function.get("description").cloned().unwrap_or(Value::Null),
        "input_schema": function.get("parameters").cloned().unwrap_or_else(|| json!({"type": "object"})),
    })
}

fn cc_tool_choice_to_am(choice: &Value) -> Value {
    match choice {
        Value::String(text) => match text.as_str() {
            "required" => json!({"type": "any"}),
            "none" => json!({"type": "none"}),
            _ => json!({"type": "auto"}),
        },
        Value::Object(_) => {
            let name = choice
                .pointer("/function/name")
                .cloned()
                .unwrap_or(Value::Null);
            json!({"type": "tool", "name": name})
        }
        _ => json!({"type": "auto"}),
    }
}

/// AM requires strictly alternating roles; fold runs of same-role messages
/// into one message with concatenated content blocks.
fn merge_consecutive(messages: Vec<(String, Vec<Value>)>) -> Vec<Value> {
    let mut merged: Vec<(String, Vec<Value>)> = Vec::new();
    for (role, blocks) in messages {
        if let Some(last) = merged.last_mut() {
            if last.0 == role {
                last.1.extend(blocks);
                continue;
            }
        }
        merged.push((role, blocks));
    }
    merged
        .into_iter()
        .filter(|(_, blocks)| !blocks.is_empty())
        .map(|(role, blocks)| json!({"role": role, "content": blocks}))
        .collect()
}

// --- RS -> AM -------------------------------------------------------------

fn rs_to_am(payload: Value) -> Result<Value, ConversionError> {
    let src = object(&payload, RS)?;
    let mut out = Map::new();
    pass(src, &mut out, &["model", "stream", "temperature", "top_p"]);
    let mut system_parts = Vec::new();
    if let Some(instructions) = src.get("instructions").and_then(Value::as_str) {
        if !instructions.is_empty() {
            system_parts.push(instructions.to_owned());
        }
    }
    let max_tokens = src
        .get("max_output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_MAX_TOKENS);
    out.insert("max_tokens".into(), json!(max_tokens));

    let mut messages: Vec<(String, Vec<Value>)> = Vec::new();
    match src.get("input") {
        Some(Value::String(text)) => {
            messages.push((
                "user".to_string(),
                vec![json!({"type": "text", "text": text})],
            ));
        }
        Some(Value::Array(items)) => {
            for item in items {
                if matches!(
                    item.get("role").and_then(Value::as_str),
                    Some("system" | "developer")
                ) {
                    for block in rs_message_content_to_am(item.get("content")) {
                        if let Some(text) = block.get("text").and_then(Value::as_str) {
                            system_parts.push(text.to_owned());
                        }
                    }
                } else {
                    rs_item_to_am(item, &mut messages)?;
                }
            }
        }
        None => return Err(invalid(RS, "missing input")),
        _ => return Err(invalid(RS, "input must be a string or an array")),
    }
    out.insert("messages".into(), Value::Array(merge_consecutive(messages)));

    if !system_parts.is_empty() {
        out.insert("system".into(), json!(system_parts.join("\n")));
    }

    if let Some(tools) = src.get("tools").and_then(Value::as_array) {
        let am_tools: Vec<Value> = tools
            .iter()
            .filter(|tool| tool.get("type").and_then(Value::as_str) == Some("function"))
            .map(cc_function_to_am_tool)
            .collect();
        if !am_tools.is_empty() {
            out.insert("tools".into(), Value::Array(am_tools));
        }
    }
    if let Some(choice) = src.get("tool_choice") {
        let am_choice = match choice {
            Value::String(_) => cc_tool_choice_to_am(choice),
            Value::Object(_) => json!({
                "type": "tool",
                "name": choice.get("name").cloned().unwrap_or(Value::Null),
            }),
            _ => json!({"type": "auto"}),
        };
        out.insert("tool_choice".into(), am_choice);
    }
    Ok(Value::Object(out))
}

fn rs_item_to_am(
    item: &Value,
    messages: &mut Vec<(String, Vec<Value>)>,
) -> Result<(), ConversionError> {
    match item.get("type").and_then(Value::as_str) {
        Some("function_call") => {
            let arguments = item
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}");
            let input: Value = serde_json::from_str(arguments).map_err(|err| {
                invalid(
                    RS,
                    format!("function_call arguments are not valid JSON: {err}"),
                )
            })?;
            messages.push((
                "assistant".to_string(),
                vec![json!({
                    "type": "tool_use",
                    "id": item.get("call_id").or_else(|| item.get("id")).cloned().unwrap_or(Value::Null),
                    "name": item.get("name").cloned().unwrap_or(Value::Null),
                    "input": input,
                })],
            ));
        }
        Some("function_call_output") => {
            let content = match item.get("output") {
                Some(Value::String(text)) => text.clone(),
                Some(other) => json_string(other),
                None => String::new(),
            };
            messages.push((
                "user".to_string(),
                vec![json!({
                    "type": "tool_result",
                    "tool_use_id": item.get("call_id").cloned().unwrap_or(Value::Null),
                    "content": content,
                })],
            ));
        }
        Some("message") | None if item.get("role").is_some() => {
            let role = item.get("role").and_then(Value::as_str).unwrap_or("user");
            let role = if role == "assistant" {
                "assistant"
            } else {
                "user"
            };
            let blocks = rs_message_content_to_am(item.get("content"));
            messages.push((role.to_string(), blocks));
        }
        _ => {}
    }
    Ok(())
}

fn rs_message_content_to_am(content: Option<&Value>) -> Vec<Value> {
    match content {
        Some(Value::String(text)) => vec![json!({"type": "text", "text": text})],
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| match part.get("type").and_then(Value::as_str) {
                Some("input_text") | Some("output_text") => {
                    Some(json!({"type": "text", "text": part.get("text").cloned().unwrap_or(Value::Null)}))
                }
                Some("input_image") => part
                    .get("image_url")
                    .and_then(Value::as_str)
                    .map(|url| json!({"type": "image", "source": data_url_to_am_source(url)})),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn am_request() -> Value {
        json!({
            "model": "claude-sonnet-4-5",
            "system": [{"type": "text", "text": "You are helpful."}],
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "What is in this image?"},
                    {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "aGVsbG8="}}
                ]},
                {"role": "assistant", "content": [
                    {"type": "text", "text": "Let me look."},
                    {"type": "tool_use", "id": "toolu_1", "name": "describe", "input": {"detail": "high"}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "toolu_1", "content": [{"type": "text", "text": "A cat."}]}
                ]}
            ],
            "tools": [{"name": "describe", "description": "Describe an image", "input_schema": {"type": "object", "properties": {"detail": {"type": "string"}}}}],
            "tool_choice": {"type": "tool", "name": "describe"},
            "max_tokens": 1024,
            "stop_sequences": ["END"],
            "temperature": 0.5,
            "top_p": 0.9,
            "stream": true,
            "thinking": {"type": "enabled", "budget_tokens": 1000},
            "metadata": {"user_id": "u1"}
        })
    }

    #[test]
    fn responses_system_and_developer_items_remain_system_instructions() {
        let out = rs_to_am(json!({
            "model":"m", "instructions":"top-level",
            "input":[
                {"role":"system","content":"system rule"},
                {"type":"message","role":"developer","content":[{"type":"input_text","text":"developer rule"}]},
                {"role":"user","content":"hello"}
            ]
        })).unwrap();
        assert_eq!(out["system"], "top-level\nsystem rule\ndeveloper rule");
        assert_eq!(
            out["messages"],
            json!([{ "role":"user", "content":[{"type":"text","text":"hello"}] }])
        );
    }

    #[test]
    fn am_to_cc_full_request() {
        let out = am_to_cc(am_request()).unwrap();
        assert_eq!(out["model"], "claude-sonnet-4-5");
        assert_eq!(out["max_tokens"], 1024);
        assert_eq!(out["stop"], json!(["END"]));
        assert_eq!(out["temperature"], 0.5);
        assert_eq!(out["top_p"], 0.9);
        assert_eq!(out["stream"], true);
        assert!(out.get("thinking").is_none());
        assert!(out.get("metadata").is_none());

        let messages = out["messages"].as_array().unwrap();
        assert_eq!(
            messages[0],
            json!({"role": "system", "content": "You are helpful."})
        );
        assert_eq!(
            messages[1],
            json!({"role": "user", "content": [
                {"type": "text", "text": "What is in this image?"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,aGVsbG8="}}
            ]})
        );
        assert_eq!(
            messages[2],
            json!({"role": "assistant", "content": "Let me look.", "tool_calls": [
                {"id": "toolu_1", "type": "function", "function": {"name": "describe", "arguments": "{\"detail\":\"high\"}"}}
            ]})
        );
        assert_eq!(
            messages[3],
            json!({"role": "tool", "tool_call_id": "toolu_1", "content": "A cat."})
        );
        assert_eq!(
            out["tools"],
            json!([{"type": "function", "function": {"name": "describe", "description": "Describe an image", "parameters": {"type": "object", "properties": {"detail": {"type": "string"}}}}}])
        );
        assert_eq!(
            out["tool_choice"],
            json!({"type": "function", "function": {"name": "describe"}})
        );
    }

    #[test]
    fn am_to_cc_tool_choice_variants_and_string_system() {
        let base =
            json!({"model": "m", "system": "sys", "messages": [{"role": "user", "content": "hi"}]});
        let mut with_auto = base.clone();
        with_auto["tool_choice"] = json!({"type": "auto"});
        let out = am_to_cc(with_auto).unwrap();
        assert_eq!(out["tool_choice"], json!("auto"));
        assert_eq!(
            out["messages"][0],
            json!({"role": "system", "content": "sys"})
        );

        let mut with_any = base.clone();
        with_any["tool_choice"] = json!({"type": "any"});
        assert_eq!(
            am_to_cc(with_any).unwrap()["tool_choice"],
            json!("required")
        );

        let mut with_none = base;
        with_none["tool_choice"] = json!({"type": "none"});
        assert_eq!(am_to_cc(with_none).unwrap()["tool_choice"], json!("none"));
    }

    #[test]
    fn am_to_rs_full_request() {
        let out = am_to_rs(am_request()).unwrap();
        assert_eq!(out["instructions"], "You are helpful.");
        assert_eq!(out["max_output_tokens"], 1024);
        assert_eq!(out["model"], "claude-sonnet-4-5");
        assert_eq!(out["stream"], true);
        assert!(out.get("thinking").is_none());

        let input = out["input"].as_array().unwrap();
        assert_eq!(
            input[0],
            json!({"type": "message", "role": "user", "content": [
                {"type": "input_text", "text": "What is in this image?"},
                {"type": "input_image", "image_url": "data:image/png;base64,aGVsbG8="}
            ]})
        );
        assert_eq!(
            input[1],
            json!({"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Let me look."}]})
        );
        assert_eq!(
            input[2],
            json!({"type": "function_call", "call_id": "toolu_1", "name": "describe", "arguments": "{\"detail\":\"high\"}"})
        );
        assert_eq!(
            input[3],
            json!({"type": "function_call_output", "call_id": "toolu_1", "output": "A cat."})
        );
        assert_eq!(
            out["tools"],
            json!([{"type": "function", "name": "describe", "description": "Describe an image", "parameters": {"type": "object", "properties": {"detail": {"type": "string"}}}}])
        );
        assert_eq!(
            out["tool_choice"],
            json!({"type": "function", "name": "describe"})
        );
    }

    #[test]
    fn cc_to_am_full_request() {
        let request = json!({
            "model": "gpt-5",
            "messages": [
                {"role": "system", "content": "Be terse."},
                {"role": "user", "content": [
                    {"type": "text", "text": "Look:"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,aGVsbG8="}}
                ]},
                {"role": "assistant", "content": null, "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "describe", "arguments": "{\"detail\":\"high\"}"}}
                ]},
                {"role": "tool", "tool_call_id": "call_1", "content": "A cat."},
                {"role": "tool", "tool_call_id": "call_2", "content": "A dog."}
            ],
            "tools": [{"type": "function", "function": {"name": "describe", "description": "d", "parameters": {"type": "object"}}}],
            "tool_choice": "required",
            "stop": "END",
            "temperature": 0.2
        });
        let out = cc_to_am(request).unwrap();
        assert_eq!(out["model"], "gpt-5");
        assert_eq!(out["system"], "Be terse.");
        assert_eq!(out["max_tokens"], DEFAULT_MAX_TOKENS);
        assert_eq!(out["stop_sequences"], json!(["END"]));
        assert_eq!(out["temperature"], 0.2);

        let messages = out["messages"].as_array().unwrap();
        assert_eq!(
            messages[0],
            json!({"role": "user", "content": [
                {"type": "text", "text": "Look:"},
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "aGVsbG8="}}
            ]})
        );
        assert_eq!(
            messages[1],
            json!({"role": "assistant", "content": [
                {"type": "tool_use", "id": "call_1", "name": "describe", "input": {"detail": "high"}}
            ]})
        );
        // consecutive tool messages merge into one user message
        assert_eq!(
            messages[2],
            json!({"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "call_1", "content": "A cat."},
                {"type": "tool_result", "tool_use_id": "call_2", "content": "A dog."}
            ]})
        );
        assert_eq!(
            out["tools"],
            json!([{"name": "describe", "description": "d", "input_schema": {"type": "object"}}])
        );
        assert_eq!(out["tool_choice"], json!({"type": "any"}));
    }

    #[test]
    fn cc_to_am_preserves_max_tokens_and_named_tool_choice() {
        let request = json!({
            "messages": [{"role": "user", "content": "hi"}],
            "max_completion_tokens": 512,
            "tool_choice": {"type": "function", "function": {"name": "f"}}
        });
        let out = cc_to_am(request).unwrap();
        assert_eq!(out["max_tokens"], 512);
        assert_eq!(out["tool_choice"], json!({"type": "tool", "name": "f"}));
    }

    #[test]
    fn rs_to_am_full_request() {
        let request = json!({
            "model": "gpt-5",
            "instructions": "Be terse.",
            "input": [
                {"type": "message", "role": "user", "content": [
                    {"type": "input_text", "text": "Look:"},
                    {"type": "input_image", "image_url": "data:image/png;base64,aGVsbG8="}
                ]},
                {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "One moment."}]},
                {"type": "function_call", "call_id": "call_1", "name": "describe", "arguments": "{\"detail\":\"high\"}"},
                {"type": "function_call_output", "call_id": "call_1", "output": "A cat."},
                {"type": "reasoning", "summary": []}
            ],
            "tools": [{"type": "function", "name": "describe", "description": "d", "parameters": {"type": "object"}}],
            "max_output_tokens": 256
        });
        let out = rs_to_am(request).unwrap();
        assert_eq!(out["system"], "Be terse.");
        assert_eq!(out["max_tokens"], 256);

        let messages = out["messages"].as_array().unwrap();
        assert_eq!(
            messages[0],
            json!({"role": "user", "content": [
                {"type": "text", "text": "Look:"},
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "aGVsbG8="}}
            ]})
        );
        assert_eq!(
            messages[1],
            json!({"role": "assistant", "content": [
                {"type": "text", "text": "One moment."},
                {"type": "tool_use", "id": "call_1", "name": "describe", "input": {"detail": "high"}}
            ]})
        );
        assert_eq!(
            messages[2],
            json!({"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "call_1", "content": "A cat."}
            ]})
        );
        assert_eq!(
            out["tools"],
            json!([{"name": "describe", "description": "d", "input_schema": {"type": "object"}}])
        );
    }

    #[test]
    fn rs_to_am_string_input_and_default_max_tokens() {
        let out = rs_to_am(json!({"model": "gpt-5", "input": "hello"})).unwrap();
        assert_eq!(out["max_tokens"], DEFAULT_MAX_TOKENS);
        assert_eq!(
            out["messages"],
            json!([{"role": "user", "content": [{"type": "text", "text": "hello"}]}])
        );
    }

    #[test]
    fn am_cc_am_round_trip_preserves_text_and_tools() {
        let request = json!({
            "model": "claude-sonnet-4-5",
            "system": "You are helpful.",
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "Hi"}]},
                {"role": "assistant", "content": [
                    {"type": "text", "text": "Working."},
                    {"type": "tool_use", "id": "toolu_1", "name": "f", "input": {"a": 1}}
                ]},
                {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "content": "done"}]}
            ],
            "tools": [{"name": "f", "description": "d", "input_schema": {"type": "object", "properties": {"a": {"type": "number"}}}}],
            "max_tokens": 100
        });
        let back = cc_to_am(am_to_cc(request).unwrap()).unwrap();
        assert_eq!(back["system"], "You are helpful.");
        let messages = back["messages"].as_array().unwrap();
        assert_eq!(
            messages[0]["content"][0],
            json!({"type": "text", "text": "Hi"})
        );
        assert_eq!(
            messages[1]["content"][1],
            json!({"type": "tool_use", "id": "toolu_1", "name": "f", "input": {"a": 1}})
        );
        assert_eq!(
            messages[2]["content"][0],
            json!({"type": "tool_result", "tool_use_id": "toolu_1", "content": "done"})
        );
        assert_eq!(
            back["tools"][0]["input_schema"]["properties"]["a"],
            json!({"type": "number"})
        );
        assert_eq!(back["max_tokens"], 100);
    }

    #[test]
    fn am_rs_am_round_trip_preserves_text_and_tools() {
        let request = json!({
            "model": "claude-sonnet-4-5",
            "system": "You are helpful.",
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "Hi"}]},
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "toolu_1", "name": "f", "input": {"a": 1}}
                ]},
                {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "content": "done"}]}
            ],
            "tools": [{"name": "f", "description": "d", "input_schema": {"type": "object"}}],
            "max_tokens": 100
        });
        let back = rs_to_am(am_to_rs(request).unwrap()).unwrap();
        assert_eq!(back["system"], "You are helpful.");
        let messages = back["messages"].as_array().unwrap();
        assert_eq!(
            messages[0]["content"][0],
            json!({"type": "text", "text": "Hi"})
        );
        assert_eq!(
            messages[1]["content"][0],
            json!({"type": "tool_use", "id": "toolu_1", "name": "f", "input": {"a": 1}})
        );
        assert_eq!(
            messages[2]["content"][0],
            json!({"type": "tool_result", "tool_use_id": "toolu_1", "content": "done"})
        );
        assert_eq!(back["max_tokens"], 100);
    }

    #[test]
    fn malformed_requests_are_invalid_payload() {
        assert!(matches!(
            am_to_cc(json!("nope")),
            Err(ConversionError::InvalidPayload { .. })
        ));
        assert!(matches!(
            am_to_cc(json!({"model": "m"})),
            Err(ConversionError::InvalidPayload { .. })
        ));
        assert!(matches!(
            cc_to_am(json!({"model": "m"})),
            Err(ConversionError::InvalidPayload { .. })
        ));
        assert!(matches!(
            cc_to_am(
                json!({"messages": [{"role": "assistant", "tool_calls": [{"function": {"name": "f", "arguments": "{oops"}}]}]})
            ),
            Err(ConversionError::InvalidPayload { .. })
        ));
        assert!(matches!(
            rs_to_am(
                json!({"input": [{"type": "function_call", "call_id": "c", "name": "f", "arguments": "not json"}]})
            ),
            Err(ConversionError::InvalidPayload { .. })
        ));
    }
}
