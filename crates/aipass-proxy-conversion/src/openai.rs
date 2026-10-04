//! Direct Chat Completions / Responses conversion. Keep OpenAI fields and
//! tool identities intact instead of taking a lossy detour through Messages.

use serde_json::{json, Map, Value};

use crate::{invalid, ConversionError, ProxyProtocol};
use ProxyProtocol::{OpenAiChatCompletions as CC, OpenAiResponses as RS};

mod stream;
pub(crate) use stream::{ChatToResponses, ResponsesToChat};

fn object(value: &Value, protocol: ProxyProtocol) -> Result<&Map<String, Value>, ConversionError> {
    value
        .as_object()
        .ok_or_else(|| invalid(protocol, "expected a JSON object"))
}

fn copy(src: &Map<String, Value>, dst: &mut Map<String, Value>, keys: &[&str]) {
    for key in keys {
        if let Some(value) = src.get(*key) {
            dst.insert((*key).into(), value.clone());
        }
    }
}

fn string(value: Option<&Value>) -> &str {
    value.and_then(Value::as_str).unwrap_or_default()
}

fn arguments(value: Option<&Value>) -> Value {
    match value {
        Some(Value::String(_)) => value.unwrap().clone(),
        Some(value) if !value.is_null() => json!(value.to_string()),
        _ => json!("{}"),
    }
}

fn content(value: &Value, to: ProxyProtocol, assistant: bool) -> Result<Value, ConversionError> {
    if value.is_string() || value.is_null() {
        return Ok(value.clone());
    }
    let parts = value
        .as_array()
        .ok_or_else(|| invalid(to, "invalid message content"))?;
    let mut out = Vec::new();
    for part in parts {
        out.push(match (to, string(part.get("type"))) {
            (RS, "text") => json!({"type": if assistant { "output_text" } else { "input_text" }, "text": part["text"]}),
            (RS, "image_url") => {
                let mut image = json!({"type":"input_image", "image_url":part["image_url"]["url"]});
                if let Some(detail) = part.pointer("/image_url/detail") { image["detail"] = detail.clone(); }
                image
            }
            (RS, "file") => {
                let mut file = part.get("file").cloned().unwrap_or(json!({}));
                object(&file, CC)?;
                file["type"] = json!("input_file");
                file
            }
            (CC, "input_text" | "output_text") => json!({"type":"text", "text":part["text"]}),
            (CC, "input_image") if part.get("image_url").is_some() => {
                let mut image = json!({"type":"image_url", "image_url":{"url":part["image_url"]}});
                if let Some(detail) = part.get("detail") { image["image_url"]["detail"] = detail.clone(); }
                image
            }
            (CC, "input_file") => {
                let mut file = part.clone();
                file.as_object_mut().unwrap().remove("type");
                json!({"type":"file", "file":file})
            }
            (_, kind) => return Err(invalid(to, format!("unsupported converted content type: {kind}"))),
        });
    }
    Ok(Value::Array(out))
}

