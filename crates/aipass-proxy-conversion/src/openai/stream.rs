use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;

fn data(raw: &str) -> Result<Option<Value>, ConversionError> {
    let text = raw
        .strip_prefix('\u{feff}')
        .unwrap_or(raw)
        .split(['\r', '\n'])
        .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
        .collect::<Vec<_>>()
        .join("\n");
    if text.trim().is_empty() {
        return Ok(None);
    }
    if text.trim() == "[DONE]" {
        return Ok(Some(json!("[DONE]")));
    }
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|_| invalid(CC, "invalid stream JSON"))
}

fn event(mut value: Value, sequence: &mut u64) -> String {
    value["sequence_number"] = json!(*sequence);
    *sequence += 1;
    format!("event: {}\ndata: {value}\n\n", string(value.get("type")))
}

fn chunk(value: Value) -> String {
    format!("data: {value}\n\n")
}

pub(super) fn unique_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!(
        "conv_{}_{}",
        crate::response::unix_now(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

#[derive(Default)]
pub(crate) struct ChatToResponses {
    id: String,
    model: Value,
    created: Value,
    output: Vec<Value>,
    tools: HashMap<u64, usize>,
    tool_identities: HashMap<u64, String>,
    text: Option<usize>,
    reasoning: Option<usize>,
    refusal: Option<usize>,
    usage: crate::UsageAccumulator,
    finish_reason: Option<String>,
    choice: crate::chat::Choice,
    sequence: u64,
    done: bool,
}

impl ChatToResponses {
    fn emit(&mut self, value: Value, out: &mut Vec<String>) {
        out.push(event(value, &mut self.sequence));
    }

    fn start(&mut self, payload: &Value, out: &mut Vec<String>) {
        if !self.id.is_empty() {
            return;
        }
        let source = payload
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(unique_id);
        self.id = crate::response::swap_id_prefix(&source, "chatcmpl-", "resp_");
        self.model = payload["model"].clone();
        self.created = payload
            .get("created")
            .cloned()
            .unwrap_or_else(|| json!(crate::response::unix_now()));
        for kind in ["response.created", "response.in_progress"] {
            self.emit(json!({"type":kind, "response":{"id":self.id, "object":"response", "model":self.model, "created_at":self.created, "status":"in_progress", "output":[]}}), out);
        }
    }

    fn add(&mut self, item: Value, out: &mut Vec<String>) -> usize {
        let index = self.output.len();
        self.emit(
            json!({"type":"response.output_item.added", "output_index":index, "item":item}),
            out,
        );
        self.output.push(item);
        index
    }

    fn text(&mut self, text: &str, thinking: bool, refusal: bool, out: &mut Vec<String>) {
        if text.is_empty() {
            return;
        }
        let slot = if thinking {
            self.reasoning
        } else if refusal {
            self.refusal
        } else {
            self.text
        };
        let index = slot.unwrap_or_else(|| {
            let id = format!("{}_item_{}", self.id, self.output.len());
            let item = if thinking { json!({"id":id, "type":"reasoning", "summary":[]}) }
                else { json!({"id":id, "type":"message", "role":"assistant", "status":"in_progress", "content":[]}) };
            let index = self.add(item, out);
            let part = if thinking { json!({"type":"summary_text", "text":""}) }
                else if refusal { json!({"type":"refusal", "refusal":""}) }
                else { json!({"type":"output_text", "text":"", "annotations":[]}) };
            let kind = if thinking { "response.reasoning_summary_part.added" } else { "response.content_part.added" };
            let mut added = json!({"type":kind, "item_id":id, "output_index":index, "part":part});
            added[if thinking { "summary_index" } else { "content_index" }] = json!(0);
            self.emit(added, out);
            self.output[index][if thinking { "summary" } else { "content" }] = json!([part]);
            if thinking { self.reasoning = Some(index); } else if refusal { self.refusal = Some(index); } else { self.text = Some(index); }
            index
        });
        let list = if thinking { "summary" } else { "content" };
        let field = if refusal { "refusal" } else { "text" };
        let previous = string(self.output[index][list][0].get(field));
        self.output[index][list][0][field] = json!(format!("{previous}{text}"));
        let kind = if thinking {
            "response.reasoning_summary_text.delta"
        } else if refusal {
            "response.refusal.delta"
        } else {
            "response.output_text.delta"
        };
        let mut delta = json!({"type":kind, "item_id":self.output[index]["id"], "output_index":index, "delta":text});
        delta[if thinking {
            "summary_index"
        } else {
            "content_index"
        }] = json!(0);
        self.emit(delta, out);
    }

    pub(crate) fn push(&mut self, raw: &str) -> Result<Vec<String>, ConversionError> {
        if self.done {
            return Ok(Vec::new());
        }
        let Some(payload) = data(raw)? else {
            return Ok(Vec::new());
        };
        if payload == "[DONE]" {
            if self.id.is_empty() {
                return Err(invalid(CC, "empty upstream stream"));
            }
            self.finish_reason.get_or_insert_with(|| {
                if self.tools.is_empty() {
                    "stop".into()
                } else {
                    "tool_calls".into()
                }
            });
            return self.finish();
        }
        let mut out = Vec::new();
        self.start(&payload, &mut out);
        if let Some(error) = crate::stream::stream_error(raw, &payload) {
            self.emit(
                json!({"type":"error", "error":error, "message":error["message"]}),
                &mut out,
            );
            self.emit(json!({"type":"response.failed", "response":{"id":self.id, "object":"response", "status":"failed", "error":error, "output":self.output}}), &mut out);
            self.done = true;
            return Ok(out);
        }
        if let Some(usage) = payload.get("usage").filter(|v| !v.is_null()) {
            self.usage.update(usage);
        }
        let Some(choices) = payload.get("choices").and_then(Value::as_array) else {
            return Ok(out);
        };
        if let Some(choice) = self.choice.select(choices) {
            let delta = &choice["delta"];
            crate::response::validate_chat_content(delta, RS)?;
            self.text(
                string(
                    delta
                        .get("reasoning_content")
                        .filter(|v| v.as_str().is_some_and(|s| !s.is_empty()))
                        .or_else(|| delta.get("reasoning")),
                ),
                true,
                false,
                &mut out,
            );
            match delta.get("content") {
                Some(Value::String(text)) => self.text(text, false, false, &mut out),
                Some(Value::Array(parts)) => {
                    for part in parts {
                        match string(part.get("type")) {
                            "text" => self.text(string(part.get("text")), false, false, &mut out),
                            "thinking" => {
                                if let Some(parts) = part.get("thinking").and_then(Value::as_array)
                                {
                                    for part in parts {
                                        self.text(string(part.get("text")), true, false, &mut out);
                                    }
                                } else {
                                    self.text(string(part.get("thinking")), true, false, &mut out);
                                }
                            }
                            _ => return Err(invalid(CC, "unsupported streamed content part")),
                        }
                    }
                }
                _ => {}
            }
            self.text(string(delta.get("refusal")), false, true, &mut out);
            if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                for (position, call) in calls.iter().enumerate() {
                    let key = crate::chat::tool_index(call, position)?;
                    let index = if let Some(index) = self.tools.get(&key) {
                        *index
                    } else {
                        let custom = call["type"] == "custom" || call.get("custom").is_some();
                        let field = if custom { "custom" } else { "function" };
                        let call_id = call
                            .get("id")
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned)
                            .unwrap_or_else(|| format!("call_{}_{}", self.id, key));
                        let mut item = json!({"id":format!("{}_call_{key}", self.id), "type":if custom { "custom_tool_call" } else { "function_call" }, "call_id":call_id, "name":call[field]["name"], "status":"in_progress"});
                        item[if custom { "input" } else { "arguments" }] = json!("");
                        let index = self.add(item, &mut out);
                        self.tools.insert(key, index);
                        index
                    };
                    if let Some(id) = call["id"].as_str().filter(|id| !id.is_empty()) {
                        let identity = self.tool_identities.entry(key).or_default();
                        crate::chat::identity(identity, id);
                        // The final item owns the complete identity, including
                        // metadata first supplied after the added event.
                        self.output[index]["call_id"] = json!(identity);
                    }
                    let custom = self.output[index]["type"] == "custom_tool_call";
                    let field = if custom { "custom" } else { "function" };
                    if let Some(name) = call[field].get("name").filter(|v| v.as_str() != Some("")) {
                        let mut previous = string(self.output[index].get("name")).to_owned();
                        crate::chat::identity(&mut previous, string(Some(name)));
                        self.output[index]["name"] = json!(previous);
                    }
                    let arg = if custom { "input" } else { "arguments" };
                    if let Some(value) = call[field].get(arg).filter(|v| !v.is_null()) {
                        let text = value
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| value.to_string());
                        let previous = string(self.output[index].get(arg));
                        self.output[index][arg] = json!(format!("{previous}{text}"));
                        let kind = if custom {
                            "response.custom_tool_call_input.delta"
                        } else {
                            "response.function_call_arguments.delta"
                        };
                        self.emit(json!({"type":kind, "output_index":index, "item_id":self.output[index]["id"], "delta":text}), &mut out);
                    }
                }
            }
            if let Some(finish) = choice.get("finish_reason").and_then(Value::as_str) {
                self.finish_reason = Some(finish.into());
            }
            // Translation has one choice, even when an upstream ignored n=1.
        }
        Ok(out)
    }

    pub(crate) fn finish(&mut self) -> Result<Vec<String>, ConversionError> {
        if self.done {
            return Ok(Vec::new());
        }
        let Some(reason) = self.finish_reason.clone() else {
            return Err(invalid(CC, "stream ended before completion"));
        };
        let mut out = Vec::new();
        for index in 0..self.output.len() {
            let mut item = self.output[index].clone();
            let kind = string(item.get("type")).to_owned();
            if matches!(kind.as_str(), "function_call" | "custom_tool_call") {
                let field = if kind == "function_call" {
                    "arguments"
                } else {
                    "input"
                };
                if field == "arguments" && item[field] == "" {
                    item[field] = json!("{}");
                }
                let mut done = json!({"type":if field == "arguments" { "response.function_call_arguments.done" } else { "response.custom_tool_call_input.done" }, "output_index":index, "item_id":item["id"]});
                done[field] = item[field].clone();
                self.emit(done, &mut out);
            } else {
                let thinking = kind == "reasoning";
                let part = &item[if thinking { "summary" } else { "content" }][0];
                let refusal = part["type"] == "refusal";
                let field = if refusal { "refusal" } else { "text" };
                let mut done = json!({"type":if thinking { "response.reasoning_summary_text.done" } else if refusal { "response.refusal.done" } else { "response.output_text.done" }, "item_id":item["id"], "output_index":index});
                done[if thinking {
                    "summary_index"
                } else {
                    "content_index"
                }] = json!(0);
                done[field] = part[field].clone();
                self.emit(done, &mut out);
                let mut done = json!({"type":if thinking { "response.reasoning_summary_part.done" } else { "response.content_part.done" }, "item_id":item["id"], "output_index":index, "part":part});
                done[if thinking {
                    "summary_index"
                } else {
                    "content_index"
                }] = json!(0);
                self.emit(done, &mut out);
            }
            item["status"] = json!("completed");
            self.output[index] = item.clone();
            self.emit(
                json!({"type":"response.output_item.done", "output_index":index, "item":item}),
                &mut out,
            );
        }
        let incomplete = matches!(reason.as_str(), "length" | "content_filter");
        let mut response = json!({"id":self.id, "object":"response", "model":self.model, "created_at":self.created, "status":if incomplete { "incomplete" } else { "completed" }, "output":self.output, "usage":usage(self.usage.wire(), RS)});
        if incomplete {
            response["incomplete_details"] = json!({"reason":if reason == "length" { "max_output_tokens" } else { "content_filter" }});
        }
        self.emit(json!({"type":if incomplete { "response.incomplete" } else { "response.completed" }, "response":response}), &mut out);
        self.done = true;
        Ok(out)
    }
}

