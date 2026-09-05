//! Stateful SSE stream conversion between Anthropic Messages (AM) and the
//! OpenAI wire formats (CC = Chat Completions, RS = Responses).
//!
//! [`ConversionPlugin::convert_stream_event`] is stateless and therefore
//! only passes same-protocol events through; cross-protocol streaming needs
//! per-stream state (open content blocks, tool-call indices, usage), which
//! lives here in [`StreamConverter`]. The proxy constructs one converter
//! per upstream stream and feeds each complete SSE event string to
//! [`StreamConverter::push_event`].

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::response::{am_stop_to_cc_finish, cc_finish_to_am_stop, swap_id_prefix, unix_now};
use crate::{invalid, ConversionError, ProxyProtocol};

use ProxyProtocol::{AnthropicMessages as AM, OpenAiChatCompletions as CC, OpenAiResponses as RS};

/// Converts one SSE stream from `from` to `to`. Same-protocol pairs pass
/// events through unchanged. Feed one complete SSE event (possibly with
/// `event:` and multiple `data:` lines) per call; the result is zero or
/// more complete SSE events in the target protocol.
pub struct StreamConverter {
    state: StreamState,
}

enum StreamState {
    Passthrough,
    CcToAm(CcToAm),
    AmToCc(AmToCc),
    RsToAm(RsToAm),
    AmToRs(AmToRs),
}

impl StreamConverter {
    pub fn new(from: ProxyProtocol, to: ProxyProtocol) -> Result<Self, ConversionError> {
        let state = if from == to {
            StreamState::Passthrough
        } else {
            match (from, to) {
                (CC, AM) => StreamState::CcToAm(CcToAm::default()),
                (AM, CC) => StreamState::AmToCc(AmToCc::default()),
                (RS, AM) => StreamState::RsToAm(RsToAm::default()),
                (AM, RS) => StreamState::AmToRs(AmToRs::default()),
                _ => return Err(ConversionError::Unsupported(from, to)),
            }
        };
        Ok(Self { state })
    }

    pub fn push_event(&mut self, event: &str) -> Result<Vec<String>, ConversionError> {
        match &mut self.state {
            StreamState::Passthrough => Ok(vec![event.to_string()]),
            StreamState::CcToAm(state) => state.push(event),
            StreamState::AmToCc(state) => state.push(event),
            StreamState::RsToAm(state) => state.push(event),
            StreamState::AmToRs(state) => state.push(event),
        }
    }
}

// --- SSE framing helpers ---------------------------------------------------

struct SseEvent {
    /// The `event:` field, present for typed protocols (AM, RS).
    kind: Option<String>,
    /// All `data:` lines joined with `\n`, per the SSE specification.
    data: String,
}

fn parse_sse(raw: &str) -> Result<SseEvent, ConversionError> {
    let mut kind = None;
    let mut data_lines: Vec<&str> = Vec::new();
    for line in raw.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(value) = line.strip_prefix("event:") {
            kind = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.strip_prefix(' ').unwrap_or(value));
        }
    }
    if kind.is_none() && data_lines.is_empty() {
        return Err(ConversionError::InvalidEvent(raw.to_string()));
    }
    Ok(SseEvent {
        kind,
        data: data_lines.join("\n"),
    })
}

fn emit_typed(event: &str, data: &Value) -> String {
    format!(
        "event: {event}\ndata: {}\n\n",
        serde_json::to_string(data).unwrap_or_default()
    )
}

fn emit_data(data: &Value) -> String {
    format!(
        "data: {}\n\n",
        serde_json::to_string(data).unwrap_or_default()
    )
}

/// Event type from the `event:` line, falling back to the payload's `type`.
fn event_kind(parsed: &SseEvent, payload: &Value) -> Option<String> {
    parsed.kind.clone().or_else(|| {
        payload
            .get("type")
            .and_then(Value::as_str)
            .map(str::to_string)
    })
}

fn parse_payload(
    parsed: &SseEvent,
    protocol: ProxyProtocol,
) -> Result<Option<Value>, ConversionError> {
    if parsed.data.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&parsed.data)
        .map(Some)
        .map_err(|err| invalid(protocol, format!("event data is not valid JSON: {err}")))
}

// --- CC SSE -> AM SSE ------------------------------------------------------

/// One open AM content block. `Tool(cc_index, am_index)` maps the CC
/// tool_calls `index` onto the sequential AM block index.
#[derive(Clone, Copy, PartialEq)]
enum AmBlock {
    Text(u64),
    Tool(u64, u64),
}

#[derive(Default)]
struct CcToAm {
    started: bool,
    id: Option<String>,
    model: Option<String>,
    next_block: u64,
    open: Option<AmBlock>,
    /// CC tool index -> AM block index, for fragments of already-seen calls.
    tool_blocks: HashMap<u64, u64>,
    input_tokens: u64,
    output_tokens: u64,
    terminated: bool,
}