pub(crate) fn request(from: ProxyProtocol, payload: Value) -> Result<Value, ConversionError> {
    let src = object(&payload, from)?;
    let mut out = Map::new();
    copy(
        src,
        &mut out,
        &[
            "model",
            "stream",
            "temperature",
            "top_p",
            "parallel_tool_calls",
            "store",
            "metadata",
            "service_tier",
            "prompt_cache_key",
            "safety_identifier",
            "user",
        ],
    );
    let mut messages = Vec::new();
    if from == CC {
        for field in ["functions", "function_call", "stop", "audio"] {
            if src.get(field).is_some_and(|v| !v.is_null()) {
                return Err(invalid(
                    CC,
                    format!("{field} cannot be represented by this Responses adapter"),
                ));
            }
        }
        if src.get("n").and_then(Value::as_u64).is_some_and(|n| n != 1) {
            return Err(invalid(CC, "Responses conversion requires n=1"));
        }
        if let Some(max) = src
            .get("max_completion_tokens")
            .or_else(|| src.get("max_tokens"))
        {
            out.insert("max_output_tokens".into(), max.clone());
        }
        if let Some(effort) = src.get("reasoning_effort") {
            out.insert("reasoning".into(), json!({"effort":effort}));
        }
        if let Some(format) = src.get("response_format") {
            let format = if format["type"] == "json_schema" {
                let mut schema = format["json_schema"].clone();
                object(&schema, CC)?;
                schema["type"] = json!("json_schema");
                schema
            } else {
                format.clone()
            };
            out.insert("text".into(), json!({"format":format}));
        }
        let input = src
            .get("messages")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid(CC, "missing messages array"))?;
        for message in input {
            let role = string(message.get("role"));
            match role {
                "tool" => messages.push(json!({"type":"function_call_output", "call_id":message["tool_call_id"], "output":content(&message["content"], RS, false)?})),
                "system" | "developer" | "user" | "assistant" => {
                    let reasoning=crate::reasoning::chat_reasoning(message);
                    if !reasoning.is_empty() {messages.push(json!({"type":"reasoning","summary":[{"type":"summary_text","text":reasoning}]}));}
                    let mut clean=message.get("content").cloned().unwrap_or(Value::Null);
                    if let Some(parts)=clean.as_array_mut(){parts.retain(|p|p["type"]!="thinking");}
                    if let Some(body) = Some(&clean).filter(|v| !v.is_null()) {
                        messages.push(json!({"type":"message", "role":role, "content":content(body, RS, role == "assistant")?}));
                    }
                    if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
                        for call in calls { messages.push(call_to_responses(call)?); }
                    }
                }
                _ => return Err(invalid(CC, format!("unsupported message role: {role}"))),
            }
        }
        out.insert("input".into(), json!(messages));
    } else {
        for field in ["previous_response_id", "conversation"] {
            if src
                .get(field)
                .is_some_and(|v| !v.is_null() && v.as_str() != Some(""))
            {
                return Err(invalid(RS, format!("{field} requires server-side history; send the full input when converting to Chat Completions")));
            }
        }
        if let Some(max) = src.get("max_output_tokens") {
            out.insert("max_completion_tokens".into(), max.clone());
        }
        if let Some(effort) = payload.pointer("/reasoning/effort") {
            out.insert("reasoning_effort".into(), effort.clone());
        }
        if let Some(format) = payload.pointer("/text/format") {
            let format = if format["type"] == "json_schema" {
                let mut schema = format.clone();
                schema.as_object_mut().unwrap().remove("type");
                json!({"type":"json_schema", "json_schema":schema})
            } else {
                format.clone()
            };
            out.insert("response_format".into(), format);
        }
        if let Some(instructions) = src.get("instructions").filter(|v| !v.is_null()) {
            if !instructions.is_string() {
                return Err(invalid(RS, "instructions must be text"));
            }
            messages.push(json!({"role":"developer", "content":instructions}));
        }
        match src.get("input") {
            Some(Value::String(text)) => messages.push(json!({"role":"user", "content":text})),
            Some(Value::Array(items)) => {
                let mut reasoning = String::new();
                let mut tool_images = Vec::new();
                for item in items {
                    if !matches!(
                        string(item.get("type")),
                        "function_call_output" | "custom_tool_call_output"
                    ) && !tool_images.is_empty()
                    {
                        messages.push(
                            json!({"role":"user","content":std::mem::take(&mut tool_images)}),
                        );
                    }
                    match string(item.get("type")) {
                        "message" | "" if item.get("role").is_some() => {
                            let mut message = json!({"role":item["role"], "content":content(&item["content"], CC, item["role"] == "assistant")?});
                            if item["role"] == "assistant" && !reasoning.is_empty() {
                                message["reasoning_content"] =
                                    json!(std::mem::take(&mut reasoning));
                            }
                            messages.push(message);
                        }
                        "function_call" | "custom_tool_call" => {
                            let call = call_to_chat(item)?;
                            let append = messages
                                .last_mut()
                                .filter(|m| m["role"] == "assistant" && reasoning.is_empty());
                            if let Some(message) = append {
                                if message.get("tool_calls").is_none() {
                                    message["tool_calls"] = json!([]);
                                }
                                message["tool_calls"].as_array_mut().unwrap().push(call);
                            } else {
                                let mut message = json!({"role":"assistant", "content":null, "tool_calls":[call]});
                                if !reasoning.is_empty() {
                                    message["reasoning_content"] =
                                        json!(std::mem::take(&mut reasoning));
                                }
                                messages.push(message);
                            }
                        }
                        "function_call_output" | "custom_tool_call_output" => {
                            let converted = content(&item["output"], CC, false)?;
                            let text = match converted {
                                Value::String(text) => text,
                                Value::Array(parts) => {
                                    let mut text = Vec::new();
                                    for part in parts {
                                        if part["type"] == "text" {
                                            text.push(string(part.get("text")).to_owned());
                                        } else {
                                            tool_images.push(part);
                                        }
                                    }
                                    text.join("\n")
                                }
                                _ => return Err(invalid(RS, "invalid tool output")),
                            };
                            messages.push(json!({"role":"tool", "tool_call_id":item["call_id"], "content":text}));
                        }
                        "reasoning" => {
                            if let Some(parts) = item.get("summary").and_then(Value::as_array) {
                                for part in parts {
                                    reasoning.push_str(string(part.get("text")));
                                }
                            }
                        }
                        kind => {
                            return Err(invalid(
                                RS,
                                format!("unsupported converted input item: {kind}"),
                            ))
                        }
                    }
                }
                if !tool_images.is_empty() {
                    messages.push(json!({"role":"user","content":tool_images}));
                }
                if !reasoning.is_empty() {
                    messages.push(
                        json!({"role":"assistant", "content":null, "reasoning_content":reasoning}),
                    );
                }
            }
            _ => return Err(invalid(RS, "missing input")),
        }
        out.insert("messages".into(), json!(messages));
    }
    if let Some(tools) = src.get("tools").and_then(Value::as_array) {
        let mut converted = Vec::new();
        for tool in tools {
            let kind = string(tool.get("type"));
            if !matches!(kind, "function" | "custom") {
                return Err(invalid(
                    from,
                    format!("unsupported converted tool type: {kind}"),
                ));
            }
            if from == CC {
                let mut definition = tool[kind].clone();
                object(&definition, CC)?;
                definition["type"] = json!(kind);
                converted.push(definition);
            } else {
                let mut definition = tool.clone();
                definition.as_object_mut().unwrap().remove("type");
                converted.push(json!({"type":kind, kind:definition}));
            }
        }
        out.insert("tools".into(), json!(converted));
    }
    if let Some(choice) = src.get("tool_choice") {
        let choice = if choice.is_string() {
            choice.clone()
        } else {
            let kind = string(choice.get("type"));
            if !matches!(kind, "function" | "custom") {
                return Err(invalid(from, "unsupported tool_choice"));
            }
            if from == CC {
                json!({"type":kind, "name":choice[kind]["name"]})
            } else {
                json!({"type":kind, kind:{"name":choice["name"]}})
            }
        };
        out.insert("tool_choice".into(), choice);
    }
    Ok(Value::Object(out))
}

