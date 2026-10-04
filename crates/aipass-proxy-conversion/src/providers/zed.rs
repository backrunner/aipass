use super::connect::Lines;
use serde_json::{json, Value};
pub fn request(wire: &str, model: &str, mut req: Value) -> Result<Value, String> {
    let vendor = match wire {
        "anthropic" => {
            req.as_object_mut()
                .ok_or("invalid Zed request")?
                .remove("stream");
            for m in req["messages"].as_array_mut().into_iter().flatten() {
                for b in m["content"].as_array_mut().into_iter().flatten() {
                    if b["type"] == "tool_result" && b.get("is_error").is_none() {
                        b["is_error"] = json!(false);
                    }
                }
            }
            "anthropic"
        }
        "responses" => {
            req["stream"] = json!(true);
            "open_ai"
        }
        "chat" => {
            req["stream"] = json!(true);
            if let Some(n) = req.as_object_mut().and_then(|o| o.remove("max_tokens")) {
                req["max_completion_tokens"] = n;
            }
            "x_ai"
        }
        "gemini" => {
            let o = req.as_object_mut().ok_or("invalid Zed Gemini request")?;
            for key in ["session_id", "sessionId", "stream"] {
                o.remove(key);
            }
            req["model"] = json!(format!("models/{model}"));
            "google"
        }
        _ => return Err("unsupported Zed provider protocol".into()),
    };
    Ok(json!({"provider":vendor,"model":model,"provider_request":req}))
}
pub struct ZedStream {
    wire: String,
    wrapped: bool,
    lines: Lines,
    ended: bool,
    native_done: bool,
    events: usize,
    pub failure_status: Option<u16>,
    pending: Vec<Vec<u8>>,
    pending_bytes: usize,
    useful: bool,
}
impl ZedStream {
    pub fn new(wire: &str, wrapped: bool) -> Self {
        Self {
            wire: wire.into(),
            wrapped,
            lines: Lines::default(),
            ended: false,
            native_done: false,
            events: 0,
            failure_status: None,
            pending: vec![],
            pending_bytes: 0,
            useful: false,
        }
    }
    pub fn push(&mut self, data: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        let lines = self.lines.push(data)?;
        self.decode(lines)
    }
    fn decode(&mut self, lines: Vec<Vec<u8>>) -> Result<Vec<Vec<u8>>, String> {
        let mut out = vec![];
        for line in lines {
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let v: Value =
                serde_json::from_slice(&line).map_err(|_| "unreadable Zed stream line")?;
            if self.wrapped {
                if v["status"] == "stream_ended" {
                    self.ended = true;
                    continue;
                }
                if let Some(f) = v.pointer("/status/failed") {
                    self.failure_status = Some(status(f["code"].as_str().unwrap_or("")));
                    return Err("Zed rejected the generation".into());
                }
                if v.get("event").is_none() {
                    continue;
                }
            }
            if self.ended {
                return Err("Zed sent events after stream_ended".into());
            }
            let ev = if self.wrapped { &v["event"] } else { &v };
            if ev.get("error").is_some() || ev["type"] == "error" || ev["type"] == "response.failed"
            {
                self.failure_status = Some(super::failure_status(&ev.to_string()));
                return Err("Zed upstream reported a stream error".into());
            }
            self.events += 1;
            match self.wire.as_str() {
                "anthropic" => self.native_done |= ev["type"] == "message_stop",
                "responses" => {
                    self.native_done |=
                        ev["type"] == "response.completed" || ev["type"] == "response.incomplete"
                }
                "chat" => {
                    self.native_done |= ev["choices"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|c| c["finish_reason"].is_string())
                }
                "gemini" => {
                    self.native_done |= ev["candidates"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|c| c["finishReason"].is_string())
                }
                _ => {}
            }
            let prefix = if matches!(self.wire.as_str(), "anthropic" | "responses") {
                ev["type"]
                    .as_str()
                    .map(|s| format!("event: {s}\n"))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            let data = format!("{prefix}data: {ev}\n\n").into_bytes();
            self.useful |= self.native_done || meaningful(&self.wire, ev);
            if self.useful {
                out.append(&mut self.pending);
                out.push(data);
            } else {
                self.pending_bytes += data.len();
                if self.pending_bytes > super::connect::MAX_FRAME {
                    return Err("Zed prelude exceeds limit".into());
                }
                self.pending.push(data);
            }
        }
        Ok(out)
    }
    pub fn finish(&mut self) -> Result<Vec<Vec<u8>>, String> {
        let lines = self.lines.finish();
        let mut out = self.decode(lines)?;
        if self.events == 0 || (!self.ended && self.wrapped) || !self.native_done {
            return Err("Zed reply ended before completion".into());
        }
        if self.wire == "chat" {
            out.push(b"data: [DONE]\n\n".to_vec());
        }
        Ok(out)
    }
}
fn meaningful(wire: &str, ev: &Value) -> bool {
    let nonempty = |v: &Value| v.as_str().is_some_and(|s| !s.is_empty());
    match wire {
        "anthropic" => {
            ev["content_block"]["type"] == "tool_use"
                || ["text", "thinking", "partial_json", "signature"]
                    .iter()
                    .any(|k| nonempty(&ev["delta"][k]))
                || nonempty(&ev["content_block"]["text"])
        }
        "responses" => {
            nonempty(&ev["delta"])
                || ev["item"]["type"] == "function_call"
                || ev["item"]["type"] == "custom_tool_call"
        }
        "chat" => ev["choices"].as_array().into_iter().flatten().any(|c| {
            nonempty(&c["delta"]["content"])
                || nonempty(&c["delta"]["reasoning_content"])
                || c["delta"]["tool_calls"]
                    .as_array()
                    .is_some_and(|t| !t.is_empty())
        }),
        "gemini" => ev["candidates"].as_array().into_iter().flatten().any(|c| {
            c["content"]["parts"]
                .as_array()
                .is_some_and(|p| !p.is_empty())
        }),
        _ => false,
    }
}
fn status(code: &str) -> u16 {
    for p in ["upstream_http_", "http_"] {
        if let Some(n) = code
            .strip_prefix(p)
            .and_then(|s| s.parse::<u16>().ok())
            .filter(|n| (400..600).contains(n))
        {
            return n;
        }
    }
    if code.contains("rate_limit") {
        429
    } else if code.contains("overloaded") {
        529
    } else if code.contains("billing") || code.contains("payment") {
        402
    } else if code.contains("context_length") {
        400
    } else {
        502
    }
}

impl super::ProviderStream for ZedStream {
    fn has_output(&self) -> bool {
        self.useful
    }
    fn push(&mut self, b: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        ZedStream::push(self, b)
    }
    fn finish(&mut self) -> Result<Vec<Vec<u8>>, String> {
        ZedStream::finish(self)
    }
    fn failure_status(&self) -> Option<u16> {
        self.failure_status
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framing_events_do_not_commit_success_before_a_quota_failure() {
        let mut s = ZedStream::new("anthropic", true);
        assert!(s
            .push(b"{\"event\":{\"type\":\"message_start\",\"message\":{}}}\n")
            .unwrap()
            .is_empty());
        assert!(s
            .push(b"{\"status\":{\"failed\":{\"code\":\"upstream_http_429\"}}}\n")
            .is_err());
        assert_eq!(s.failure_status, Some(429));
    }
    #[test]
    fn native_tool_error_defaults_preserve_signed_thinking() {
        let v = json!({"messages":[{"role":"assistant","content":[{"type":"thinking","thinking":"think","signature":"sig"}]},{"role":"user","content":[{"type":"tool_result","tool_use_id":"a","content":"ok"}]}],"stream":false});
        let out = request("anthropic", "claude", v.clone()).unwrap();
        assert_eq!(out["provider_request"]["messages"][0], v["messages"][0]);
        assert_eq!(
            out["provider_request"]["messages"][1]["content"][0]["is_error"],
            false
        );
        assert!(out["provider_request"].get("stream").is_none());
    }
    #[test]
    fn status_frame_cannot_hide_truncated_provider_response() {
        let mut s = ZedStream::new("responses", true);
        s.push(b"{\"event\":{\"type\":\"response.created\"}}\n{\"status\":\"stream_ended\"}\n")
            .unwrap();
        assert!(s.finish().is_err());
        let mut s = ZedStream::new("responses", true);
        s.push(b"{\"event\":{\"type\":\"response.completed\"}}\n{\"status\":\"stream_ended\"}\n")
            .unwrap();
        s.finish().unwrap();
    }
}
