//! Stateful Chat Completions / Messages conversion.
use super::*;
use std::collections::HashMap;

// --- CC SSE -> AM SSE ------------------------------------------------------

/// One open AM content block. `Tool(cc_index, am_index)` maps the CC
/// tool_calls `index` onto the sequential AM block index.
#[derive(Clone, Copy, PartialEq)]
enum AmBlock {
    Text(u64),
    Thinking(u64),
}

#[derive(Default)]
pub(super) struct CcToAm {
    started: bool,
    id: Option<String>,
    model: Option<String>,
    next_block: u64,
    open: Option<AmBlock>,
    tools: std::collections::BTreeMap<u64, BufferedTool>,
    finish_reason: Option<String>,
    usage: UsageAccumulator,
    terminated: bool,
    choice: crate::chat::Choice,
    refused: bool,
}

#[derive(Default)]
struct BufferedTool {
    id: String,
    name: String,
    arguments: String,
    custom: bool,
}

impl CcToAm {
    pub(super) fn push(&mut self, raw: &str) -> Result<Vec<String>, ConversionError> {
        let parsed = parse_sse(raw)?;
        let mut out = Vec::new();
        if parsed.data.trim() == "[DONE]" {
            return self.finish();
        }
        if self.terminated {
            return Ok(out);
        }
        let Some(chunk) = parse_payload(&parsed, CC)? else {
            return Ok(out);
        };
        // Some providers deliver mid-stream failures as an {"error": ...}
        // payload; surface it instead of silently ending with end_turn.
        if let Some(error) = stream_error(raw, &chunk) {
            self.close_open(&mut out);
            out.push(emit_typed(
                "error",
                &json!({"type": "error", "error": error}),
            ));
            self.terminated = true;
            return Ok(out);
        }
        if self.id.is_none() {
            self.id = chunk.get("id").and_then(Value::as_str).map(str::to_string);
        }
        if self.model.is_none() {
            self.model = chunk
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        self.usage.observe(CC, &chunk);
        let choices = chunk.get("choices").and_then(Value::as_array);
        let Some(choice) = choices.and_then(|choices| self.choice.select(choices)) else {
            return Ok(out); // usage-only chunk
        };
        self.ensure_started(&mut out);

        let delta = choice.get("delta").cloned().unwrap_or(Value::Null);
        crate::response::validate_chat_content(&delta, AM)?;
        let reasoning = crate::reasoning::chat_reasoning(&delta);
        if !reasoning.is_empty() {
            let index = match self.open {
                Some(AmBlock::Thinking(index)) => index,
                _ => {
                    self.close_open(&mut out);
                    let index = self.next_block;
                    self.next_block += 1;
                    out.push(emit_typed("content_block_start", &json!({"type":"content_block_start","index":index,"content_block":{"type":"thinking","thinking":""}})));
                    self.open = Some(AmBlock::Thinking(index));
                    index
                }
            };
            out.push(emit_typed("content_block_delta", &json!({"type":"content_block_delta","index":index,"delta":{"type":"thinking_delta","thinking":reasoning}})));
        }

        // A few providers stream structured content parts instead of a plain
        // string; fold the text parts so the content is not dropped.
        let mut content = match delta.get("content") {
            Some(Value::String(text)) if !text.is_empty() => Some(text.clone()),
            Some(Value::Array(parts)) => {
                let text = parts
                    .iter()
                    .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
                    .filter_map(|part| part.get("text").and_then(Value::as_str))
                    .collect::<String>();
                (!text.is_empty()).then_some(text)
            }
            _ => None,
        };
        if let Some(refusal) = delta["refusal"].as_str().filter(|s| !s.is_empty()) {
            self.refused = true;
            content.get_or_insert_with(String::new).push_str(refusal);
        }
        if let Some(content) = content {
            let index = self.open_text(&mut out);
            out.push(emit_typed(
                "content_block_delta",
                &json!({"type": "content_block_delta", "index": index, "delta": {"type": "text_delta", "text": content}}),
            ));
        }
        if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for (position, call) in tool_calls.iter().enumerate() {
                let index = crate::chat::tool_index(call, position)?;
                if self.tools.len() >= 1024 && !self.tools.contains_key(&index) {
                    return Err(invalid(CC, "too many tool calls"));
                }
                let tool = self.tools.entry(index).or_default();
                if let Some(id) = call["id"].as_str() {
                    crate::chat::identity(&mut tool.id, id);
                }
                if let Some(name) = call
                    .pointer("/function/name")
                    .or_else(|| call.pointer("/custom/name"))
                    .and_then(Value::as_str)
                {
                    crate::chat::identity(&mut tool.name, name);
                }
                tool.custom |= call["type"] == "custom" || call.get("custom").is_some();
                if let Some(value) = call
                    .pointer("/function/arguments")
                    .or_else(|| call.pointer("/custom/input"))
                {
                    if let Some(text) = value.as_str() {
                        tool.arguments.push_str(text);
                    } else if !value.is_null() {
                        tool.arguments.push_str(&value.to_string());
                    }
                }
                if tool.arguments.len() > 4 * 1024 * 1024 {
                    return Err(invalid(CC, "tool arguments exceed conversion limit"));
                }
            }
        }
        if let Some(finish) = choice.get("finish_reason").and_then(Value::as_str) {
            self.finish_reason = Some(finish.to_owned());
        }
        Ok(out)
    }