fn call_to_responses(call: &Value) -> Result<Value, ConversionError> {
    let id = call
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("call_{}", stream::unique_id()));
    match string(call.get("type")) {
        "function" | "" => Ok(
            json!({"type":"function_call", "call_id":id, "name":call["function"]["name"], "arguments":arguments(call.pointer("/function/arguments"))}),
        ),
        "custom" => Ok(
            json!({"type":"custom_tool_call", "call_id":id, "name":call["custom"]["name"], "input":call["custom"]["input"]}),
        ),
        _ => Err(invalid(CC, "unsupported tool call")),
    }
}

fn call_to_chat(item: &Value) -> Result<Value, ConversionError> {
    let id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("call_{}", stream::unique_id()));
    match string(item.get("type")) {
        "function_call" => Ok(
            json!({"id":id, "type":"function", "function":{"name":item["name"], "arguments":arguments(item.get("arguments"))}}),
        ),
        "custom_tool_call" => Ok(
            json!({"id":id, "type":"custom", "custom":{"name":item["name"], "input":item["input"]}}),
        ),
        _ => Err(invalid(RS, "unsupported tool call")),
    }
}

fn usage(value: &Value, to: ProxyProtocol) -> Value {
    let (input, output, details, output_details) = if to == RS {
        (
            "prompt_tokens",
            "completion_tokens",
            "prompt_tokens_details",
            "completion_tokens_details",
        )
    } else {
        (
            "input_tokens",
            "output_tokens",
            "input_tokens_details",
            "output_tokens_details",
        )
    };
    let mut out =
        json!({"total_tokens": crate::number(value.get(input)) + crate::number(value.get(output))});
    let keys = if to == RS {
        [
            "input_tokens",
            "output_tokens",
            "input_tokens_details",
            "output_tokens_details",
        ]
    } else {
        [
            "prompt_tokens",
            "completion_tokens",
            "prompt_tokens_details",
            "completion_tokens_details",
        ]
    };
    out[keys[0]] = json!(crate::number(value.get(input)));
    out[keys[1]] = json!(crate::number(value.get(output)));
    for (dest, source) in [(keys[2], details), (keys[3], output_details)] {
        if let Some(detail) = value.get(source) {
            out[dest] = detail.clone();
        }
    }
    out
}