impl CcToAm {
    fn push(&mut self, raw: &str) -> Result<Vec<String>, ConversionError> {
        let parsed = parse_sse(raw)?;
        let mut out = Vec::new();
        if parsed.data.trim() == "[DONE]" {
            self.finish(&mut out);
            return Ok(out);
        }
        let Some(chunk) = parse_payload(&parsed, CC)? else {
            return Ok(out);
        };
        if self.id.is_none() {
            self.id = chunk.get("id").and_then(Value::as_str).map(str::to_string);
        }
        if self.model.is_none() {
            self.model = chunk
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if let Some(usage) = chunk.get("usage") {
            self.input_tokens = crate::number(usage.get("prompt_tokens"));
            self.output_tokens = crate::number(usage.get("completion_tokens"));
        }
        let choices = chunk.get("choices").and_then(Value::as_array);
        let Some(choice) = choices.and_then(|choices| choices.first()) else {
            return Ok(out); // usage-only chunk
        };
        self.ensure_started(&mut out);

        let delta = choice.get("delta").cloned().unwrap_or(Value::Null);
        if let Some(content) = delta.get("content").and_then(Value::as_str) {
            if !content.is_empty() {
                let index = self.open_text(&mut out);
                out.push(emit_typed(
                    "content_block_delta",
                    &json!({"type": "content_block_delta", "index": index, "delta": {"type": "text_delta", "text": content}}),
                ));
            }
        }
        if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for call in tool_calls {
                let cc_index = call.get("index").and_then(Value::as_u64).unwrap_or(0);
                let am_index = self.open_tool(cc_index, call, &mut out);
                if let Some(arguments) = call.pointer("/function/arguments").and_then(Value::as_str)
                {
                    if !arguments.is_empty() {
                        out.push(emit_typed(
                            "content_block_delta",
                            &json!({"type": "content_block_delta", "index": am_index, "delta": {"type": "input_json_delta", "partial_json": arguments}}),
                        ));
                    }
                }
            }
        }
        if let Some(finish) = choice.get("finish_reason").and_then(Value::as_str) {
            self.close_open(&mut out);
            out.push(emit_typed(
                "message_delta",
                &json!({"type": "message_delta", "delta": {"stop_reason": cc_finish_to_am_stop(Some(finish)), "stop_sequence": null}, "usage": {"output_tokens": self.output_tokens}}),
            ));
            out.push(emit_typed("message_stop", &json!({"type": "message_stop"})));
            self.terminated = true;
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
                    "id": swap_id_prefix(&id, "chatcmpl_", "msg_"),
                    "type": "message",
                    "role": "assistant",
                    "model": self.model.clone().unwrap_or_default(),
                    "content": [],
                    "stop_reason": null,
                    "stop_sequence": null,
                    "usage": {"input_tokens": self.input_tokens, "output_tokens": 0},
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

    /// Open (or switch to) the tool block for CC tool_calls `cc_index`.
    fn open_tool(&mut self, cc_index: u64, call: &Value, out: &mut Vec<String>) -> u64 {
        if let Some(AmBlock::Tool(open_cc, am_index)) = self.open {
            if open_cc == cc_index {
                return am_index;
            }
        }
        self.close_open(out);
        if let Some(am_index) = self.tool_blocks.get(&cc_index) {
            // Fragments for an already-started call; reopen its block index.
            self.open = Some(AmBlock::Tool(cc_index, *am_index));
            return *am_index;
        }
        let am_index = self.next_block;
        self.next_block += 1;
        let id = call
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("toolu_conv_{cc_index}"));
        let name = call
            .pointer("/function/name")
            .and_then(Value::as_str)
            .unwrap_or("");
        out.push(emit_typed(
            "content_block_start",
            &json!({"type": "content_block_start", "index": am_index, "content_block": {"type": "tool_use", "id": id, "name": name}}),
        ));
        self.tool_blocks.insert(cc_index, am_index);
        self.open = Some(AmBlock::Tool(cc_index, am_index));
        am_index
    }

    fn close_open(&mut self, out: &mut Vec<String>) {
        let index = match self.open.take() {
            Some(AmBlock::Text(index)) | Some(AmBlock::Tool(_, index)) => index,
            None => return,
        };
        out.push(emit_typed(
            "content_block_stop",
            &json!({"type": "content_block_stop", "index": index}),
        ));
    }

    /// Terminal fallback for `[DONE]` arriving without a finish_reason.
    fn finish(&mut self, out: &mut Vec<String>) {
        if self.terminated {
            return;
        }
        self.ensure_started(out);
        self.close_open(out);
        out.push(emit_typed(
            "message_delta",
            &json!({"type": "message_delta", "delta": {"stop_reason": "end_turn", "stop_sequence": null}, "usage": {"output_tokens": self.output_tokens}}),
        ));
        out.push(emit_typed("message_stop", &json!({"type": "message_stop"})));
        self.terminated = true;
    }
}

// --- AM SSE -> CC SSE ------------------------------------------------------

#[derive(Default)]
struct AmToCc {
    started: bool,
    id: String,
    model: String,
    created: u64,
    next_tool: u64,
    /// AM block index -> assigned CC tool_calls index.
    tool_indices: HashMap<u64, u64>,
    input_tokens: u64,
    done: bool,
}

impl AmToCc {
    fn push(&mut self, raw: &str) -> Result<Vec<String>, ConversionError> {
        let parsed = parse_sse(raw)?;
        let mut out = Vec::new();
        let Some(payload) = parse_payload(&parsed, AM)? else {
            return Ok(out);
        };
        let Some(kind) = event_kind(&parsed, &payload) else {
            return Ok(out);
        };
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
                self.input_tokens = crate::number(message.pointer("/usage/input_tokens"));
                self.started = true;
                out.push(self.chunk(json!({"role": "assistant"}), Value::Null, None));
            }
            "content_block_start" => {
                let index = crate::number(payload.get("index"));
                let block = payload.get("content_block").cloned().unwrap_or(Value::Null);
                if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                    let tool_index = self.next_tool;
                    self.next_tool += 1;
                    self.tool_indices.insert(index, tool_index);
                    out.push(self.chunk(
                        json!({"tool_calls": [{
                            "index": tool_index,
                            "id": block.get("id").cloned().unwrap_or(Value::Null),
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
                let output_tokens = crate::number(payload.pointer("/usage/output_tokens"));
                let usage = json!({
                    "prompt_tokens": self.input_tokens,
                    "completion_tokens": output_tokens,
                    "total_tokens": self.input_tokens + output_tokens,
                });
                out.push(self.chunk(
                    json!({}),
                    json!(am_stop_to_cc_finish(stop_reason)),
                    Some(usage),
                ));
            }
            "message_stop" if !self.done => {
                out.push("data: [DONE]\n\n".to_string());
                self.done = true;
            }
            // content_block_stop, ping, and unknown types carry no CC meaning.
            _ => {}
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

// --- RS SSE -> AM SSE ------------------------------------------------------

#[derive(Default)]
struct RsToAm {
    started: bool,
    id: Option<String>,
    model: Option<String>,
    next_block: u64,
    open: Option<AmBlock>,
    saw_tool: bool,
    input_tokens: u64,
    output_tokens: u64,
    terminated: bool,
}

impl RsToAm {
    fn push(&mut self, raw: &str) -> Result<Vec<String>, ConversionError> {
        let parsed = parse_sse(raw)?;
        let mut out = Vec::new();
        let Some(payload) = parse_payload(&parsed, RS)? else {
            return Ok(out);
        };
        let Some(kind) = event_kind(&parsed, &payload) else {
            return Ok(out);
        };
        match kind.as_str() {
            "response.created" | "response.in_progress" => {
                let response = payload.get("response").cloned().unwrap_or(Value::Null);
                if self.id.is_none() {
                    self.id = response
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                }
                if self.model.is_none() {
                    self.model = response
                        .get("model")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                }
                self.ensure_started(&mut out);
            }
            "response.output_item.added" => {
                self.ensure_started(&mut out);
                let item = payload.get("item").cloned().unwrap_or(Value::Null);
                if item.get("type").and_then(Value::as_str) == Some("function_call") {
                    self.close_open(&mut out);
                    let index = self.next_block;
                    self.next_block += 1;
                    let id = item
                        .get("call_id")
                        .or_else(|| item.get("id"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("toolu_conv_{index}"));
                    out.push(emit_typed(
                        "content_block_start",
                        &json!({"type": "content_block_start", "index": index, "content_block": {
                            "type": "tool_use",
                            "id": id,
                            "name": item.get("name").cloned().unwrap_or(Value::Null),
                        }}),
                    ));
                    self.open = Some(AmBlock::Tool(0, index));
                    self.saw_tool = true;
                }
            }
            "response.output_text.delta" => {
                self.ensure_started(&mut out);
                let index = self.open_text(&mut out);
                let text = payload.get("delta").and_then(Value::as_str).unwrap_or("");
                out.push(emit_typed(
                    "content_block_delta",
                    &json!({"type": "content_block_delta", "index": index, "delta": {"type": "text_delta", "text": text}}),
                ));
            }
            "response.function_call_arguments.delta" => {
                self.ensure_started(&mut out);
                // Arguments without a preceding output_item.added: open a
                // placeholder tool block rather than dropping the fragment.
                let index = match self.open {
                    Some(AmBlock::Tool(_, index)) => index,
                    _ => {
                        self.close_open(&mut out);
                        let index = self.next_block;
                        self.next_block += 1;
                        out.push(emit_typed(
                            "content_block_start",
                            &json!({"type": "content_block_start", "index": index, "content_block": {"type": "tool_use", "id": format!("toolu_conv_{index}"), "name": ""}}),
                        ));
                        self.open = Some(AmBlock::Tool(0, index));
                        self.saw_tool = true;
                        index
                    }
                };
                let partial = payload.get("delta").and_then(Value::as_str).unwrap_or("");
                out.push(emit_typed(
                    "content_block_delta",
                    &json!({"type": "content_block_delta", "index": index, "delta": {"type": "input_json_delta", "partial_json": partial}}),
                ));
            }
            "response.output_item.done" => {
                let item = payload.get("item").cloned().unwrap_or(Value::Null);
                match item.get("type").and_then(Value::as_str) {
                    Some("function_call") => {
                        if matches!(self.open, Some(AmBlock::Tool(_, _))) {
                            self.close_open(&mut out);
                        }
                    }
                    Some("message") => {
                        if matches!(self.open, Some(AmBlock::Text(_))) {
                            self.close_open(&mut out);
                        }
                    }
                    _ => {}
                }
            }
            "response.output_text.done" => {
                if matches!(self.open, Some(AmBlock::Text(_))) {
                    self.close_open(&mut out);
                }
            }
            "response.completed" | "response.incomplete" => {
                self.ensure_started(&mut out);
                self.close_open(&mut out);
                let response = payload.get("response").cloned().unwrap_or(Value::Null);
                if let Some(usage) = response.get("usage") {
                    self.input_tokens = crate::number(usage.get("input_tokens"));
                    self.output_tokens = crate::number(usage.get("output_tokens"));
                }
                let incomplete = kind == "response.incomplete"
                    || response.get("status").and_then(Value::as_str) == Some("incomplete");
                let stop_reason = if incomplete {
                    "max_tokens"
                } else if self.saw_tool {
                    "tool_use"
                } else {
                    "end_turn"
                };
                out.push(emit_typed(
                    "message_delta",
                    &json!({"type": "message_delta", "delta": {"stop_reason": stop_reason, "stop_sequence": null}, "usage": {"output_tokens": self.output_tokens}}),
                ));
                out.push(emit_typed("message_stop", &json!({"type": "message_stop"})));
                self.terminated = true;
            }
            _ => {}
        }
        Ok(out)
    }

    fn ensure_started(&mut self, out: &mut Vec<String>) {
        if self.started {
            return;
        }
        self.started = true;
        let id = self.id.take().unwrap_or_else(|| "resp_conv".to_string());
        out.push(emit_typed(
            "message_start",
            &json!({
                "type": "message_start",
                "message": {
                    "id": swap_id_prefix(&id, "resp_", "msg_"),
                    "type": "message",
                    "role": "assistant",
                    "model": self.model.clone().unwrap_or_default(),
                    "content": [],
                    "stop_reason": null,
                    "stop_sequence": null,
                    "usage": {"input_tokens": self.input_tokens, "output_tokens": 0},
                }
            }),
        ));
    }

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
            Some(AmBlock::Text(index)) | Some(AmBlock::Tool(_, index)) => index,
            None => return,
        };
        out.push(emit_typed(
            "content_block_stop",
            &json!({"type": "content_block_stop", "index": index}),
        ));
    }
}

// --- AM SSE -> RS SSE ------------------------------------------------------

#[derive(Default)]
struct AmToRs {
    started: bool,
    id: String,
    model: String,
    created: u64,
    next_output: u64,
    /// Open text item: (output_index, item_id, accumulated text).
    text_item: Option<(u64, String, String)>,
    /// Open function_call item: (output_index, item, accumulated arguments).
    tool_item: Option<(u64, Value, String)>,
    input_tokens: u64,
    output_tokens: u64,
    output: Vec<Value>,
    stop_reason: String,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
    sequence: u64,
}

impl AmToRs {
    fn push(&mut self, raw: &str) -> Result<Vec<String>, ConversionError> {
        let parsed = parse_sse(raw)?;
        let mut out = Vec::new();
        let Some(payload) = parse_payload(&parsed, AM)? else {
            return Ok(out);
        };
        let Some(kind) = event_kind(&parsed, &payload) else {
            return Ok(out);
        };
        match kind.as_str() {
            "message_start" => {
                let message = payload.get("message").cloned().unwrap_or(Value::Null);
                self.id = swap_id_prefix(
                    message
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("msg_conv"),
                    "msg_",
                    "resp_",
                );
                self.model = message
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                self.created = unix_now();
                self.input_tokens = crate::number(message.pointer("/usage/input_tokens"));
                self.cache_read_tokens =
                    crate::number(message.pointer("/usage/cache_read_input_tokens"));
                self.cache_creation_tokens =
                    crate::number(message.pointer("/usage/cache_creation_input_tokens"));
                self.started = true;
                for kind in ["response.created", "response.in_progress"] {
                    out.push(emit_typed(
                        kind,
                        &self.response_envelope(kind, "in_progress"),
                    ));
                }
            }
            "content_block_start" => {
                let block = payload.get("content_block").cloned().unwrap_or(Value::Null);
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        self.finalize_open(&mut out);
                        self.start_text(&mut out);
                    }
                    Some("tool_use") => {
                        self.finalize_open(&mut out);
                        let index = self.next_output;
                        self.next_output += 1;
                        let item = json!({
                            "type": "function_call",
                            "id": format!("{}_fc_{index}", self.id),
                            "call_id": block.get("id").cloned().unwrap_or(Value::Null),
                            "name": block.get("name").cloned().unwrap_or(Value::Null),
                            "arguments": "",
                            "status": "in_progress",
                        });
                        out.push(emit_typed(
                            "response.output_item.added",
                            &json!({"type": "response.output_item.added", "output_index": index, "item": item}),
                        ));
                        let mut item = item;
                        // Empty-argument calls need valid JSON even when the
                        // upstream emits no input_json_delta at all.
                        item["arguments"] =
                            json!(block.get("input").unwrap_or(&json!({})).to_string());
                        self.tool_item = Some((index, item, String::new()));
                    }
                    _ => {}
                }
            }
            "content_block_delta" => {
                let delta = payload.get("delta").cloned().unwrap_or(Value::Null);
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        let text = delta.get("text").and_then(Value::as_str).unwrap_or("");
                        if self.text_item.is_none() {
                            self.finalize_open(&mut out);
                            self.start_text(&mut out);
                        }
                        if let Some((index, item_id, acc)) = &mut self.text_item {
                            acc.push_str(text);
                            out.push(emit_typed(
                                "response.output_text.delta",
                                &json!({"type": "response.output_text.delta", "item_id": *item_id, "output_index": *index, "content_index": 0, "delta": text}),
                            ));
                        }
                    }
                    Some("input_json_delta") => {
                        let partial = delta
                            .get("partial_json")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        if let Some((index, item, acc)) = &mut self.tool_item {
                            acc.push_str(partial);
                            out.push(emit_typed(
                                "response.function_call_arguments.delta",
                                &json!({"type": "response.function_call_arguments.delta", "item_id": item.get("id").cloned().unwrap_or(Value::Null), "output_index": *index, "delta": partial}),
                            ));
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => self.finalize_open(&mut out),
            "message_delta" => {
                self.output_tokens = crate::number(payload.pointer("/usage/output_tokens"));
                self.stop_reason = payload
                    .pointer("/delta/stop_reason")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
            }
            "message_stop" => {
                self.finalize_open(&mut out);
                let (kind, status) = if self.stop_reason == "max_tokens" {
                    ("response.incomplete", "incomplete")
                } else {
                    ("response.completed", "completed")
                };
                out.push(emit_typed(kind, &self.response_envelope(kind, status)));
            }
            _ => {}
        }
        for event in &mut out {
            let parsed = parse_sse(event)?;
            let mut value: Value = serde_json::from_str(&parsed.data)
                .map_err(|_| invalid(AM, "invalid converted event"))?;
            value["sequence_number"] = json!(self.sequence);
            self.sequence += 1;
            *event = emit_typed(parsed.kind.as_deref().unwrap_or_default(), &value);
        }
        Ok(out)
    }

    fn response_envelope(&self, kind: &str, status: &str) -> Value {
        let mut response = json!({
            "id": self.id,
            "object": "response",
            "created_at": self.created,
            "status": status,
            "model": self.model,
            "output": self.output,
        });
        if matches!(kind, "response.completed" | "response.incomplete") {
            let total_input =
                self.input_tokens + self.cache_read_tokens + self.cache_creation_tokens;
            response["usage"] = json!({
                "input_tokens": total_input,
                "output_tokens": self.output_tokens,
                "total_tokens": total_input + self.output_tokens,
                "input_tokens_details": { "cached_tokens": self.cache_read_tokens, "cache_creation_tokens": self.cache_creation_tokens },
            });
        }
        if status == "incomplete" {
            response["incomplete_details"] = json!({"reason":"max_output_tokens"});
        }
        json!({"type": kind, "response": response})
    }

    fn start_text(&mut self, out: &mut Vec<String>) {
        let index = self.next_output;
        self.next_output += 1;
        let item_id = format!("{}_msg_{index}", self.id);
        out.push(emit_typed("response.output_item.added", &json!({
            "type":"response.output_item.added", "output_index":index,
            "item":{"type":"message","id":item_id,"role":"assistant","status":"in_progress","content":[]}
        })));
        out.push(emit_typed("response.content_part.added", &json!({
            "type":"response.content_part.added","item_id":item_id,"output_index":index,"content_index":0,
            "part":{"type":"output_text","text":"","annotations":[]}
        })));
        self.text_item = Some((index, item_id, String::new()));
    }

    /// Close the open text or function_call item, if any.
    fn finalize_open(&mut self, out: &mut Vec<String>) {
        if let Some((index, item_id, text)) = self.text_item.take() {
            out.push(emit_typed(
                "response.output_text.done",
                &json!({"type": "response.output_text.done", "item_id": item_id, "output_index": index, "content_index": 0, "text": text}),
            ));
            let part = json!({"type":"output_text","text":text,"annotations":[]});
            let item = json!({"type":"message","id":item_id,"role":"assistant","status":"completed","content":[part]});
            out.push(emit_typed("response.content_part.done", &json!({"type":"response.content_part.done","item_id":item_id,"output_index":index,"content_index":0,"part":part})));
            out.push(emit_typed(
                "response.output_item.done",
                &json!({"type":"response.output_item.done","output_index":index,"item":item}),
            ));
            self.output.push(item);
        }
        if let Some((index, mut item, arguments)) = self.tool_item.take() {
            if !arguments.is_empty() {
                item["arguments"] = json!(arguments);
            }
            item["status"] = json!("completed");
            out.push(emit_typed(
                "response.function_call_arguments.done",
                &json!({"type":"response.function_call_arguments.done","item_id":item["id"],"output_index":index,"arguments":item["arguments"]}),
            ));
            out.push(emit_typed(
                "response.output_item.done",
                &json!({"type": "response.output_item.done", "output_index": index, "item": item}),
            ));
            self.output.push(item);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn convert_all(converter: &mut StreamConverter, events: &[&str]) -> Vec<String> {
        let mut out = Vec::new();
        for event in events {
            out.extend(converter.push_event(event).unwrap());
        }
        out
    }

    /// Parse emitted SSE events into (kind, data-json) pairs.
    fn parsed(events: &[String]) -> Vec<(Option<String>, Value)> {
        events
            .iter()
            .map(|raw| {
                let event = parse_sse(raw).unwrap();
                let data = serde_json::from_str(&event.data).unwrap_or(Value::Null);
                (event.kind, data)
            })
            .collect()
    }

    fn kinds(events: &[String]) -> Vec<String> {
        parsed(events)
            .iter()
            .map(|(kind, _)| kind.clone().unwrap_or_default())
            .collect()
    }

    // --- CC -> AM ----------------------------------------------------------

    #[test]
    fn cc_text_stream_becomes_am_event_sequence() {
        let mut c = StreamConverter::new(CC, AM).unwrap();
        let out = convert_all(
            &mut c,
            &[
                "data: {\"id\":\"chatcmpl_1\",\"model\":\"gpt-5\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Hel\"},\"finish_reason\":null}]}\n\n",
                "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"lo\"},\"finish_reason\":null}]}\n\n",
                "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":2}}\n\n",
                "data: [DONE]\n\n",
            ],
        );
        assert_eq!(
            kinds(&out),
            vec![
                "message_start",
                "content_block_start",
                "content_block_delta",
                "content_block_delta",
                "content_block_stop",
                "message_delta",
                "message_stop",
            ]
        );
        let events = parsed(&out);
        assert_eq!(events[0].1["message"]["id"], "msg_1");
        assert_eq!(events[0].1["message"]["model"], "gpt-5");
        assert_eq!(
            events[2].1["delta"],
            json!({"type": "text_delta", "text": "Hel"})
        );
        assert_eq!(
            events[3].1["delta"],
            json!({"type": "text_delta", "text": "lo"})
        );
        assert_eq!(events[5].1["delta"]["stop_reason"], "end_turn");
        assert_eq!(events[5].1["usage"]["output_tokens"], 2);
    }

    #[test]
    fn cc_tool_call_stream_with_fragmented_arguments() {
        let mut c = StreamConverter::new(CC, AM).unwrap();
        let out = convert_all(
            &mut c,
            &[
                "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"f\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
                "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\"}}]},\"finish_reason\":null}]}\n\n",
                "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"a\\\":1}\"}}]},\"finish_reason\":null}]}\n\n",
                "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
                "data: [DONE]\n\n",
            ],
        );
        assert_eq!(
            kinds(&out),
            vec![
                "message_start",
                "content_block_start",
                "content_block_delta",
                "content_block_delta",
                "content_block_stop",
                "message_delta",
                "message_stop",
            ]
        );
        let events = parsed(&out);
        assert_eq!(
            events[1].1["content_block"],
            json!({"type": "tool_use", "id": "call_1", "name": "f"})
        );
        assert_eq!(
            events[2].1["delta"],
            json!({"type": "input_json_delta", "partial_json": "{"})
        );
        assert_eq!(
            events[3].1["delta"],
            json!({"type": "input_json_delta", "partial_json": "\"a\":1}"})
        );
        assert_eq!(events[5].1["delta"]["stop_reason"], "tool_use");
    }

    #[test]
    fn cc_done_without_finish_reason_emits_terminal_events() {
        let mut c = StreamConverter::new(CC, AM).unwrap();
        let out = convert_all(
            &mut c,
            &[
                "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"}}]}\n\n",
                "data: [DONE]\n\n",
            ],
        );
        assert_eq!(
            kinds(&out),
            vec![
                "message_start",
                "content_block_start",
                "content_block_delta",
                "content_block_stop",
                "message_delta",
                "message_stop",
            ]
        );
        let events = parsed(&out);
        assert_eq!(events[4].1["delta"]["stop_reason"], "end_turn");
    }

    #[test]
    fn cc_multiline_data_is_merged() {
        let mut c = StreamConverter::new(CC, AM).unwrap();
        let out = c
            .push_event("data: {\ndata: \"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"x\"}}]\ndata: }\n\n")
            .unwrap();
        assert_eq!(
            kinds(&out),
            vec![
                "message_start",
                "content_block_start",
                "content_block_delta"
            ]
        );
    }

    // --- AM -> CC ----------------------------------------------------------

    #[test]
    fn am_text_stream_becomes_cc_chunks() {
        let mut c = StreamConverter::new(AM, CC).unwrap();
        let out = convert_all(
            &mut c,
            &[
                "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-sonnet-4-5\",\"usage\":{\"input_tokens\":9,\"output_tokens\":0}}}\n\n",
                "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\n",
                "event: ping\ndata: {\"type\":\"ping\"}\n\n",
                "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":4}}\n\n",
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            ],
        );
        assert_eq!(out.len(), 4);
        let first = parsed(&out[..1]);
        assert_eq!(first[0].1["id"], "chatcmpl_1");
        assert_eq!(first[0].1["model"], "claude-sonnet-4-5");
        assert_eq!(
            first[0].1["choices"][0]["delta"],
            json!({"role": "assistant"})
        );

        let delta: Value = serde_json::from_str(&parse_sse(&out[1]).unwrap().data).unwrap();
        assert_eq!(delta["choices"][0]["delta"], json!({"content": "Hi"}));

        let finish: Value = serde_json::from_str(&parse_sse(&out[2]).unwrap().data).unwrap();
        assert_eq!(finish["choices"][0]["finish_reason"], "stop");
        assert_eq!(
            finish["usage"],
            json!({"prompt_tokens": 9, "completion_tokens": 4, "total_tokens": 13})
        );

        assert_eq!(out[3], "data: [DONE]\n\n");
    }

