//! Claude event decoding, tool collection and streamed response lifecycle.
use super::*;

pub(super) struct SegmentStream {
    pub(super) receiver: tokio::sync::mpsc::Receiver<(Bytes, bool)>,
    pub(super) inner: Weak<Inner>,
    pub(super) key: String,
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
                    runs.remove(&self.key);
                }
            }
        }
    }
}
#[derive(Default)]
pub(super) struct Collector {
    pub(super) message: Value,
    pub(super) complete: bool,
}
impl Collector {
    fn event(&mut self, event: &Value) -> Result<(), String> {
        match event["type"].as_str() {
            Some("message_start") => {
                self.message = event["message"].clone();
                self.message["content"] = json!([]);
            }
            Some("content_block_start") => {
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
                    .ok_or("unknown Claude block")?;
                let delta = &event["delta"];
                let (field, source) = match delta["type"].as_str() {
                    Some("text_delta") => ("text", "text"),
                    Some("thinking_delta") => ("thinking", "thinking"),
                    Some("signature_delta") => ("signature", "signature"),
                    Some("input_json_delta") => ("_arguments", "partial_json"),
                    _ => return Ok(()),
                };
                let mut value = block[field].as_str().unwrap_or("").to_owned();
                value.push_str(delta[source].as_str().unwrap_or(""));
                block[field] = json!(value);
            }
            Some("content_block_stop") => {
                let i = event["index"].as_u64().unwrap_or(0) as usize;
                if let Some(block) = self.message["content"]
                    .as_array_mut()
                    .and_then(|v| v.get_mut(i))
                {
                    if let Some(args) = block.get("_arguments").and_then(Value::as_str) {
                        block["input"] = serde_json::from_str(args)
                            .map_err(|_| "invalid Claude tool arguments")?;
                        block.as_object_mut().unwrap().remove("_arguments");
                    }
                }
            }
            Some("message_delta") => {
                self.message["stop_reason"] = event["delta"]["stop_reason"].clone();
                if let Some(usage) = event["usage"].as_object() {
                    for (k, v) in usage {
                        self.message["usage"][k] = v.clone();
                    }
                }
            }
            Some("message_stop") => self.complete = true,
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
pub(super) fn read_events(stdout: std::process::ChildStdout, inner: Weak<Inner>, key: String) {
    use std::io::Read;
    let mut reader = BufReader::new(stdout);
    let mut collector = Collector::default();
    loop {
        let mut line = Vec::new();
        let Ok(n) = reader
            .by_ref()
            .take(MAX_LINE + 1)
            .read_until(b'\n', &mut line)
        else {
            break;
        };
        if n == 0 || n as u64 > MAX_LINE {
            break;
        }
        let parsed = serde_json::from_slice::<Value>(&line);
        line.zeroize();
        let Ok(value) = parsed else {
            break;
        };
        if value["type"] == "result" && value["is_error"] == true {
            break;
        }
        if value["type"] != "stream_event" {
            continue;
        }
        let mut event = value["event"].clone();
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
            collector = Collector::default();
        }
        if collector.event(&event).is_err() {
            break;
        }
        let Some(inner) = inner.upgrade() else {
            break;
        };
        let sender = {
            let Ok(mut runs) = inner.runs.lock() else {
                break;
            };
            let Some(run) = runs.get_mut(&key) else {
                break;
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
                run.last_used = Instant::now();
                run.history_messages
                    .push(json!({"role":"assistant","content":run.content}));
                run.history = history_hash(&run.history_messages);
                run.sender = None;
            }
            sender
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
                break;
            }
        }
    }
    if let Some(inner) = inner.upgrade() {
        if let Ok(mut runs) = inner.runs.lock() {
            runs.remove(&key);
        }
    }
}