#[derive(Default)]
pub(crate) struct ResponsesToChat {
    id: String,
    model: Value,
    created: Value,
    tools: HashMap<u64, (usize, bool)>,
    emitted: HashMap<(u64, String), String>,
    done: bool,
}

impl ResponsesToChat {
    fn chunk(&self, delta: Value, finish: Value, usage: Option<Value>) -> String {
        let mut value = json!({"id":self.id, "object":"chat.completion.chunk", "created":self.created, "model":self.model, "choices":[{"index":0, "delta":delta, "finish_reason":finish}]});
        if let Some(usage) = usage {
            value["usage"] = usage;
        }
        chunk(value)
    }

    // Snapshots may complete a partial delta sequence. Only append the unseen
    // suffix; a different prefix cannot be repaired after bytes were delivered.
    fn tail(&self, key: &(u64, String), snapshot: &str) -> Result<String, ConversionError> {
        let previous = self.emitted.get(key).map(String::as_str).unwrap_or("");
        if let Some(tail) = snapshot.strip_prefix(previous) {
            return Ok(tail.to_owned());
        }
        if previous.starts_with(snapshot) {
            return Ok(String::new());
        }
        Err(invalid(
            RS,
            "output snapshot conflicts with delivered deltas",
        ))
    }
    fn text_snapshot(
        &mut self,
        index: u64,
        part: u64,
        field: &str,
        text: &str,
        out: &mut Vec<String>,
    ) -> Result<(), ConversionError> {
        let key = (index, format!("{field}:{part}"));
        let tail = self.tail(&key, text)?;
        if !tail.is_empty() {
            self.emitted.entry(key).or_default().push_str(&tail);
            out.push(self.chunk(json!({field:tail}), Value::Null, None));
        }
        Ok(())
    }
    fn content_snapshot(
        &mut self,
        index: u64,
        part_index: u64,
        part: &Value,
        out: &mut Vec<String>,
    ) -> Result<(), ConversionError> {
        let (field, key) = match string(part.get("type")) {
            "output_text" => ("content", "text"),
            "refusal" => ("refusal", "refusal"),
            "summary_text" => ("reasoning_content", "text"),
            _ => return Err(invalid(RS, "unsupported streamed content part")),
        };
        self.text_snapshot(index, part_index, field, string(part.get(key)), out)
    }
    fn item(
        &mut self,
        index: u64,
        item: &Value,
        out: &mut Vec<String>,
    ) -> Result<(), ConversionError> {
        match string(item.get("type")) {
            "function_call" | "custom_tool_call" => {
                let custom = item["type"] == "custom_tool_call";
                let tool_index = self.tools.len();
                if let std::collections::hash_map::Entry::Vacant(slot) = self.tools.entry(index) {
                    slot.insert((tool_index, custom));
                    let mut call = call_to_chat(item)?;
                    call["index"] = json!(tool_index);
                    let field = if custom { "custom" } else { "function" };
                    call[field][if custom { "input" } else { "arguments" }] = json!("");
                    out.push(self.chunk(json!({"tool_calls":[call]}), Value::Null, None));
                }
                let field = if custom { "input" } else { "arguments" };
                let tail = self.tail(&(index, field.into()), string(item.get(field)))?;
                self.tool_delta(index, &tail, out)?;
            }
            "message" => {
                if let Some(parts) = item.get("content").and_then(Value::as_array) {
                    for (part_index, part) in parts.iter().enumerate() {
                        self.content_snapshot(index, part_index as u64, part, out)?;
                    }
                }
            }
            "reasoning" => {
                if let Some(parts) = item.get("summary").and_then(Value::as_array) {
                    for (part_index, part) in parts.iter().enumerate() {
                        self.text_snapshot(
                            index,
                            part_index as u64,
                            "reasoning_content",
                            string(part.get("text")),
                            out,
                        )?;
                    }
                }
            }
            _ => return Err(invalid(RS, "unsupported streamed output item")),
        }
        Ok(())
    }