    fn ensure_started(&mut self, out: &mut Vec<String>) {
        if self.started {
            return;
        }
        self.started = true;
        let id = self
            .id
            .take()
            .unwrap_or_else(|| "chatcmpl_conv".to_string());
        out.push(emit_typed(
            "message_start",
            &json!({
                "type": "message_start",
                "message": {
                    "id": swap_id_prefix(&id.replace("chatcmpl-", "chatcmpl_"), "chatcmpl_", "msg_"),
                    "type": "message",
                    "role": "assistant",
                    "model": self.model.clone().unwrap_or_default(),
                    "content": [],
                    "stop_reason": null,
                    "stop_sequence": null,
                    "usage": crate::response::openai_usage_to_am(Some(self.usage.wire()), "prompt_tokens", "prompt_tokens_details"),
                }
            }),
        ));
    }

    /// Open (or reuse) the text block, closing any open tool block first.
    fn open_text(&mut self, out: &mut Vec<String>) -> u64 {
        if let Some(AmBlock::Text(index)) = self.open {
            return index;
        }
        self.close_open(out);
        let index = self.next_block;
        self.next_block += 1;
        out.push(emit_typed(
            "content_block_start",
            &json!({"type": "content_block_start", "index": index, "content_block": {"type": "text", "text": ""}}),
        ));
        self.open = Some(AmBlock::Text(index));
        index
    }

    fn close_open(&mut self, out: &mut Vec<String>) {
        let index = match self.open.take() {
            Some(AmBlock::Text(index)) | Some(AmBlock::Thinking(index)) => index,
            None => return,
        };
        out.push(emit_typed(
            "content_block_stop",
            &json!({"type":"content_block_stop","index":index}),
        ));
    }

    pub(super) fn finish(&mut self) -> Result<Vec<String>, ConversionError> {
        if self.terminated {
            return Ok(Vec::new());
        }
        let reason = self
            .finish_reason
            .clone()
            .ok_or_else(|| invalid(CC, "stream ended without finish_reason"))?;
        let mut out = Vec::new();
        self.ensure_started(&mut out);
        self.close_open(&mut out);
        for (cc_index, mut tool) in std::mem::take(&mut self.tools) {
            if tool.id.is_empty() {
                tool.id = format!("toolu_conv_{cc_index}");
            }
            if tool.name.is_empty() {
                return Err(invalid(CC, "incomplete tool identity"));
            }
            let arguments = if tool.custom {
                json!({"input":tool.arguments}).to_string()
            } else if tool.arguments.is_empty() {
                "{}".to_owned()
            } else {
                tool.arguments
            };
            serde_json::from_str::<Value>(&arguments)
                .map_err(|_| invalid(CC, "incomplete tool arguments"))?;
            let index = self.next_block;
            self.next_block += 1;
            out.push(emit_typed("content_block_start",&json!({"type":"content_block_start","index":index,"content_block":{"type":"tool_use","id":tool.id,"name":tool.name,"input":{}}})));
            out.push(emit_typed("content_block_delta",&json!({"type":"content_block_delta","index":index,"delta":{"type":"input_json_delta","partial_json":arguments}})));
            out.push(emit_typed(
                "content_block_stop",
                &json!({"type":"content_block_stop","index":index}),
            ));
        }
        let stop = if self.refused && reason == "stop" {
            "refusal"
        } else {
            cc_finish_to_am_stop(Some(&reason))
        };
        out.push(emit_typed("message_delta",&json!({"type":"message_delta","delta":{"stop_reason":stop,"stop_sequence":null},"usage":crate::response::openai_usage_to_am(Some(self.usage.wire()), "prompt_tokens", "prompt_tokens_details")})));
        out.push(emit_typed("message_stop", &json!({"type":"message_stop"})));
        self.terminated = true;
        Ok(out)
    }
}

// --- AM SSE -> CC SSE ------------------------------------------------------

#[derive(Default)]
pub(super) struct AmToCc {
    started: bool,
    text_started: bool,
    id: String,
    model: String,
    created: u64,
    next_tool: u64,
    /// AM block index -> assigned CC tool_calls index.
    tool_indices: HashMap<u64, u64>,
    tool_fallbacks: HashMap<u64, String>,
    usage: UsageAccumulator,
    pub(super) done: bool,
}

