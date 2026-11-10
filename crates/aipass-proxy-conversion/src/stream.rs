//! Stateful SSE stream conversion between Anthropic Messages (AM) and the
//! OpenAI wire formats (CC = Chat Completions, RS = Responses).
//!
//! [`ConversionPlugin::convert_stream_event`] is stateless and therefore
//! only passes same-protocol events through; cross-protocol streaming needs
//! per-stream state (open content blocks, tool-call indices, usage), which
//! lives here in [`StreamConverter`]. The proxy constructs one converter
//! per upstream stream and feeds each complete SSE event string to
//! [`StreamConverter::push_event`].

use serde_json::{json, Value};

mod chat_messages;
use chat_messages::{AmToCc, CcToAm};

use crate::response::{am_stop_to_cc_finish, cc_finish_to_am_stop, swap_id_prefix, unix_now};
use crate::{invalid, ConversionError, ProxyProtocol, UsageAccumulator};

use ProxyProtocol::{AnthropicMessages as AM, OpenAiChatCompletions as CC, OpenAiResponses as RS};

/// Converts one SSE stream from `from` to `to`. Same-protocol pairs pass
/// events through unchanged. Feed one complete SSE event (possibly with
/// `event:` and multiple `data:` lines) per call; the result is zero or
/// more complete SSE events in the target protocol.
pub struct StreamConverter {
    state: StreamState,
}

enum StreamState {
    Passthrough { protocol: ProxyProtocol, done: bool },
    CcToAm(CcToAm),
    AmToCc(AmToCc),
    RsToAm(RsToAm),
    AmToRs(AmToRs),
    CcToRs(crate::openai::ChatToResponses),
    RsToCc(crate::openai::ResponsesToChat),
}

impl StreamConverter {
    pub fn new(from: ProxyProtocol, to: ProxyProtocol) -> Result<Self, ConversionError> {
        let state = if from == to {
            StreamState::Passthrough {
                protocol: from,
                done: false,
            }
        } else {
            match (from, to) {
                (CC, AM) => StreamState::CcToAm(CcToAm::default()),
                (AM, CC) => StreamState::AmToCc(AmToCc::default()),
                (RS, AM) => StreamState::RsToAm(RsToAm::default()),
                (AM, RS) => StreamState::AmToRs(AmToRs::default()),
                (CC, RS) => StreamState::CcToRs(crate::openai::ChatToResponses::default()),
                (RS, CC) => StreamState::RsToCc(crate::openai::ResponsesToChat::default()),
                _ => return Err(ConversionError::Unsupported(from, to)),
            }
        };
        Ok(Self { state })
    }

    pub fn push_event(&mut self, event: &str) -> Result<Vec<String>, ConversionError> {
        match &mut self.state {
            StreamState::Passthrough { protocol, done } => {
                let parsed = parse_sse(event)?;
                if !parsed.data.is_empty() && parsed.data != "[DONE]" {
                    let value: Value = serde_json::from_str(&parsed.data)
                        .map_err(|_| invalid(*protocol, "invalid native stream event"))?;
                    if stream_error(event, &value).is_some() {
                        return Err(invalid(*protocol, "upstream generation failed"));
                    }
                    *done |= match protocol {
                        AM => value["type"] == "message_stop",
                        RS => matches!(
                            value["type"].as_str(),
                            Some("response.completed" | "response.incomplete")
                        ),
                        CC => value["choices"].as_array().is_some_and(|choices| {
                            !choices.is_empty()
                                && choices
                                    .iter()
                                    .all(|c| c.get("finish_reason").is_some_and(|v| !v.is_null()))
                        }),
                    };
                }
                Ok(vec![event.to_string()])
            }
            StreamState::CcToAm(state) => state.push(event),
            StreamState::AmToCc(state) => state.push(event),
            StreamState::RsToAm(state) => state.push(event),
            StreamState::AmToRs(state) => state.push(event),
            StreamState::CcToRs(state) => state.push(event),
            StreamState::RsToCc(state) => state.push(event),
        }
    }