    #[test]
    fn am_tool_use_stream_becomes_cc_tool_call_fragments() {
        let mut c = StreamConverter::new(AM, CC).unwrap();
        let out = convert_all(
            &mut c,
            &[
                "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"m\",\"usage\":{\"input_tokens\":1}}}\n\n",
                "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"f\"}}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\"}}\n\n",
                "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_2\",\"name\":\"g\"}}\n\n",
                "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":5}}\n\n",
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            ],
        );
        let chunks: Vec<Value> = out
            .iter()
            .filter(|raw| !raw.contains("[DONE]"))
            .map(|raw| serde_json::from_str(&parse_sse(raw).unwrap().data).unwrap())
            .collect();
        assert_eq!(
            chunks[1]["choices"][0]["delta"]["tool_calls"][0],
            json!({"index": 0, "id": "toolu_1", "type": "function", "function": {"name": "f", "arguments": ""}})
        );
        assert_eq!(
            chunks[2]["choices"][0]["delta"]["tool_calls"][0],
            json!({"index": 0, "function": {"arguments": "{"}})
        );
        // second tool block gets the next CC index
        assert_eq!(
            chunks[3]["choices"][0]["delta"]["tool_calls"][0],
            json!({"index": 1, "id": "toolu_2", "type": "function", "function": {"name": "g", "arguments": ""}})
        );
        assert_eq!(chunks[4]["choices"][0]["finish_reason"], "tool_calls");
        assert!(out.last().unwrap().contains("[DONE]"));
    }

    #[test]
    fn am_message_stop_without_message_delta_still_terminates() {
        let mut c = StreamConverter::new(AM, CC).unwrap();
        let out = convert_all(
            &mut c,
            &[
                "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"m\"}}\n\n",
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            ],
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[1], "data: [DONE]\n\n");
    }

    #[test]
    fn unknown_events_are_ignored() {
        let mut am_cc = StreamConverter::new(AM, CC).unwrap();
        assert!(am_cc
            .push_event("event: some_future_event\ndata: {\"type\":\"some_future_event\"}\n\n")
            .unwrap()
            .is_empty());

        let mut rs_am = StreamConverter::new(RS, AM).unwrap();
        assert!(rs_am.push_event("event: response.reasoning.delta\ndata: {\"type\":\"response.reasoning.delta\",\"delta\":\"hmm\"}\n\n").unwrap().is_empty());

        let mut cc_am = StreamConverter::new(CC, AM).unwrap();
        assert!(cc_am.push_event("data: {\"id\":\"chatcmpl_1\",\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":0}}\n\n").unwrap().is_empty());
    }

    // --- RS -> AM ----------------------------------------------------------

    #[test]
    fn rs_tool_stream_becomes_am_tool_use() {
        let mut c = StreamConverter::new(RS, AM).unwrap();
        let out = convert_all(
            &mut c,
            &[
                "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5\",\"status\":\"in_progress\"}}\n\n",
                "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"f\",\"arguments\":\"\"}}\n\n",
                "event: response.function_call_arguments.delta\ndata: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"output_index\":0,\"delta\":\"{\\\"a\\\"\"}\n\n",
                "event: response.function_call_arguments.delta\ndata: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"output_index\":0,\"delta\":\":1}\"}\n\n",
                "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"f\",\"arguments\":\"{\\\"a\\\":1}\"}}\n\n",
                "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"status\":\"completed\",\"usage\":{\"input_tokens\":11,\"output_tokens\":6}}}\n\n",
            ],
        );
        assert_eq!(
            kinds(&out),
            vec![
                "message_start",
                "content_block_start",
                "content_block_delta",
                "content_block_delta",
                "content_block_stop",
                "message_delta",
                "message_stop",
            ]
        );
        let events = parsed(&out);
        assert_eq!(events[0].1["message"]["id"], "msg_1");
        assert_eq!(
            events[1].1["content_block"],
            json!({"type": "tool_use", "id": "call_1", "name": "f"})
        );
        assert_eq!(
            events[2].1["delta"],
            json!({"type": "input_json_delta", "partial_json": "{\"a\""})
        );
        assert_eq!(events[5].1["delta"]["stop_reason"], "tool_use");
        assert_eq!(events[5].1["usage"]["output_tokens"], 6);
    }

    #[test]
    fn rs_text_and_incomplete_status() {
        let mut c = StreamConverter::new(RS, AM).unwrap();
        let out = convert_all(
            &mut c,
            &[
                "event: response.in_progress\ndata: {\"type\":\"response.in_progress\",\"response\":{\"id\":\"resp_2\",\"model\":\"gpt-5\"}}\n\n",
                "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"abc\"}\n\n",
                "event: response.output_text.done\ndata: {\"type\":\"response.output_text.done\",\"output_index\":0,\"text\":\"abc\"}\n\n",
                "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_2\",\"status\":\"incomplete\",\"usage\":{\"input_tokens\":2,\"output_tokens\":50}}}\n\n",
            ],
        );
        let events = parsed(&out);
        assert_eq!(events[0].1["message"]["id"], "msg_2");
        assert_eq!(
            events[2].1["delta"],
            json!({"type": "text_delta", "text": "abc"})
        );
        let last_delta = &events[events.len() - 2];
        assert_eq!(last_delta.1["delta"]["stop_reason"], "max_tokens");
    }

    // --- AM -> RS ----------------------------------------------------------

    #[test]
    fn am_stream_becomes_rs_events() {
        let mut c = StreamConverter::new(AM, RS).unwrap();
        let out = convert_all(
            &mut c,
            &[
                "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-sonnet-4-5\",\"usage\":{\"input_tokens\":8}}}\n\n",
                "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\n",
                "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":3}}\n\n",
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            ],
        );
        assert_eq!(
            kinds(&out),
            vec![
                "response.created",
                "response.in_progress",
                "response.output_item.added",
                "response.content_part.added",
                "response.output_text.delta",
                "response.output_text.done",
                "response.content_part.done",
                "response.output_item.done",
                "response.completed",
            ]
        );
        let events = parsed(&out);
        assert_eq!(events[0].1["response"]["id"], "resp_1");
        assert_eq!(events[0].1["response"]["status"], "in_progress");
        assert_eq!(events[4].1["delta"], "Hi");
        assert_eq!(events[5].1["text"], "Hi");
        assert_eq!(events[8].1["response"]["status"], "completed");
        assert_eq!(
            events[8].1["response"]["usage"],
            json!({"input_tokens": 8, "output_tokens": 3, "total_tokens": 11, "input_tokens_details":{"cached_tokens":0,"cache_creation_tokens":0}})
        );
    }

    #[test]
    fn am_tool_use_stream_becomes_rs_function_call_items() {
        let mut c = StreamConverter::new(AM, RS).unwrap();
        let out = convert_all(
            &mut c,
            &[
                "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"m\"}}\n\n",
                "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"f\"}}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"a\\\"\"}}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\":1}\"}}\n\n",
                "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            ],
        );
        assert_eq!(
            kinds(&out),
            vec![
                "response.created",
                "response.in_progress",
                "response.output_item.added",
                "response.function_call_arguments.delta",
                "response.function_call_arguments.delta",
                "response.function_call_arguments.done",
                "response.output_item.done",
                "response.completed",
            ]
        );
        let events = parsed(&out);
        assert_eq!(
            events[2].1["item"],
            json!({"type": "function_call", "id": "resp_1_fc_0", "call_id": "toolu_1", "name": "f", "arguments": "", "status":"in_progress"})
        );
        // arguments are accumulated into the done item
        assert_eq!(events[5].1["arguments"], "{\"a\":1}");
        assert_eq!(events[6].1["item"]["arguments"], "{\"a\":1}");
        assert_eq!(events[6].1["item"]["status"], "completed");
        assert_eq!(events[7].1["response"]["output"][0], events[6].1["item"]);
        // terminal fallback: message_stop without message_delta still completes
        assert_eq!(events[7].1["response"]["usage"]["output_tokens"], 0);
    }

    // --- framing & passthrough ----------------------------------------------

    #[test]
    fn same_protocol_stream_is_lossless() {
        let mut c = StreamConverter::new(AM, AM).unwrap();
        let event = "event: ping\ndata: {\"type\":\"ping\"}\r\n\r\n";
        assert_eq!(c.push_event(event).unwrap(), vec![event]);
    }

    #[test]
    fn unsupported_stream_pairs_error_at_construction() {
        assert!(matches!(
            StreamConverter::new(CC, RS),
            Err(ConversionError::Unsupported(_, _))
        ));
    }

    #[test]
    fn malformed_stream_events_error_without_panicking() {
        let mut c = StreamConverter::new(CC, AM).unwrap();
        assert!(matches!(
            c.push_event("data: {not json\n\n"),
            Err(ConversionError::InvalidPayload { .. })
        ));
        assert!(matches!(
            c.push_event(""),
            Err(ConversionError::InvalidEvent(_))
        ));
        // payload-less events are ignored, not errors
        assert!(c
            .push_event("event: content_block_stop\n\n")
            .unwrap()
            .is_empty());
    }
}