impl AmToCc {
    pub(super) fn push(&mut self, raw: &str) -> Result<Vec<String>, ConversionError> {
        let parsed = parse_sse(raw)?;
        let mut out = Vec::new();
        let Some(payload) = parse_payload(&parsed, AM)? else {
            return Ok(out);
        };
        let error = stream_error(raw, &payload);
        let Some(kind) = error
            .as_ref()
            .map(|_| "error".to_owned())
            .or_else(|| event_kind(&parsed, &payload))
        else {
            return Ok(out);
        };
        if self.done {
            return Ok(out);
        }
        self.usage.observe(AM, &payload);
        match kind.as_str() {
            "message_start" => {
                let message = payload.get("message").cloned().unwrap_or(Value::Null);
                self.id = swap_id_prefix(
                    message
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("msg_conv"),
                    "msg_",
                    "chatcmpl_",
                );
                self.model = message
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                self.created = unix_now();
                self.started = true;
                out.push(self.chunk(json!({"role": "assistant"}), Value::Null, None));
            }
            "content_block_start" => {
                let index = crate::number(payload.get("index"));
                let block = payload.get("content_block").cloned().unwrap_or(Value::Null);
                crate::response::validate_am_block(&block, CC)?;
                if block["type"] == "text" {
                    // Match non-streaming conversion's block separator so a
                    // streamed response can be replayed as identical history.
                    if self.text_started {
                        out.push(self.chunk(json!({"content":"\n"}), Value::Null, None));
                    }
                    self.text_started = true;
                }
                if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                    let tool_index = self.next_tool;
                    self.next_tool += 1;
                    self.tool_indices.insert(index, tool_index);
                    self.tool_fallbacks
                        .insert(index, block.get("input").unwrap_or(&json!({})).to_string());
                    out.push(self.chunk(
                        json!({"tool_calls": [{
                            "index": tool_index,
                            "id": block.get("id").cloned().unwrap_or_else(|| json!(format!("call_conv_{tool_index}"))),
                            "type": "function",
                            "function": {"name": block.get("name").cloned().unwrap_or(Value::Null), "arguments": ""},
                        }]}),
                        Value::Null,
                        None,
                    ));
                }
            }
            "content_block_delta" => {
                let index = crate::number(payload.get("index"));
                let delta = payload.get("delta").cloned().unwrap_or(Value::Null);
                match delta.get("type").and_then(Value::as_str) {
                    Some("thinking_delta") => {
                        if let Some(text) = delta
                            .get("thinking")
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                        {
                            out.push(self.chunk(
                                json!({"reasoning_content":text}),
                                Value::Null,
                                None,
                            ));
                        }
                    }
                    Some("text_delta") => {
                        let text = delta.get("text").and_then(Value::as_str).unwrap_or("");
                        out.push(self.chunk(json!({"content": text}), Value::Null, None));
                    }
                    Some("input_json_delta") => {
                        if let Some(tool_index) = self.tool_indices.get(&index) {
                            let partial = delta
                                .get("partial_json")
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            if !partial.is_empty() {
                                self.tool_fallbacks.remove(&index);
                            }
                            out.push(self.chunk(
                                json!({"tool_calls": [{"index": tool_index, "function": {"arguments": partial}}]}),
                                Value::Null,
                                None,
                            ));
                        }
                    }
                    _ => {}
                }
            }
            "message_delta" => {
                let stop_reason = payload
                    .pointer("/delta/stop_reason")
                    .and_then(Value::as_str);
                let usage = crate::response::am_usage_to_openai(
                    Some(self.usage.wire()),
                    "prompt_tokens",
                    "completion_tokens",
                    "prompt_tokens_details",
                );
                out.push(
                    self.chunk(
                        json!({}),
                        stop_reason
                            .map(|reason| json!(am_stop_to_cc_finish(Some(reason))))
                            .unwrap_or(Value::Null),
                        Some(usage),
                    ),
                );
            }
            "content_block_stop" => {
                let index = crate::number(payload.get("index"));
                if let Some(arguments) = self.tool_fallbacks.remove(&index) {
                    let tool_index = self.tool_indices[&index];
                    out.push(self.chunk(json!({"tool_calls":[{"index":tool_index,"function":{"arguments":arguments}}]}), Value::Null, None));
                }
            }
            "message_stop" if !self.done => {
                out.push("data: [DONE]\n\n".to_string());
                self.done = true;
            }
            // Upstream errors must reach the client; an unnoticed failure
            // would look like a cleanly truncated stream.
            "error" if !self.done => {
                let error = error.expect("error events have normalized details");
                let id = if self.id.is_empty() {
                    "chatcmpl_conv".to_string()
                } else {
                    self.id.clone()
                };
                out.push(emit_data(&json!({
                    "id": id,
                    "object": "chat.completion.chunk",
                    "created": self.created,
                    "model": self.model,
                    "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
                    "error": error,
                })));
                out.push("data: [DONE]\n\n".to_string());
                self.done = true;
            }
            // content_block_stop, ping, and unknown types carry no CC meaning.
            _ => {}
        }
        if let Some(delta) = initial_block_delta(&payload) {
            out.extend(self.push(&emit_typed("content_block_delta", &delta))?);
        }
        Ok(out)
    }

    fn chunk(&self, delta: Value, finish_reason: Value, usage: Option<Value>) -> String {
        let mut chunk = json!({
            "id": self.id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model,
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish_reason}],
        });
        if let Some(usage) = usage {
            chunk["usage"] = usage;
        }
        emit_data(&chunk)
    }
}
