//! Claude event decoding, tool collection and streamed response lifecycle.
use super::*;

pub(super) struct SegmentStream {
    pub(super) receiver: tokio::sync::mpsc::Receiver<(Bytes, bool)>,
    pub(super) inner: Weak<Inner>,
    pub(super) key: String,
    pub(super) segment: Uuid,
    pub(super) complete: bool,
}
impl Stream for SegmentStream {
    type Item = Result<Bytes, Box<dyn std::error::Error + Send + Sync>>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.complete {
            return Poll::Ready(None);
        }
        match self.receiver.poll_recv(cx) {
            Poll::Ready(Some((bytes, complete))) => {
                self.complete = complete;
                Poll::Ready(Some(Ok(bytes)))
            }
            Poll::Ready(None) => {
                self.complete = true;
                Poll::Ready(Some(Err(std::io::Error::other(
                    "Claude process ended before completion",
                )
                .into())))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
impl Drop for SegmentStream {
    fn drop(&mut self) {
        if !self.complete {
            if let Some(inner) = self.inner.upgrade() {
                if let Ok(mut runs) = inner.runs.lock() {
                    if runs
                        .get(&self.key)
                        .is_some_and(|run| run.segment == self.segment)
                    {
                        runs.remove(&self.key);
                    }
                }
            }
        }
    }
}
#[derive(Default)]
pub(super) struct Collector {
    pub(super) message: Value,
    pub(super) complete: bool,
    bytes: usize,
}
impl Collector {
    fn event(&mut self, event: &Value) -> Result<(), String> {
        self.bytes = self.bytes.saturating_add(event.to_string().len());
        if self.bytes > 64 * 1024 * 1024 {
            return Err("Claude response exceeds bridge limit".into());
        }
        match event["type"].as_str() {
            Some("message_start") => {
                if !event["message"].is_object()
                    || event["message"]
                        .get("usage")
                        .is_some_and(|v| !v.is_null() && !v.is_object())
                {
                    return Err("invalid Claude message envelope".into());
                }
                self.message = event["message"].clone();
                self.message["content"] = json!([]);
            }
            Some("content_block_start") => {
                if !event["content_block"].is_object()
                    || !event["content_block"]["type"].is_string()
                {
                    return Err("invalid Claude content block".into());
                }
                let i = event["index"]
                    .as_u64()
                    .ok_or("invalid Claude block index")? as usize;
                if i > 4096 {
                    return Err("too many Claude blocks".into());
                }
                let content = self.message["content"]
                    .as_array_mut()
                    .ok_or("Claude block before message start")?;
                while content.len() <= i {
                    content.push(Value::Null);
                }
                content[i] = event["content_block"].clone();
            }
            Some("content_block_delta") => {
                let i = event["index"]
                    .as_u64()
                    .ok_or("invalid Claude block index")? as usize;
                let block = self.message["content"]
                    .as_array_mut()
                    .and_then(|v| v.get_mut(i))
                    .filter(|v| v.is_object())
                    .ok_or("unknown Claude block")?;
                let delta = &event["delta"];
                if delta["type"] == "citations_delta" {
                    if block.get("citations").is_none_or(Value::is_null) {
                        block["citations"] = json!([]);
                    }
                    block["citations"]
                        .as_array_mut()
                        .ok_or("invalid Claude citations")?
                        .push(delta["citation"].clone());
                    return Ok(());
                }
                let (field, source) = match delta["type"].as_str() {
                    Some("text_delta") => ("text", "text"),
                    Some("thinking_delta") => ("thinking", "thinking"),
                    Some("signature_delta") => ("signature", "signature"),
                    Some("input_json_delta") => ("_arguments", "partial_json"),
                    _ => return Ok(()),
                };
                let mut value = block[field].as_str().unwrap_or("").to_owned();
                value.push_str(
                    delta[source]
                        .as_str()
                        .ok_or("invalid Claude content delta")?,
                );
                block[field] = json!(value);
            }
            Some("content_block_stop") => {
                let i = event["index"]
                    .as_u64()
                    .ok_or("invalid Claude block index")? as usize;
                let block = self.message["content"]
                    .as_array_mut()
                    .and_then(|v| v.get_mut(i))
                    .filter(|v| v.is_object())
                    .ok_or("unknown Claude block")?;
                if let Some(args) = block.get("_arguments").and_then(Value::as_str) {
                    block["input"] =
                        serde_json::from_str(args).map_err(|_| "invalid Claude tool arguments")?;
                    block.as_object_mut().unwrap().remove("_arguments");
                }
            }
            Some("message_delta") => {
                if !self.message.is_object()
                    || event
                        .get("usage")
                        .is_some_and(|v| !v.is_null() && !v.is_object())
                {
                    return Err("invalid Claude usage envelope".into());
                }
                self.message["stop_reason"] = event["delta"]["stop_reason"].clone();
                if let Some(usage) = event["usage"].as_object() {
                    for (k, v) in usage {
                        self.message["usage"][k] = v.clone();
                    }
                }
            }
            Some("message_stop") => {
                if !self.message["content"].is_array() {
                    return Err("Claude stopped before message start".into());
                }
                self.complete = true;
            }
            Some("error") => return Err("Claude reported an upstream error".into()),
            _ => {}
        }
        Ok(())
    }
    pub(super) fn push(&mut self, bytes: &[u8]) -> Result<(), String> {
        let text = std::str::from_utf8(bytes).map_err(|_| "invalid Claude stream encoding")?;
        for line in text.lines().filter_map(|l| l.strip_prefix("data: ")) {
            self.event(&serde_json::from_str::<Value>(line).map_err(|_| "invalid Claude event")?)?;
        }
        Ok(())
    }
}
pub(super) fn read_events(
    output: std::sync::mpsc::Receiver<Value>,
    inner: Weak<Inner>,
    key: String,
    native_tools: bool,
) {
    let mut collector = Collector::default();
    let mut native = super::native_tools::Events::default();
    let mut tool_records = HashMap::new();
    'messages: for rpc in output {
        let events = match rpc["method"].as_str() {
            Some("_aipass/closed" | "_aipass/request_cancelled") => break,
            Some("session/request_permission" | "_aipass/request_input") => {
                if !interactions::await_receiver(&inner, &key) {
                    break;
                }
                native.permission(rpc)
            }
            Some("_aipass/child_tools") => {
                if !interactions::await_receiver(&inner, &key) {
                    break;
                }
                native.caller_tools(
                    rpc["params"]["calls"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default(),
                )
            }
            Some("session/update") if native_tools => {
                let update = &rpc["params"]["update"];
                let id = update["toolCallId"].as_str().unwrap_or("");
                match update["sessionUpdate"].as_str() {
                    Some("tool_call")
                        if !update["title"]
                            .as_str()
                            .is_some_and(|n| n.starts_with("mcp__aipass__")) =>
                    {
                        if tool_records.len() >= 4096 {
                            break;
                        }
                        tool_records.insert(id.to_owned(), update.clone());
                        Vec::new()
                    }
                    Some("tool_call_update") => {
                        if let Some(call) = tool_records.remove(id) {
                            // Text records survive all three model API conversions
                            // and can be handed to another provider without rerunning tools.
                            native.activity(format!("Claude Code tool activity (already executed): {}", json!({"tool":call["title"],"input":call["rawInput"],"status":update["status"],"result":update["rawOutput"]})))
                        } else {
                            Vec::new()
                        }
                    }
                    _ => Vec::new(),
                }
            }
            Some("_aipass/native_event") => {
                let value = &rpc["params"]["event"];
                match value["type"].as_str() {
                    Some("system") if value["subtype"] == "init" => {
                        if let Some(session) = value["session_id"]
                            .as_str()
                            .and_then(|s| Uuid::parse_str(s).ok())
                        {
                            if let Some(inner) = inner.upgrade() {
                                if let Ok(mut runs) = inner.runs.lock() {
                                    if let Some(run) = runs.get_mut(&key) {
                                        run.native_session = session;
                                    }
                                }
                            }
                        }
                        Vec::new()
                    }
                    Some("rate_limit_event") => {
                        if let Some(inner) = inner.upgrade() {
                            super::feedback::observe(&inner, &key, &value["rate_limit_info"]);
                        }
                        Vec::new()
                    }
                    Some("system") if value["subtype"] == "local_command_output" => {
                        native.command_output(value["content"].as_str().unwrap_or("").into())
                    }
                    Some("system") if native_tools && value["subtype"] == "task_notification" => {
                        native.activity(format!("Claude Code background task (already executed): {}", json!({"taskId":value["task_id"],"status":value["status"],"summary":value["summary"],"outputFile":value["output_file"]})))
                    }
                    Some("result") if value["parent_tool_use_id"].is_null() && rpc["params"]["autonomous"] == true => {
                        native.autonomous_result(value)
                    }
                    Some("result") if value["parent_tool_use_id"].is_null() => {
                        if value["is_error"] == true {
                            break;
                        }
                        if !interactions::await_receiver(&inner, &key) {
                            break;
                        }
                        let events = native.result(value);
                        for event in events {
                            if !deliver(&inner, &key, event, &mut collector) {
                                break 'messages;
                            }
                        }
                        if let Some(inner) = inner.upgrade() {
                            super::checkpoint::save(&inner, &key);
                        }
                        Vec::new()
                    }
                    Some("stream_event") if value["parent_tool_use_id"].is_null() => {
                        if value["event"]["type"] == "message_start" {
                            if let Some(inner) = inner.upgrade() {
                                if let Ok(mut runs) = inner.runs.lock() {
                                    if let Some(run) = runs.get_mut(&key) {
                                        run.native_message = value["event"]["message"]["id"]
                                            .as_str()
                                            .map(str::to_owned);
                                    }
                                }
                            }
                        }
                        match native.adapt(value["event"].clone()) {
                            Ok(event) => event.into_iter().collect(),
                            Err(_) => break,
                        }
                    }
                    _ => Vec::new(),
                }
            }
            _ if rpc.get("error").is_some() => break,
            _ => Vec::new(),
        };
        if !events.is_empty() && !interactions::await_receiver(&inner, &key) {
            break;
        }
        for event in events {
            if !deliver(&inner, &key, event, &mut collector) {
                break 'messages;
            }
        }
    }
    if let Some(inner) = inner.upgrade() {
        if let Ok(mut runs) = inner.runs.lock() {
            runs.remove(&key);
        }
    }
}
fn deliver(inner: &Weak<Inner>, key: &str, mut event: Value, collector: &mut Collector) -> bool {
    // Synthesized ACP activity and complete assistant frames contain populated
    // blocks. Model API streams expect empty starts followed by deltas; emitting
    // only a populated start loses text/tool arguments in OpenAI converters.
    let mut deltas = Vec::new();
    if event["type"] == "content_block_start" {
        let index = event["index"].clone();
        let block = &mut event["content_block"];
        let fields: &[(&str, &str, &str)] = match block["type"].as_str() {
            Some("text") => &[("text", "text_delta", "text")],
            Some("thinking") => &[
                ("thinking", "thinking_delta", "thinking"),
                ("signature", "signature_delta", "signature"),
            ],
            Some("tool_use") => &[("input", "input_json_delta", "partial_json")],
            _ => &[],
        };
        for (field, kind, output) in fields {
            if let Some(value) = block.get_mut(*field) {
                let text = if *field == "input" {
                    let text = if *value == json!({}) {
                        String::new()
                    } else {
                        value.to_string()
                    };
                    *value = json!({});
                    text
                } else {
                    let text = value.as_str().unwrap_or("").to_owned();
                    *value = json!("");
                    text
                };
                if !text.is_empty() {
                    deltas.push(json!({"type":"content_block_delta","index":index,"delta":{"type":kind,*output:text}}));
                }
            }
        }
    }
    if !deliver_event(inner, key, event, collector) {
        return false;
    }
    deltas
        .into_iter()
        .all(|delta| deliver_event(inner, key, delta, collector))
}
fn deliver_event(
    inner: &Weak<Inner>,
    key: &str,
    mut event: Value,
    collector: &mut Collector,
) -> bool {
    if let Some(name) = event
        .pointer("/content_block/name")
        .and_then(Value::as_str)
        .and_then(|n| n.strip_prefix("mcp__aipass__"))
    {
        event["content_block"]["name"] = json!(name);
    }
    if event["type"] == "content_block_start" && event["content_block"]["type"] == "tool_use" {
        event["content_block"]["id"] =
            json!(format!("toolu_aipass_claude_{}", Uuid::new_v4().simple()));
    }
    if event["type"] == "message_start" {
        *collector = Collector::default();
    }
    if collector.event(&event).is_err() {
        return false;
    }
    let Some(inner) = inner.upgrade() else {
        return false;
    };
    let (sender, segment) = {
        let Ok(mut runs) = inner.runs.lock() else {
            return false;
        };
        let Some(run) = runs.get_mut(key) else {
            return false;
        };
        if event["type"] == "content_block_stop" {
            let index = event["index"].as_u64().unwrap_or(0) as usize;
            if let Some(block) = collector.message["content"]
                .as_array()
                .and_then(|c| c.get(index))
                .filter(|b| b["type"] == "tool_use")
            {
                run.calls.push(ToolCall {
                    id: block["id"].as_str().unwrap_or("").into(),
                    name: block["name"].as_str().unwrap_or("").into(),
                    arguments: block["input"].clone(),
                    claimed: false,
                    result: None,
                    control_id: if block["name"] == "aipass_acp_respond" {
                        block
                            .pointer("/input/request/id")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    } else {
                        None
                    },
                });
            }
        }
        run.message = collector.message.clone();
        run.content = collector.message["content"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let sender = run.sender.clone();
        if collector.complete {
            run.terminal = true;
            run.parked = run.message["stop_reason"] == "tool_use";
            run.deferred = false;
            run.last_used = Instant::now();
            run.history_messages
                .push(json!({"role":"assistant","content":run.content}));
            run.history = history_hash(&run.history_messages);
            run.sender = None;
        }
        (sender, run.segment)
    };
    if let Some(sender) = sender {
        let kind = event["type"].as_str().unwrap_or("message");
        if sender
            .blocking_send((
                Bytes::from(format!("event: {kind}\ndata: {event}\n\n")),
                collector.complete,
            ))
            .is_err()
        {
            // The old terminal send may have been blocked by backpressure
            // while another HTTP segment attached. Its disconnect must not
            // terminate the reader serving that newer segment.
            return inner
                .runs
                .lock()
                .ok()
                .and_then(|runs| runs.get(key).map(|run| run.segment != segment))
                .unwrap_or(false);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collected_citations_survive_and_streaming_history_has_a_size_limit() {
        let mut collector = Collector::default();
        collector
            .event(&json!({"type":"message_start","message":{"content":[]}}))
            .unwrap();
        collector.event(&json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"answer"}})).unwrap();
        let citation = json!({"type":"web_search_result_location","url":"https://example.test/source","cited_text":"fact"});
        collector.event(&json!({"type":"content_block_delta","index":0,"delta":{"type":"citations_delta","citation":citation}})).unwrap();
        assert_eq!(
            collector.message["content"][0]["citations"],
            json!([citation])
        );
        collector.bytes = 64 * 1024 * 1024;
        assert!(collector.event(&json!({"type":"message_stop"})).is_err());
        assert!(!collector.complete);
    }

    #[test]
    fn malformed_native_frames_return_errors_without_panicking() {
        for malformed in [
            json!({"type":"message_start","message":42}),
            json!({"type":"message_start","message":{"usage":42}}),
            json!({"type":"content_block_start","index":0,"content_block":42}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"unowned"}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":42}}),
            json!({"type":"content_block_stop","index":"invalid"}),
            json!({"type":"content_block_stop","index":2}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":42}),
        ] {
            let mut collector = Collector::default();
            collector
                .event(&json!({"type":"message_start","message":{"usage":{}}}))
                .unwrap();
            collector.event(&json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}})).unwrap();
            assert!(
                collector.event(&malformed).is_err(),
                "accepted malformed frame: {malformed}"
            );
        }
    }
}