pub(crate) fn response(from: ProxyProtocol, payload: Value) -> Result<Value, ConversionError> {
    object(&payload, from)?;
    if payload.get("error").is_some_and(|e| !e.is_null()) {
        return Err(invalid(from, "upstream returned an error"));
    }
    let id = string(payload.get("id"));
    if from == CC {
        let choice = payload
            .pointer("/choices/0")
            .ok_or_else(|| invalid(CC, "missing choice"))?;
        let message = choice
            .get("message")
            .ok_or_else(|| invalid(CC, "missing message"))?;
        let mut output = Vec::new();
        let reasoning = crate::reasoning::chat_reasoning(message);
        if !reasoning.is_empty() {
            output.push(json!({"type":"reasoning", "id":format!("rs_{id}"), "summary":[{"type":"summary_text", "text":reasoning}]}));
        }
        let mut parts = Vec::new();
        if let Some(text) = message.get("content").filter(|v| !v.is_null()) {
            let text = if let Some(parts) = text.as_array() {
                json!(parts
                    .iter()
                    .filter(|p| p["type"] != "thinking")
                    .collect::<Vec<_>>())
            } else {
                text.clone()
            };
            match content(&text, RS, true)? {
                Value::String(text) if !text.is_empty() => {
                    parts.push(json!({"type":"output_text", "text":text, "annotations":[]}))
                }
                Value::Array(values) => parts.extend(values),
                _ => {}
            }
        }
        if let Some(refusal) = message.get("refusal").filter(|v| !v.is_null()) {
            parts.push(json!({"type":"refusal", "refusal":refusal}));
        }
        if !parts.is_empty() {
            output.push(json!({"type":"message", "id":format!("msg_{id}"), "role":"assistant", "status":"completed", "content":parts}));
        }
        if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
            for (index, call) in calls.iter().enumerate() {
                let mut item = call_to_responses(call)?;
                item["id"] = json!(format!("fc_{id}_{index}"));
                item["status"] = json!("completed");
                output.push(item);
            }
        }
        let reason = string(choice.get("finish_reason"));
        let incomplete = matches!(reason, "length" | "content_filter");
        let mut response = json!({"id":crate::response::swap_id_prefix(id, "chatcmpl-", "resp_"), "object":"response", "model":payload["model"], "created_at":payload["created"], "status":if incomplete { "incomplete" } else { "completed" }, "output":output, "usage":usage(&payload["usage"], RS)});
        if incomplete {
            response["incomplete_details"] = json!({"reason":if reason == "length" { "max_output_tokens" } else { "content_filter" }});
        }
        Ok(response)
    } else {
        let items = payload
            .get("output")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid(RS, "missing output array"))?;
        let mut text = String::new();
        let mut reasoning = String::new();
        let mut refusal = String::new();
        let mut calls = Vec::new();
        for item in items {
            match string(item.get("type")) {
                "message" => {
                    if let Some(parts) = item.get("content").and_then(Value::as_array) {
                        for part in parts {
                            match string(part.get("type")) {
                                "output_text" => text.push_str(string(part.get("text"))),
                                "refusal" => refusal.push_str(string(part.get("refusal"))),
                                _ => return Err(invalid(RS, "unsupported output content")),
                            }
                        }
                    }
                }
                "reasoning" => {
                    if let Some(parts) = item.get("summary").and_then(Value::as_array) {
                        for part in parts {
                            reasoning.push_str(string(part.get("text")));
                        }
                    }
                }
                "function_call" | "custom_tool_call" => calls.push(call_to_chat(item)?),
                _ => return Err(invalid(RS, "unsupported output item")),
            }
        }
        let mut message = json!({"role":"assistant", "content":if text.is_empty() && !calls.is_empty() { Value::Null } else { json!(text) }});
        if !reasoning.is_empty() {
            message["reasoning_content"] = json!(reasoning);
        }
        if !refusal.is_empty() {
            message["refusal"] = json!(refusal);
        }
        let mut finish = if calls.is_empty() {
            "stop"
        } else {
            "tool_calls"
        };
        if !calls.is_empty() {
            message["tool_calls"] = json!(calls);
        }
        if payload["status"] == "incomplete" {
            finish = if payload["incomplete_details"]["reason"] == "content_filter" {
                "content_filter"
            } else {
                "length"
            };
        }
        if payload["status"] == "failed" {
            return Err(invalid(RS, "upstream response failed"));
        }
        Ok(
            json!({"id":crate::response::swap_id_prefix(id, "resp_", "chatcmpl-"), "object":"chat.completion", "created":payload["created_at"], "model":payload["model"], "choices":[{"index":0, "message":message, "finish_reason":finish}], "usage":usage(&payload["usage"], CC)}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StreamConverter;

    fn events(raw: &[String]) -> Vec<Value> {
        raw.iter()
            .filter_map(|raw| raw.lines().find_map(|line| line.strip_prefix("data: ")))
            .filter(|data| *data != "[DONE]")
            .map(|data| serde_json::from_str(data).unwrap())
            .collect()
    }

    #[test]
    fn openai_request_roundtrip_preserves_roles_tools_images_reasoning_and_schema() {
        let original = json!({"model":"model", "stream":true, "max_completion_tokens":512, "reasoning_effort":"high", "parallel_tool_calls":false,
            "response_format":{"type":"json_schema", "json_schema":{"name":"result", "schema":{"type":"object"}, "strict":true}},
            "messages":[{"role":"developer", "content":"Use the tool"}, {"role":"user", "content":[{"type":"image_url", "image_url":{"url":"data:image/png;base64,AA==", "detail":"high"}}]},
                {"role":"assistant", "content":null, "reasoning_content":"Need a lookup", "tool_calls":[{"id":"call_a", "type":"function", "function":{"name":"lookup", "arguments":"{\"q\":1}"}}]},
                {"role":"tool", "tool_call_id":"call_a", "content":"answer"}],
            "tools":[{"type":"function", "function":{"name":"lookup", "parameters":{"type":"object"}, "strict":true}}], "tool_choice":{"type":"function", "function":{"name":"lookup"}}});
        let rs = request(CC, original.clone()).unwrap();
        assert_eq!(rs["input"][0]["role"], "developer");
        assert_eq!(rs["input"][3]["call_id"], "call_a");
        assert_eq!(rs["max_output_tokens"], 512);
        let roundtrip = request(RS, rs).unwrap();
        assert_eq!(roundtrip, original);
    }

    #[test]
    fn responses_instructions_and_unsupported_state_or_tools_are_explicit() {
        let cc = request(
            RS,
            json!({"model":"x", "instructions":"policy", "input":"hello"}),
        )
        .unwrap();
        assert_eq!(
            cc["messages"],
            json!([{"role":"developer","content":"policy"},{"role":"user","content":"hello"}])
        );
        for payload in [
            json!({"input":[],"previous_response_id":"resp_x"}),
            json!({"input":[],"tools":[{"type":"web_search"}]}),
            json!({"input":[{"type":"item_reference","id":"x"}]}),
        ] {
            assert!(request(RS, payload).is_err());
        }
        assert!(request(CC, json!({"messages":[], "n":2})).is_err());
    }

    #[test]
    fn openai_responses_keep_reasoning_custom_tools_usage_and_incomplete_status() {
        let cc = json!({"id":"chatcmpl-a", "model":"x", "created":1, "choices":[{"index":0,"message":{"role":"assistant", "content":"hello", "reasoning_content":"think", "tool_calls":[{"id":"call_a","type":"custom","custom":{"name":"shell","input":"pwd"}}]}, "finish_reason":"length"}],
            "usage":{"prompt_tokens":100,"completion_tokens":12,"prompt_tokens_details":{"cached_tokens":60},"completion_tokens_details":{"reasoning_tokens":8}}});
        let rs = response(CC, cc).unwrap();
        assert_eq!(rs["status"], "incomplete");
        assert_eq!(rs["output"][0]["summary"][0]["text"], "think");
        assert_eq!(rs["output"][2]["call_id"], "call_a");
        let back = response(RS, rs).unwrap();
        assert_eq!(back["choices"][0]["finish_reason"], "length");
        assert_eq!(
            back["choices"][0]["message"]["tool_calls"][0]["custom"]["input"],
            "pwd"
        );
        assert_eq!(back["usage"]["prompt_tokens_details"]["cached_tokens"], 60);
        assert_eq!(
            back["usage"]["completion_tokens_details"]["reasoning_tokens"],
            8
        );
    }

    #[test]
    fn chat_stream_preserves_parallel_tools_reasoning_and_usage_after_finish() {
        let mut converter = StreamConverter::new(CC, RS).unwrap();
        let payloads = [
            json!({"id":"chatcmpl-stream","model":"x","choices":[{"index":0,"delta":{"reasoning_content":"think"}}]}),
            json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_a","function":{"name":"a","arguments":"{\"x\":"}},{"index":1,"id":"call_b","function":{"name":"b","arguments":"{"}}]}}]}),
            json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"function":{"arguments":"}"}},{"index":0,"function":{"arguments":"1}"}}]},"finish_reason":"tool_calls"}]}),
            json!({"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":12,"prompt_tokens_details":{"cached_tokens":60}}}),
        ];
        let mut output = Vec::new();
        for payload in payloads {
            output.extend(
                converter
                    .push_event(&format!("data: {payload}\n\n"))
                    .unwrap(),
            );
        }
        assert!(!output
            .iter()
            .any(|e| e.starts_with("event: response.completed\n")));
        output.extend(converter.push_event("data: [DONE]\n\n").unwrap());
        let values = events(&output);
        let terminal = &values.last().unwrap()["response"];
        assert_eq!(terminal["usage"]["input_tokens"], 100);
        assert_eq!(terminal["output"][0]["summary"][0]["text"], "think");
        assert_eq!(terminal["output"][1]["call_id"], "call_a");
        assert_eq!(terminal["output"][1]["arguments"], "{\"x\":1}");
        assert_eq!(terminal["output"][2]["arguments"], "{}");
        for (index, value) in values.iter().enumerate() {
            assert_eq!(value["sequence_number"], index);
        }

        let mut reverse = StreamConverter::new(RS, CC).unwrap();
        let mut restored = Vec::new();
        for event in output {
            restored.extend(reverse.push_event(&event).unwrap());
        }
        let restored = events(&restored);
        let mut arguments = [String::new(), String::new()];
        for value in &restored {
            if let Some(calls) = value
                .pointer("/choices/0/delta/tool_calls")
                .and_then(Value::as_array)
            {
                for call in calls {
                    arguments[call["index"].as_u64().unwrap() as usize]
                        .push_str(string(call.pointer("/function/arguments")));
                }
            }
        }
        assert_eq!(arguments, ["{\"x\":1}", "{}"]);
        assert_eq!(restored.last().unwrap()["usage"]["prompt_tokens"], 100);
        assert_eq!(
            restored.last().unwrap()["choices"][0]["finish_reason"],
            "tool_calls"
        );
    }

    #[test]
    fn stream_eof_requires_a_terminal_and_never_duplicates_completion() {
        let mut converter = StreamConverter::new(CC, RS).unwrap();
        converter
            .push_event("data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n")
            .unwrap();
        assert!(converter.finish().is_err());
        converter
            .push_event("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\n")
            .unwrap();
        let output = events(&converter.finish().unwrap());
        assert_eq!(output.last().unwrap()["type"], "response.incomplete");
        assert!(converter.finish().unwrap().is_empty());
        assert!(converter.push_event("data: [DONE]\n\n").unwrap().is_empty());
        // Empty provider deltas must not hide content supplied only on done.
        let mut reverse = StreamConverter::new(RS, CC).unwrap();
        let mut converted = Vec::new();
        for payload in [
            json!({"type":"response.created","response":{"id":"resp_done","model":"x"}}),
            json!({"type":"response.output_item.added","output_index":0,"item":{"id":"msg_done","type":"message","content":[]}}),
            json!({"type":"response.output_text.delta","output_index":0,"delta":""}),
            json!({"type":"response.output_item.done","output_index":0,"item":{"id":"msg_done","type":"message","content":[{"type":"output_text","text":"done only"}]}}),
            json!({"type":"response.completed","response":{"id":"resp_done","status":"completed","output":[]}}),
        ] {
            converted.extend(reverse.push_event(&format!("data: {payload}\n\n")).unwrap());
        }
        assert!(events(&converted)
            .iter()
            .any(|value| value["choices"][0]["delta"]["content"] == "done only"));
        let first = call_to_responses(&json!({"function":{"name":"f","arguments":{}}})).unwrap();
        let second = call_to_responses(&json!({"function":{"name":"f","arguments":{}}})).unwrap();
        assert_ne!(first["call_id"], second["call_id"]);
        assert_eq!(first["arguments"], "{}");
    }
}