    fn tool_delta(
        &mut self,
        index: u64,
        text: &str,
        out: &mut Vec<String>,
    ) -> Result<(), ConversionError> {
        if text.is_empty() {
            return Ok(());
        }
        let (tool_index, custom) = self
            .tools
            .get(&index)
            .copied()
            .ok_or_else(|| invalid(RS, "tool arguments arrived before the tool identity"))?;
        let field = if custom { "custom" } else { "function" };
        let arg = if custom { "input" } else { "arguments" };
        self.emitted
            .entry((index, arg.into()))
            .or_default()
            .push_str(text);
        out.push(self.chunk(
            json!({"tool_calls":[{"index":tool_index, field:{arg:text}}]}),
            Value::Null,
            None,
        ));
        Ok(())
    }

    pub(crate) fn push(&mut self, raw: &str) -> Result<Vec<String>, ConversionError> {
        if self.done {
            return Ok(Vec::new());
        }
        let Some(payload) = data(raw)? else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        if let Some(error) = crate::stream::stream_error(raw, &payload) {
            self.done = true;
            return Ok(vec![
                chunk(json!({"error":error})),
                "data: [DONE]\n\n".into(),
            ]);
        }
        if let Some(response) = payload.get("response") {
            if self.id.is_empty() {
                let source = response
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(unique_id);
                self.id = crate::response::swap_id_prefix(&source, "resp_", "chatcmpl-");
                self.model = response["model"].clone();
                self.created = response
                    .get("created_at")
                    .cloned()
                    .unwrap_or_else(|| json!(crate::response::unix_now()));
                out.push(self.chunk(json!({"role":"assistant", "content":""}), Value::Null, None));
            }
        }
        let index = crate::number(payload.get("output_index"));
        match string(payload.get("type")) {
            "response.output_item.added" | "response.output_item.done" => {
                self.item(index, &payload["item"], &mut out)?
            }
            "response.output_text.delta"
            | "response.refusal.delta"
            | "response.reasoning_summary_text.delta" => {
                if string(payload.get("delta")).is_empty() {
                    return Ok(out);
                }
                let kind = string(payload.get("type"));
                let field = if kind == "response.output_text.delta" {
                    "content"
                } else if kind == "response.refusal.delta" {
                    "refusal"
                } else {
                    "reasoning_content"
                };
                let part = crate::number(payload.get(if field == "reasoning_content" {
                    "summary_index"
                } else {
                    "content_index"
                }));
                self.emitted
                    .entry((index, format!("{field}:{part}")))
                    .or_default()
                    .push_str(string(payload.get("delta")));
                out.push(self.chunk(json!({field:payload["delta"]}), Value::Null, None));
            }
            "response.function_call_arguments.delta" | "response.custom_tool_call_input.delta" => {
                self.tool_delta(index, string(payload.get("delta")), &mut out)?
            }
            "response.content_part.added" | "response.content_part.done" => {
                self.content_snapshot(
                    index,
                    crate::number(payload.get("content_index")),
                    &payload["part"],
                    &mut out,
                )?;
            }
            "response.reasoning_summary_part.added" | "response.reasoning_summary_part.done" => {
                self.text_snapshot(
                    index,
                    crate::number(payload.get("summary_index")),
                    "reasoning_content",
                    string(payload["part"].get("text")),
                    &mut out,
                )?;
            }
            "response.output_text.done"
            | "response.refusal.done"
            | "response.reasoning_summary_text.done" => {
                let (field, key, part) = match string(payload.get("type")) {
                    "response.refusal.done" => ("refusal", "refusal", "content_index"),
                    "response.reasoning_summary_text.done" => {
                        ("reasoning_content", "text", "summary_index")
                    }
                    _ => ("content", "text", "content_index"),
                };
                self.text_snapshot(
                    index,
                    crate::number(payload.get(part)),
                    field,
                    string(payload.get(key)),
                    &mut out,
                )?;
            }
            "response.function_call_arguments.done" | "response.custom_tool_call_input.done" => {
                let field = if payload["type"] == "response.custom_tool_call_input.done" {
                    "input"
                } else {
                    "arguments"
                };
                let tail = self.tail(&(index, field.into()), string(payload.get(field)))?;
                self.tool_delta(index, &tail, &mut out)?;
            }
            "response.completed" | "response.incomplete" => {
                let response = &payload["response"];
                if let Some(items) = response.get("output").and_then(Value::as_array) {
                    for (index, item) in items.iter().enumerate() {
                        self.item(index as u64, item, &mut out)?;
                    }
                }
                let finish = if response["status"] == "incomplete"
                    || payload["type"] == "response.incomplete"
                {
                    if response["incomplete_details"]["reason"] == "content_filter" {
                        "content_filter"
                    } else {
                        "length"
                    }
                } else if self.tools.is_empty() {
                    "stop"
                } else {
                    "tool_calls"
                };
                out.push(self.chunk(
                    json!({}),
                    json!(finish),
                    Some(usage(&response["usage"], CC)),
                ));
                out.push("data: [DONE]\n\n".into());
                self.done = true;
            }
            _ => {}
        }
        Ok(out)
    }

    pub(crate) fn finish(&mut self) -> Result<Vec<String>, ConversionError> {
        if self.done {
            Ok(Vec::new())
        } else {
            Err(invalid(RS, "stream ended before completion"))
        }
    }
}
