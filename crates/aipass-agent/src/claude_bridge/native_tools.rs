//! Separate Claude-executed tools from caller tools; merge native agent rounds.
use super::*;
pub(super) fn search_requested(payload: &Value) -> Result<bool, preflight::Error> {
    let mut requested = false;
    for tool in payload["tools"].as_array().into_iter().flatten() {
        let kind = tool["type"].as_str().unwrap_or("");
        if kind.starts_with("web_search_") {
            if tool["name"] != "web_search" {
                return Err("native web search must be named web_search".into());
            }
            if tool.as_object().is_none_or(|o| {
                o.keys()
                    .any(|k| !matches!(k.as_str(), "type" | "name" | "cache_control"))
            }) {
                return Err(preflight::Error::unsupported("Claude CLI native search cannot enforce tool-level search options; use a search API route for constrained searches"));
            }
            requested = true;
        } else if tool.get("type").is_some_and(|v| v != "custom") {
            return Err(preflight::Error::unsupported(
                "Claude subscription supports caller function tools and native web search only",
            ));
        }
    }
    Ok(requested)
}
pub(super) fn configure(command: &mut Command, web: bool) {
    command
        .arg("--tools")
        .arg(if web { "WebSearch" } else { "" })
        .arg("--allowedTools")
        .arg(if web {
            "mcp__aipass__*,WebSearch"
        } else {
            "mcp__aipass__*"
        });
}
/// Native calls are progress, not instructions for the client to execute again.
#[derive(Default)]
pub(super) struct Events {
    started: bool,
    next: u64,
    indices: std::collections::HashMap<u64, u64>,
    own: bool,
    caller: bool,
    current: Value,
    before: Value,
    searches: u64,
    ending: Option<Value>,
    native_text: bool,
}
impl Events {
    pub(super) fn adapt(&mut self, mut event: Value) -> Result<Option<Value>, String> {
        match event["type"].as_str() {
            Some("message_start") => {
                if !event["message"].is_object()
                    || event["message"]
                        .get("usage")
                        .is_some_and(|v| !v.is_null() && !v.is_object())
                {
                    return Err("invalid Claude message envelope".into());
                }
                self.indices.clear();
                self.own = false;
                self.caller = false;
                self.current = event["message"]["usage"].clone();
                if self.started {
                    return Ok(None);
                }
                self.started = true;
            }
            Some("content_block_start") => {
                let old = event["index"]
                    .as_u64()
                    .ok_or("invalid Claude block index")?;
                let block = &event["content_block"];
                self.native_text |= block["type"] == "text"
                    && block["text"].as_str().is_some_and(|s| !s.is_empty());
                if block["type"] == "tool_use"
                    && !block["name"]
                        .as_str()
                        .is_some_and(|n| n.starts_with("mcp__aipass__"))
                {
                    self.own = true;
                    if block["name"] == "WebSearch" {
                        self.searches += 1;
                    }
                    return Ok(None);
                }
                if block["type"] == "tool_use" {
                    self.caller = true;
                }
                self.indices.insert(old, self.next);
                event["index"] = json!(self.next);
                self.next += 1;
            }
            Some("content_block_delta" | "content_block_stop") => {
                self.native_text |= event["delta"]["type"] == "text_delta"
                    && event["delta"]["text"]
                        .as_str()
                        .is_some_and(|s| !s.is_empty());
                let old = event["index"]
                    .as_u64()
                    .ok_or("invalid Claude block index")?;
                let Some(index) = self.indices.get(&old) else {
                    return Ok(None);
                };
                event["index"] = json!(index);
            }
            Some("message_delta") => {
                if event
                    .get("usage")
                    .is_some_and(|v| !v.is_null() && !v.is_object())
                {
                    return Err("invalid Claude usage envelope".into());
                }
                if let Some(usage) = event["usage"].as_object() {
                    for (k, v) in usage {
                        self.current[k] = v.clone();
                    }
                }
                if self.own && !self.caller {
                    return Ok(None);
                }
                event["usage"] = sum(&self.before, &self.current);
                if self.searches > 0 {
                    event["usage"]["server_tool_use"] =
                        json!({"web_search_requests":self.searches});
                }
                if !self.caller {
                    self.ending = Some(event);
                    return Ok(None);
                }
            }
            Some("message_stop") => {
                if self.own && !self.caller {
                    self.before = sum(&self.before, &self.current);
                    self.current = Value::Null;
                    return Ok(None);
                }
                if !self.caller {
                    return Ok(None);
                }
                *self = Self::default();
            }
            _ => {}
        }
        Ok(Some(event))
    }
    fn ensure_started(&mut self) -> Vec<Value> {
        if self.started {
            return Vec::new();
        }
        self.started = true;
        vec![
            json!({"type":"message_start","message":{"id":format!("msg_acp_{}",Uuid::new_v4().simple()),"type":"message","role":"assistant","content":[],"usage":{}}}),
        ]
    }
    pub(super) fn activity(&mut self, text: String) -> Vec<Value> {
        let mut events = self.ensure_started();
        let index = self.next;
        self.next += 1;
        events.push(json!({"type":"content_block_start","index":index,"content_block":{"type":"text","text":text}}));
        events.push(json!({"type":"content_block_stop","index":index}));
        events
    }
    pub(super) fn permission(&mut self, request: Value) -> Vec<Value> {
        self.caller_tools(vec![json!({"type":"tool_use","id":"pending","name":"mcp__aipass__aipass_acp_respond","input":{"request":request}})])
    }
    pub(super) fn caller_tools(&mut self, calls: Vec<Value>) -> Vec<Value> {
        let mut events = self.ensure_started();
        for block in calls {
            let index = self.next;
            self.next += 1;
            events.push(json!({"type":"content_block_start","index":index,"content_block":block}));
            events.push(json!({"type":"content_block_stop","index":index}));
        }
        events.push(json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":sum(&self.before,&self.current)}));
        events.push(json!({"type":"message_stop"}));
        *self = Self::default();
        events
    }
    pub(super) fn finish(&mut self) -> Vec<Value> {
        let mut events = self.ensure_started();
        events.push(self.ending.take().unwrap_or_else(||json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":sum(&self.before,&self.current)})));
        events.push(json!({"type":"message_stop"}));
        *self = Self::default();
        events
    }
    pub(super) fn result(&mut self, result: &Value) -> Vec<Value> {
        let mut events = Vec::new();
        if !self.native_text {
            if let Some(text) = result["result"].as_str().filter(|s| !s.is_empty()) {
                events.extend(self.activity(text.into()));
            }
        }
        events.extend(self.finish());
        events
    }
    pub(super) fn autonomous_result(&mut self, result: &Value) -> Vec<Value> {
        let mut events = Vec::new();
        if result["is_error"] == true {
            events.extend(self.activity("Claude Code background turn failed.".into()));
        } else if !self.native_text {
            if let Some(text) = result["result"].as_str().filter(|s| !s.is_empty()) {
                events.extend(self.activity(text.into()));
            }
        }
        self.before = sum(&self.before, &self.current);
        self.current = Value::Null;
        self.ending = None;
        self.native_text = false;
        events
    }
    pub(super) fn command_output(&mut self, text: String) -> Vec<Value> {
        self.native_text = true;
        self.activity(text)
    }
}
fn sum(a: &Value, b: &Value) -> Value {
    let mut out = a.as_object().cloned().unwrap_or_default();
    for (k, v) in b.as_object().into_iter().flatten() {
        if let Some(n) = v.as_u64() {
            out.insert(
                k.clone(),
                json!(out
                    .get(k)
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    .saturating_add(n)),
            );
        } else if v.is_object() {
            let value = sum(out.get(k).unwrap_or(&Value::Null), v);
            out.insert(k.clone(), value);
        } else {
            out.insert(k.clone(), v.clone());
        }
    }
    Value::Object(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_search_is_allowed_on_explicit_request() {
        assert!(
            !search_requested(&json!({"tools":[{"name":"WebSearch","input_schema":{}}]})).unwrap()
        );
        assert!(search_requested(
            &json!({"tools":[{"type":"web_search_20250305","name":"web_search"}]})
        )
        .unwrap());
        assert!(search_requested(
            &json!({"tools":[{"type":"computer_20250124","name":"computer"}]})
        )
        .is_err());
        for web in [false, true] {
            let mut c = Command::new("claude");
            configure(&mut c, web);
            let args = c
                .get_args()
                .map(|v| v.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            assert_eq!(args[1], if web { "WebSearch" } else { "" });
            assert!(!args
                .iter()
                .any(|v| v.contains("Bash") || v.contains("Read") || v.contains("Write")));
        }
    }
    #[test]
    fn search_rounds_are_one_reply_with_aggregated_usage() {
        let mut events = Events::default();
        let mut output = Vec::new();
        for e in [
            json!({"type":"message_start","message":{"usage":{"input_tokens":10}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","name":"WebSearch"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":3}}),
            json!({"type":"message_stop"}),
            json!({"type":"message_start","message":{"usage":{"input_tokens":20}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"answer"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":4}}),
            json!({"type":"message_stop"}),
        ] {
            if let Some(e) = events.adapt(e).unwrap() {
                output.push(e);
            }
        }
        output.extend(events.finish());
        assert_eq!(
            output
                .iter()
                .filter(|e| e["type"] == "message_start")
                .count(),
            1
        );
        assert_eq!(
            output
                .iter()
                .filter(|e| e["type"] == "message_stop")
                .count(),
            1
        );
        let usage = &output
            .iter()
            .find(|e| e["type"] == "message_delta")
            .unwrap()["usage"];
        assert_eq!(usage["input_tokens"], 30);
        assert_eq!(usage["output_tokens"], 7);
        assert_eq!(usage["server_tool_use"]["web_search_requests"], 1);
        assert!(!output
            .iter()
            .any(|e| e["content_block"]["name"] == "WebSearch"));
    }
}