    /// Flush a confirmed completion after trailing usage chunks. EOF without
    /// a protocol terminal is an error, never a fabricated successful result.
    pub fn finish(&mut self) -> Result<Vec<String>, ConversionError> {
        match &mut self.state {
            StreamState::Passthrough {
                protocol,
                done: false,
            } => Err(invalid(*protocol, "native stream ended before completion")),
            StreamState::CcToRs(state) => state.finish(),
            StreamState::RsToCc(state) => state.finish(),
            StreamState::CcToAm(state) => state.finish(),
            StreamState::RsToAm(state) => state.finish(),
            StreamState::AmToCc(state) if !state.done => {
                Err(invalid(AM, "stream ended before message_stop"))
            }
            StreamState::AmToRs(state) if !state.terminated => {
                Err(invalid(AM, "stream ended before message_stop"))
            }
            _ => Ok(Vec::new()),
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
    for line in raw
        .strip_prefix('\u{feff}')
        .unwrap_or(raw)
        .split(['\r', '\n'])
    {
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => kind = Some(value.to_string()),
            "data" => data_lines.push(value),
            _ => {}
        }
    }
    // SSE comments, retry/id fields and unknown fields without data are
    // control frames. They must not interrupt a valid generation.
    if raw.is_empty() {
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
    parsed
        .kind
        .clone()
        .filter(|kind| !kind.is_empty() && kind != "message")
        .or_else(|| {
            payload
                .get("type")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

// Null optional error fields are not failures. Conversely, explicit failure
// events must retain an error even when the provider omits its details.
pub(crate) fn stream_error(raw: &str, payload: &Value) -> Option<Value> {
    if let Some(error) = payload
        .get("error")
        .filter(|v| !v.is_null())
        .or_else(|| payload.pointer("/response/error").filter(|v| !v.is_null()))
    {
        return Some(error.clone());
    }
    let parsed = parse_sse(raw).ok()?;
    let failure_kind = |kind: &str| kind == "error" || kind.ends_with(".failed");
    let failed = parsed.kind.as_deref().is_some_and(failure_kind)
        || payload["type"].as_str().is_some_and(failure_kind)
        || payload["status"] == "failed"
        || payload["response"]["status"] == "failed";
    failed.then(|| json!({"type":"upstream_error", "message":payload["message"].as_str().unwrap_or("upstream generation failed")}))
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

// --- RS SSE -> AM SSE ------------------------------------------------------

/// Share Responses identity/parallel-fragment handling with the direct OpenAI
/// converter, then emit Anthropic blocks once each tool's arguments are complete.
#[derive(Default)]
struct RsToAm {
    chat: crate::openai::ResponsesToChat,
    anthropic: CcToAm,
}
impl RsToAm {
    fn push(&mut self, raw: &str) -> Result<Vec<String>, ConversionError> {
        let mut out = Vec::new();
        for event in self.chat.push(raw)? {
            out.extend(self.anthropic.push(&event)?);
        }
        Ok(out)
    }
    fn finish(&mut self) -> Result<Vec<String>, ConversionError> {
        let mut out = Vec::new();
        for event in self.chat.finish()? {
            out.extend(self.anthropic.push(&event)?);
        }
        out.extend(self.anthropic.finish()?);
        Ok(out)
    }
}

// Relays may put initial text/reasoning in a block start instead of a delta.
fn initial_block_delta(payload: &Value) -> Option<Value> {
    if payload["type"] != "content_block_start" {
        return None;
    }
    let block = &payload["content_block"];
    let (field, kind) = match block["type"].as_str()? {
        "text" => ("text", "text_delta"),
        "thinking" => ("thinking", "thinking_delta"),
        _ => return None,
    };
    let text = block[field].as_str().filter(|s| !s.is_empty())?;
    Some(
        json!({"type":"content_block_delta","index":payload["index"],"delta":{"type":kind,field:text}}),
    )
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
    reasoning_item: Option<(u64, String, String)>,
    /// Open function_call item: (output_index, item, accumulated arguments).
    tool_item: Option<(u64, Value, String)>,
    usage: UsageAccumulator,
    output: Vec<Value>,
    stop_reason: String,
    sequence: u64,
    terminated: bool,
}

impl AmToRs {
    fn push(&mut self, raw: &str) -> Result<Vec<String>, ConversionError> {
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
        if self.terminated {
            return Ok(out);
        }
        self.usage.observe(AM, &payload);
        match kind.as_str() {
            "error" => {
                // Forward upstream failures as an RS error + response.failed;
                // dropping them would leave the client waiting on a stream
                // that never completes.
                self.finalize_open(&mut out);
                let error = error.expect("error events have normalized details");
                out.push(emit_typed(
                    "error",
                    &json!({
                        "type": "error",
                        "code": error.get("type").cloned().unwrap_or(Value::Null),
                        "message": error.get("message").cloned().unwrap_or(Value::Null),
                        "param": Value::Null,
                    }),
                ));
                let mut failed = self.response_envelope("response.failed", "failed");
                failed["response"]["error"] = error;
                out.push(emit_typed("response.failed", &failed));
                self.terminated = true;
            }
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
                crate::response::validate_am_block(&block, RS)?;
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
                            "call_id": block.get("id").cloned().unwrap_or_else(|| json!(format!("call_conv_{index}"))),
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
                    Some("thinking_delta") => {
                        let text = delta.get("thinking").and_then(Value::as_str).unwrap_or("");
                        if !text.is_empty() {
                            if self.reasoning_item.is_none() {
                                self.finalize_open(&mut out);
                                let index = self.next_output;
                                self.next_output += 1;
                                let id = format!("{}_rs_{index}", self.id);
                                out.push(emit_typed("response.output_item.added", &json!({"type":"response.output_item.added","output_index":index,"item":{"type":"reasoning","id":id,"summary":[]}})));
                                out.push(emit_typed("response.reasoning_summary_part.added", &json!({"type":"response.reasoning_summary_part.added","output_index":index,"item_id":id,"summary_index":0,"part":{"type":"summary_text","text":""}})));
                                self.reasoning_item = Some((index, id, String::new()));
                            }
                            if let Some((index, id, acc)) = &mut self.reasoning_item {
                                acc.push_str(text);
                                out.push(emit_typed("response.reasoning_summary_text.delta", &json!({"type":"response.reasoning_summary_text.delta","output_index":index,"item_id":id,"summary_index":0,"delta":text})));
                            }
                        }
                    }
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
                if let Some(reason) = payload
                    .pointer("/delta/stop_reason")
                    .and_then(Value::as_str)
                {
                    self.stop_reason = reason.to_owned();
                }
            }
            "message_stop" => {
                self.finalize_open(&mut out);
                let (kind, status) = if self.stop_reason == "max_tokens" {
                    ("response.incomplete", "incomplete")
                } else {
                    ("response.completed", "completed")
                };
                out.push(emit_typed(kind, &self.response_envelope(kind, status)));
                self.terminated = true;
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
        if let Some(delta) = initial_block_delta(&payload) {
            out.extend(self.push(&emit_typed("content_block_delta", &delta))?);
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
            response["usage"] = crate::response::am_usage_to_openai(
                Some(self.usage.wire()),
                "input_tokens",
                "output_tokens",
                "input_tokens_details",
            );
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
        if let Some((index, id, text)) = self.reasoning_item.take() {
            let part = json!({"type":"summary_text","text":text});
            let item = json!({"type":"reasoning","id":id,"summary":[part]});
            out.push(emit_typed("response.reasoning_summary_text.done", &json!({"type":"response.reasoning_summary_text.done","output_index":index,"item_id":id,"summary_index":0,"text":text})));
            out.push(emit_typed("response.reasoning_summary_part.done", &json!({"type":"response.reasoning_summary_part.done","output_index":index,"item_id":id,"summary_index":0,"part":part})));
            out.push(emit_typed(
                "response.output_item.done",
                &json!({"type":"response.output_item.done","output_index":index,"item":item}),
            ));
            self.output.push(item);
        }
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
mod tests;
