type AwsEvent = (std::collections::HashMap<String, String>, serde_json::Value);
use super::connect::MAX_FRAME;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
fn text(v: &Value) -> String {
    v.as_str().map(str::to_owned).unwrap_or_else(|| {
        v.as_array()
            .into_iter()
            .flatten()
            .filter(|p| p["type"] == "text")
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    })
}
fn id(v: &Value) -> String {
    let id = v.as_str().unwrap_or("");
    if !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
    {
        id.into()
    } else {
        format!(
            "t_{}",
            &URL_SAFE_NO_PAD.encode(Sha256::digest(id.as_bytes()))[..32]
        )
    }
}
fn append(a: &mut Value, b: &str) {
    let before = a.as_str().unwrap_or("");
    *a = json!(if before.is_empty() {
        b.into()
    } else if b.is_empty() {
        before.to_owned()
    } else {
        format!("{before}\n\n{b}")
    });
}
pub fn build_request(
    req: &Value,
    model: &str,
    profile: &str,
    session: &str,
) -> Result<(Value, u64), String> {
    let effort = req["output_config"]["effort"].as_str().unwrap_or("");
    let on = matches!(
        req["thinking"]["type"].as_str(),
        Some("enabled" | "adaptive")
    ) || (!effort.is_empty() && effort != "low");
    let budget = if on && (model.to_lowercase().contains("claude") || model == "auto") {
        match effort {
            "low" => 10000,
            "high" => 30000,
            "xhigh" | "max" => 50000,
            "" => req["thinking"]["budget_tokens"].as_u64().unwrap_or(20000),
            _ => 20000,
        }
    } else {
        0
    };
    let mut entries: Vec<Value> = vec![];
    for m in req["messages"]
        .as_array()
        .ok_or("Kiro messages must be an array")?
    {
        let parts = m["content"].as_array().cloned().unwrap_or_else(|| {
            vec![json!({"type":"text","text":m["content"].as_str().unwrap_or("")})]
        });
        let mut texts = vec![];
        if m["role"] == "assistant" {
            let mut calls = vec![];
            for p in parts {
                match p["type"].as_str() {
                    Some("text") => texts.push(p["text"].as_str().unwrap_or("").to_owned()),
                    Some("tool_use") => {
                        if !p["input"].is_object() {
                            return Err("Kiro tool arguments must be objects".into());
                        }
                        calls.push(
                            json!({"name":p["name"],"toolUseId":id(&p["id"]),"input":p["input"]}),
                        );
                    }
                    Some("thinking" | "redacted_thinking") => {}
                    _ => return Err("unsupported Kiro assistant block".into()),
                }
            }
            if texts.is_empty() && calls.is_empty() {
                continue;
            }
            let a = json!({"content":texts.join("\n\n"),"toolUses":calls});
            if let Some(prev) = entries
                .last_mut()
                .and_then(|e| e.get_mut("assistantResponseMessage"))
            {
                append(&mut prev["content"], a["content"].as_str().unwrap_or(""));
                prev["toolUses"]
                    .as_array_mut()
                    .unwrap()
                    .extend(a["toolUses"].as_array().unwrap().clone());
            } else {
                entries.push(json!({"assistantResponseMessage":a}));
            }
            continue;
        }
        let (mut images, mut results) = (vec![], vec![]);
        for p in parts {
            match p["type"].as_str() {
                Some("text") => texts.push(p["text"].as_str().unwrap_or("").to_owned()),
                Some("image") => {
                    if p["source"]["type"] != "base64" {
                        return Err("Kiro images must be inline base64".into());
                    }
                    let mime = p["source"]["media_type"]
                        .as_str()
                        .unwrap_or("image/png")
                        .trim_start_matches("image/");
                    images.push(json!({"format":if mime=="jpg"{"jpeg"}else{mime},"source":{"bytes":p["source"]["data"]}}));
                }
                Some("document") => {
                    let content = match p["source"]["type"].as_str() {
                        Some("text") => p["source"]["data"].as_str().unwrap_or("").to_owned(),
                        Some("content") => text(&p["source"]["content"]),
                        _ => return Err("Kiro only supports text documents".into()),
                    };
                    texts.push(format!("{}\n{content}", p["title"].as_str().unwrap_or("")));
                }
                Some("tool_result") => {
                    let output = text(&p["content"]);
                    if output.chars().count() > 250000 {
                        return Err("Kiro tool result exceeds the provider limit".into());
                    }
                    results.push(json!({"toolUseId":id(&p["tool_use_id"]),"status":if p["is_error"]==true{"error"}else{"success"},"content":[{"text":if output.is_empty(){"(no output)"}else{&output}}]}));
                }
                _ => return Err("unsupported Kiro user block".into()),
            }
        }
        let u = json!({"content":texts.join("\n\n"),"modelId":model,"origin":"KIRO_CLI","images":images,"userInputMessageContext":{"toolResults":results}});
        if let Some(prev) = entries
            .last_mut()
            .and_then(|e| e.get_mut("userInputMessage"))
        {
            append(&mut prev["content"], u["content"].as_str().unwrap_or(""));
            prev["images"].as_array_mut().unwrap().extend(images);
            prev["userInputMessageContext"]["toolResults"]
                .as_array_mut()
                .unwrap()
                .extend(results);
        } else {
            entries.push(json!({"userInputMessage":u}));
        }
    }
    if entries
        .first()
        .is_some_and(|e| e.get("assistantResponseMessage").is_some())
    {
        entries.insert(0,json!({"userInputMessage":{"content":"Please proceed with the task.","modelId":model,"origin":"KIRO_CLI","images":[],"userInputMessageContext":{"toolResults":[]}}}));
    }
    if entries
        .last()
        .is_none_or(|e| e.get("userInputMessage").is_none())
    {
        entries.push(json!({"userInputMessage":{"content":"","modelId":model,"origin":"KIRO_CLI","images":[],"userInputMessageContext":{"toolResults":[]}}}));
    }
    for i in 0..entries.len() {
        let calls = if i > 0 {
            entries[i - 1]["assistantResponseMessage"]["toolUses"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        } else {
            vec![]
        };
        if let Some(u) = entries[i].get_mut("userInputMessage") {
            let results = u["userInputMessageContext"]["toolResults"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let mut seen = std::collections::HashSet::new();
            let mut kept = vec![];
            for r in results {
                let id = r["toolUseId"].as_str().unwrap_or("").to_owned();
                if !calls.iter().any(|c| c["toolUseId"] == id) || !seen.insert(id) {
                    return Err("Kiro tool result has no unique matching call".into());
                }
                kept.push(r);
            }
            for call in calls {
                if !seen.contains(call["toolUseId"].as_str().unwrap_or("")) {
                    kept.push(json!({"toolUseId":call["toolUseId"],"status":"error","content":[{"text":"Tool use was interrupted and did not produce a result."}]}));
                }
            }
            u.as_object_mut().unwrap().remove("userInputMessageContext");
            if !kept.is_empty() {
                u["userInputMessageContext"] = json!({"toolResults":kept});
            }
            if text(&u["content"]).is_empty() && u.get("userInputMessageContext").is_none() {
                u["content"] = json!("Please proceed with the task.");
            }
        }
    }
    let latest = entries.iter().rposition(|e| {
        e["userInputMessage"]["images"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
    });
    for (i, e) in entries.iter_mut().enumerate() {
        if let Some(u) = e.get_mut("userInputMessage") {
            if latest != Some(i) || u["images"].as_array().is_none_or(|a| a.is_empty()) {
                u.as_object_mut().unwrap().remove("images");
            }
        }
        if let Some(a) = e.get_mut("assistantResponseMessage") {
            if a["toolUses"].as_array().is_none_or(|v| v.is_empty()) {
                a.as_object_mut().unwrap().remove("toolUses");
            }
        }
    }
    let system = text(&req["system"]);
    let system = if budget > 0 {
        format!("<thinking_mode>enabled</thinking_mode><max_thinking_length>{budget}</max_thinking_length>\n{system}")
    } else {
        system
    };
    if !system.is_empty() {
        entries[0]["userInputMessage"]["content"] = json!(format!(
            "{system}\n\n{}",
            entries[0]["userInputMessage"]["content"]
                .as_str()
                .unwrap_or("")
        ));
    }
    let mut tools = vec![];
    let mut offered = std::collections::HashSet::new();
    for t in req["tools"].as_array().into_iter().flatten() {
        let name = t["name"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("Kiro tool name missing")?;
        if !t["input_schema"].is_object() {
            return Err("Kiro only supports caller-defined tools with an input schema".into());
        }
        offered.insert(name.to_owned());
        tools.push(json!({"toolSpecification":{"name":name,"description":t["description"].as_str().filter(|s|!s.is_empty()).unwrap_or(name),"inputSchema":{"json":t["input_schema"]}}}));
    }
    for e in &entries {
        for call in e["assistantResponseMessage"]["toolUses"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let name = call["name"].as_str().unwrap_or("");
            if offered.insert(name.to_owned()) {
                tools.push(json!({"toolSpecification":{"name":name,"description":"Tool","inputSchema":{"json":{"type":"object","properties":{}}}}}));
            }
        }
    }
    let mut current = entries.pop().unwrap();
    if !tools.is_empty() {
        if !current["userInputMessage"]["userInputMessageContext"].is_object() {
            current["userInputMessage"]["userInputMessageContext"] = json!({});
        }
        current["userInputMessage"]["userInputMessageContext"]["tools"] = json!(tools);
    }
    let mut state = json!({"chatTriggerType":"MANUAL","agentTaskType":"vibe","conversationId":session,"currentMessage":current});
    if !entries.is_empty() {
        state["history"] = json!(entries);
    }
    let mut out = json!({"conversationState":state,"agentMode":"vibe"});
    if !profile.is_empty() {
        out["profileArn"] = json!(profile);
    }
    Ok((out, budget))
}
#[derive(Default)]
struct AwsFrames {
    buffer: Vec<u8>,
}
impl AwsFrames {
    fn push(&mut self, data: &[u8]) -> Result<Vec<AwsEvent>, String> {
        self.buffer.extend_from_slice(data);
        let mut out = vec![];
        let mut pos = 0;
        while self.buffer.len() - pos >= 12 {
            let b = &self.buffer[pos..];
            let total = u32::from_be_bytes(b[..4].try_into().unwrap()) as usize;
            let len = u32::from_be_bytes(b[4..8].try_into().unwrap()) as usize;
            if !(16..=MAX_FRAME).contains(&total) || len > total - 16 {
                return Err("malformed AWS eventstream frame".into());
            }
            if crc32fast::hash(&b[..8]) != u32::from_be_bytes(b[8..12].try_into().unwrap()) {
                return Err("AWS eventstream prelude checksum mismatch".into());
            }
            if b.len() < total {
                break;
            }
            if crc32fast::hash(&b[..total - 4])
                != u32::from_be_bytes(b[total - 4..total].try_into().unwrap())
            {
                return Err("AWS eventstream checksum mismatch".into());
            }
            let mut headers = std::collections::HashMap::new();
            let h = &b[12..12 + len];
            let mut i = 0;
            while i < h.len() {
                let n = h[i] as usize;
                i += 1;
                let key = std::str::from_utf8(h.get(i..i + n).ok_or("truncated AWS header")?)
                    .map_err(|_| "invalid AWS header")?
                    .to_owned();
                i += n;
                let kind = *h.get(i).ok_or("truncated AWS header type")?;
                i += 1;
                let n = match kind {
                    0 | 1 => 0,
                    2 => 1,
                    3 => 2,
                    4 => 4,
                    5 | 8 => 8,
                    9 => 16,
                    6 | 7 => {
                        let n = u16::from_be_bytes(
                            h.get(i..i + 2)
                                .ok_or("truncated AWS header length")?
                                .try_into()
                                .unwrap(),
                        ) as usize;
                        i += 2;
                        n
                    }
                    _ => return Err("unknown AWS header type".into()),
                };
                let value = h.get(i..i + n).ok_or("truncated AWS header value")?;
                if kind == 7 {
                    headers.insert(
                        key,
                        std::str::from_utf8(value)
                            .map_err(|_| "invalid AWS string header")?
                            .to_owned(),
                    );
                }
                i += n;
            }
            let payload = serde_json::from_slice(&b[12 + len..total - 4])
                .map_err(|_| "invalid AWS event JSON")?;
            out.push((headers, payload));
            pos += total;
        }
        self.buffer.drain(..pos);
        if self.buffer.len() > MAX_FRAME {
            return Err("AWS stream buffer exceeds limit".into());
        }
        Ok(out)
    }
}
#[derive(Default)]
struct Thinking {
    state: u8,
    buffer: String,
}
impl Thinking {
    fn feed(&mut self, s: &str) -> Vec<(bool, String)> {
        self.buffer.push_str(s);
        let mut out = vec![];
        if self.state == 0 {
            let t = self.buffer.trim_start();
            if let Some(rest) = t.strip_prefix("<thinking>") {
                self.buffer = rest.trim_start_matches(['\r', '\n']).into();
                self.state = 1;
            } else if "<thinking>".starts_with(t) {
                return out;
            } else {
                self.state = 2;
            }
        }
        if self.state == 1 {
            if let Some(i) = self.buffer.find("</thinking>") {
                if i > 0 {
                    out.push((true, self.buffer[..i].into()));
                }
                self.buffer = self.buffer[i + 11..]
                    .trim_start_matches(['\r', '\n'])
                    .into();
                self.state = 2;
            } else {
                let held = (1..11)
                    .rev()
                    .find(|n| self.buffer.ends_with(&"</thinking>"[..*n]))
                    .unwrap_or(0);
                let n = self.buffer.len() - held;
                if n > 0 {
                    out.push((true, self.buffer[..n].into()));
                    self.buffer = self.buffer[n..].into();
                }
                return out;
            }
        }
        if !self.buffer.is_empty() {
            out.push((false, std::mem::take(&mut self.buffer)));
        }
        out
    }
}
fn event(kind: &str, mut v: Value) -> Vec<u8> {
    v["type"] = json!(kind);
    format!("event: {kind}\ndata: {v}\n\n").into_bytes()
}
pub struct KiroStream {
    status: Option<u16>,
    frames: AwsFrames,
    thinking: Thinking,
    budget: u64,
    model: String,
    id: String,
    begun: bool,
    index: i64,
    open: String,
    tool_id: String,
    tool_args: String,
    tools: usize,
    metadata: bool,
    usage: Value,
    stop: String,
    window: u64,
    pct: f64,
}
impl KiroStream {
    pub fn new(model: &str, id: &str, budget: u64, window: u64) -> Self {
        Self {
            status: None,
            frames: AwsFrames::default(),
            thinking: Thinking::default(),
            budget,
            model: model.into(),
            id: id.into(),
            begun: false,
            index: -1,
            open: String::new(),
            tool_id: String::new(),
            tool_args: String::new(),
            tools: 0,
            metadata: false,
            usage: json!({"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}),
            stop: String::new(),
            window,
            pct: 0.0,
        }
    }
    fn close(&mut self, out: &mut Vec<Vec<u8>>) -> Result<(), String> {
        if self.open == "tool" && !self.tool_args.is_empty() {
            let v: Value = serde_json::from_str(&self.tool_args)
                .map_err(|_| "Kiro returned incomplete tool arguments")?;
            if !v.is_object() {
                return Err("Kiro tool arguments must be objects".into());
            }
        }
        if !self.open.is_empty() {
            out.push(event("content_block_stop", json!({"index":self.index})));
            self.open.clear();
            self.tool_args.clear();
        }
        Ok(())
    }
    fn begin(&mut self, kind: &str, block: Value, out: &mut Vec<Vec<u8>>) -> Result<(), String> {
        if !self.begun {
            self.begun = true;
            out.push(event("message_start",json!({"message":{"id":self.id,"type":"message","role":"assistant","model":self.model,"content":[],"stop_reason":null,"usage":{"input_tokens":0,"output_tokens":0}}})));
        }
        self.close(out)?;
        self.index += 1;
        self.open = kind.into();
        out.push(event(
            "content_block_start",
            json!({"index":self.index,"content_block":block}),
        ));
        Ok(())
    }
    fn text(&mut self, thinking: bool, text: &str, out: &mut Vec<Vec<u8>>) -> Result<(), String> {
        if text.is_empty() {
            return Ok(());
        }
        let (kind, delta, field) = if thinking {
            ("thinking", "thinking_delta", "thinking")
        } else {
            ("text", "text_delta", "text")
        };
        if self.open != kind {
            let mut block = json!({"type":kind});
            block[field] = json!("");
            if thinking {
                block["signature"] = json!("");
            }
            self.begin(kind, block, out)?;
        }
        let mut v = json!({"type":delta});
        v[field] = json!(text);
        out.push(event(
            "content_block_delta",
            json!({"index":self.index,"delta":v}),
        ));
        Ok(())
    }
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        let mut out = vec![];
        for (h, v) in self.frames.push(bytes)? {
            let kind = h.get(":event-type").map(String::as_str).unwrap_or("");
            if h.get(":message-type")
                .is_some_and(|s| s == "error" || s == "exception")
                || matches!(
                    kind,
                    "error"
                        | "throttlingError"
                        | "validationError"
                        | "serviceUnavailableError"
                        | "internalServerException"
                )
            {
                self.status = Some(super::failure_status(&format!("{kind} {v}")));
                return Err("Kiro returned an eventstream error".into());
            }
            match kind {
                "assistantResponseEvent" => {
                    let text = v["content"].as_str().unwrap_or("");
                    if self.budget > 0 {
                        for (thinking, text) in self.thinking.feed(text) {
                            self.text(thinking, &text, &mut out)?;
                        }
                    } else {
                        self.text(false, text, &mut out)?;
                    }
                }
                "reasoningContentEvent" => {
                    if let Some(t) = v["text"].as_str() {
                        self.text(true, t, &mut out)?;
                    } else if let Some(sig) = v["signature"].as_str() {
                        if self.open != "thinking" {
                            return Err("Kiro thinking signature has no matching block".into());
                        }
                        out.push(event("content_block_delta",json!({"index":self.index,"delta":{"type":"signature_delta","signature":sig}})));
                    }
                }
                "toolUseEvent" => {
                    let id = v["toolUseId"].as_str().unwrap_or("");
                    if !id.is_empty() && id != self.tool_id {
                        self.tool_id = id.into();
                        self.tools += 1;
                        self.begin(
                            "tool",
                            json!({"type":"tool_use","id":id,"name":v["name"],"input":{}}),
                            &mut out,
                        )?;
                    }
                    if !v["input"].is_null() {
                        if self.open != "tool" {
                            return Err("Kiro tool argument has no matching call".into());
                        }
                        if v["input"].as_object().is_some_and(|v| v.is_empty()) {
                            continue;
                        }
                        let args = v["input"]
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| v["input"].to_string());
                        if self.tool_args.len() + args.len() > MAX_FRAME {
                            return Err("Kiro tool arguments exceed limit".into());
                        }
                        self.tool_args.push_str(&args);
                        out.push(event("content_block_delta",json!({"index":self.index,"delta":{"type":"input_json_delta","partial_json":args}})));
                    }
                }
                "metadataEvent" | "messageMetadataEvent" => {
                    self.metadata = true;
                    if let Some(s) = v["stopReason"].as_str() {
                        self.stop = s.into();
                    }
                    let u = &v["tokenUsage"];
                    for (k, src) in [
                        ("input_tokens", "uncachedInputTokens"),
                        ("output_tokens", "outputTokens"),
                        ("cache_read_input_tokens", "cacheReadInputTokens"),
                        ("cache_creation_input_tokens", "cacheWriteInputTokens"),
                    ] {
                        let value = u[src]
                            .as_u64()
                            .or_else(|| {
                                if k == "input_tokens" {
                                    u["inputTokens"].as_u64()
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(0);
                        self.usage[k] = json!(self.usage[k]
                            .as_u64()
                            .unwrap_or(0)
                            .checked_add(value)
                            .ok_or("Kiro token usage overflow")?);
                    }
                    self.pct = u["contextUsagePercentage"].as_f64().unwrap_or(self.pct);
                }
                "contextUsageEvent" => {
                    self.pct = v["contextUsagePercentage"].as_f64().unwrap_or(self.pct)
                }
                _ => {}
            }
        }
        Ok(out)
    }
    pub fn finish(&mut self) -> Result<Vec<Vec<u8>>, String> {
        if !self.frames.buffer.is_empty() || (!self.metadata && !self.begun) {
            return Err("Kiro reply ended before a complete AWS event".into());
        }
        if self.thinking.state == 1 {
            return Err("Kiro thinking block ended before its closing tag".into());
        }
        let mut out = vec![];
        if !self.begun {
            self.begin("text", json!({"type":"text","text":""}), &mut out)?;
        }
        if !self.thinking.buffer.is_empty() {
            let text = std::mem::take(&mut self.thinking.buffer);
            self.text(false, &text, &mut out)?;
        }
        self.close(&mut out)?;
        if self.usage["input_tokens"] == 0
            && self.usage["cache_read_input_tokens"] == 0
            && self.usage["cache_creation_input_tokens"] == 0
            && self.pct > 0.0
            && self.window > 0
        {
            self.usage["input_tokens"] =
                json!((self.pct / 100.0 * self.window as f64).floor() as u64);
        }
        let stop = if self.tools > 0 {
            "tool_use"
        } else if self.stop.eq_ignore_ascii_case("max_tokens") {
            "max_tokens"
        } else if self.stop.eq_ignore_ascii_case("content_filtered") {
            "refusal"
        } else {
            "end_turn"
        };
        out.push(event(
            "message_delta",
            json!({"delta":{"stop_reason":stop,"stop_sequence":null},"usage":self.usage}),
        ));
        out.push(event("message_stop", json!({})));
        Ok(out)
    }
}

impl super::ProviderStream for KiroStream {
    fn has_output(&self) -> bool {
        self.begun || self.metadata || !self.thinking.buffer.is_empty()
    }
    fn push(&mut self, b: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        KiroStream::push(self, b)
    }
    fn finish(&mut self) -> Result<Vec<Vec<u8>>, String> {
        KiroStream::finish(self)
    }
    fn failure_status(&self) -> Option<u16> {
        self.status
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn content_before_coalesced_throttling_is_not_a_retryable_rejection() {
        let mut s = KiroStream::new("m", "id", 0, 0);
        let body = [
            aws_event("assistantResponseEvent", json!({"content":"generated"})),
            aws_event("throttlingError", json!({"message":"try later"})),
        ]
        .concat();
        assert!(s.push(&body).is_err());
        assert!(super::super::ProviderStream::has_output(&s));
        assert_eq!(s.status, Some(429));
    }
    #[test]
    fn overflowing_metadata_cannot_panic_the_decoder() {
        let mut s = KiroStream::new("m", "id", 0, 0);
        s.push(&aws_event(
            "metadataEvent",
            json!({"tokenUsage":{"inputTokens":u64::MAX}}),
        ))
        .unwrap();
        assert!(s
            .push(&aws_event(
                "metadataEvent",
                json!({"tokenUsage":{"inputTokens":1}})
            ))
            .is_err());
    }
    fn aws_event(kind: &str, body: Value) -> Vec<u8> {
        let key = b":event-type";
        let mut header = vec![key.len() as u8];
        header.extend(key);
        header.push(7);
        header.extend((kind.len() as u16).to_be_bytes());
        header.extend(kind.as_bytes());
        let payload = body.to_string();
        let total = 16 + header.len() + payload.len();
        let mut out = [
            (total as u32).to_be_bytes(),
            (header.len() as u32).to_be_bytes(),
        ]
        .concat();
        out.extend(crc32fast::hash(&out).to_be_bytes());
        out.extend(header);
        out.extend(payload.as_bytes());
        out.extend(crc32fast::hash(&out).to_be_bytes());
        out
    }
    #[test]
    fn fragmented_aws_tools_reasoning_and_metadata_are_preserved() {
        let data=[aws_event("reasoningContentEvent",json!({"text":"consider"})),aws_event("reasoningContentEvent",json!({"signature":"signed"})),aws_event("toolUseEvent",json!({"toolUseId":"call_a","name":"calc","input":"{\"n\":"})),aws_event("toolUseEvent",json!({"toolUseId":"call_a","input":"3}"})),aws_event("metadataEvent",json!({"tokenUsage":{"uncachedInputTokens":10,"outputTokens":7,"cacheReadInputTokens":90}}))].concat();
        let mut s = KiroStream::new("model", "id", 1024, 1000);
        let mut out = vec![];
        for part in data.chunks(3) {
            out.extend(s.push(part).unwrap());
        }
        out.extend(s.finish().unwrap());
        let text = String::from_utf8(out.concat()).unwrap();
        assert!(text.contains("signature_delta"));
        assert!(text.contains("signed"));
        assert!(text.contains("\"stop_reason\":\"tool_use\""));
        assert!(text.contains("\"cache_read_input_tokens\":90"));
        assert!(text.contains("\"input_tokens\":10"));
    }
    #[test]
    fn aws_clean_end_without_optional_usage_is_valid_but_partial_tool_is_not() {
        let mut s = KiroStream::new("model", "id", 0, 0);
        s.push(&aws_event(
            "assistantResponseEvent",
            json!({"content":"answer"}),
        ))
        .unwrap();
        s.finish().unwrap();
        let mut s = KiroStream::new("model", "id", 0, 0);
        s.push(&aws_event(
            "toolUseEvent",
            json!({"toolUseId":"a","name":"calc","input":"{\"n\":"}),
        ))
        .unwrap();
        assert!(s.finish().is_err());
    }
    #[test]
    fn preserves_tools_and_marks_missing_results_as_interrupted() {
        let req = json!({"system":"Keep these rules","messages":[{"role":"user","content":"go"},{"role":"assistant","content":[{"type":"tool_use","id":"foreign/id","name":"calc","input":{"n":1}}]}],"tools":[{"name":"calc","input_schema":{"type":"object"}}]});
        let (body, _) = build_request(&req, "auto", "arn", "session").unwrap();
        let current = &body["conversationState"]["currentMessage"]["userInputMessage"];
        assert_eq!(
            current["userInputMessageContext"]["toolResults"][0]["status"],
            "error"
        );
        assert!(
            body["conversationState"]["history"][0]["userInputMessage"]["content"]
                .as_str()
                .unwrap()
                .contains("Keep these rules")
        );
    }
    #[test]
    fn thinking_tags_can_span_arbitrary_chunks() {
        let mut p = Thinking::default();
        assert!(p.feed("<thi").is_empty());
        assert_eq!(p.feed("nking>hi</thi"), vec![(true, "hi".into())]);
        assert_eq!(p.feed("nking>answer"), vec![(false, "answer".into())]);
    }
    #[test]
    fn checksum_errors_are_never_accepted_as_completion() {
        let mut s = KiroStream::new("auto", "id", 0, 0);
        assert!(s
            .push(&[0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
            .is_err());
        assert!(s.finish().is_err());
    }
}
