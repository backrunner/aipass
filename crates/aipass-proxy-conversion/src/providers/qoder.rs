use super::connect::{text, ChatStream, Lines, ToolArguments, MAX_FRAME};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
const ALPHABET: &[u8] = b"_doRTgHZBKcGVjlvpC,@aFSx#DPuNJme&i*MzLOEn)sUrthbf%Y^w.(kIQyXqWA!";
const NORMAL: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
pub fn encode_body(bytes: &[u8]) -> String {
    let s: Vec<u8> = STANDARD
        .encode(bytes)
        .bytes()
        .map(|b| {
            if b == b'=' {
                b'$'
            } else {
                ALPHABET[NORMAL.iter().position(|x| *x == b).unwrap()]
            }
        })
        .collect();
    let n = s.len() / 3;
    String::from_utf8([&s[s.len() - n..], &s[n..s.len() - n], &s[..n]].concat()).unwrap()
}
pub fn decode_body(s: &str) -> Result<Vec<u8>, String> {
    let s = s.as_bytes();
    if s.len() > MAX_FRAME || !s.len().is_multiple_of(4) {
        return Err("invalid Qoder body length".into());
    }
    let n = s.len() / 3;
    let s = [&s[s.len() - n..], &s[n..s.len() - n], &s[..n]].concat();
    let mapped = s
        .into_iter()
        .map(|b| {
            if b == b'$' {
                Ok(b'=')
            } else {
                ALPHABET
                    .iter()
                    .position(|x| *x == b)
                    .map(|n| NORMAL[n])
                    .ok_or("invalid Qoder body alphabet")
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    STANDARD
        .decode(mapped)
        .map_err(|_| "invalid Qoder body encoding".into())
}
pub fn request(chat: &Value, config: &Value, session: &str, now: u64) -> Result<Value, String> {
    let messages = chat["messages"]
        .as_array()
        .ok_or("Qoder messages must be an array")?;
    let system = messages
        .iter()
        .filter(|m| m["role"] == "system" || m["role"] == "developer")
        .map(|m| text(&m["content"]))
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut system=format!("You are a Qoder agent. Use the instructions below and the tools available to you to assist the user.\n\n{system}");
    let mut tools = vec![];
    if chat["tool_choice"] != "none" {
        for t in chat["tools"].as_array().into_iter().flatten() {
            if t["type"] != "function" {
                return Err("Qoder only supports function tools".into());
            }
            tools.push(t.clone());
        }
    }
    if chat["tool_choice"] == "required" && !tools.is_empty() {
        system.push_str("\nYou must call an available function in this response.");
    }
    if chat["tool_choice"].is_object() {
        return Err("Qoder cannot force a named function".into());
    }
    let sys = json!({"type":"text","text":system});
    let mut turns = vec![json!({"role":"system","content":[sys.clone()]})];
    let mut pending_images = vec![];
    for m in messages {
        let role = m["role"].as_str().unwrap_or("");
        if matches!(role, "system" | "developer") {
            continue;
        }
        let mut m = m.clone();
        if role == "tool" {
            let mut t = text(&m["content"]);
            for p in m["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|p| p["type"] == "image_url")
            {
                pending_images.push(json!({"type":"text","text":format!("[From tool call {}:]",m["tool_call_id"].as_str().unwrap_or(""))}));
                pending_images.push(p.clone());
            }
            if !pending_images.is_empty() {
                t.push_str("\n[Tool images follow in the next user message.]");
            }
            m["content"] = json!(t);
            turns.push(m);
            continue;
        }
        if role != "user" && !pending_images.is_empty() {
            turns.push(json!({"role":"user","content":std::mem::take(&mut pending_images)}));
        }
        if role == "assistant" && m["tool_calls"].is_array() {
            m["content"] = json!(text(&m["content"]));
            turns.push(m);
            continue;
        }
        let mut blocks = m["content"]
            .as_array()
            .cloned()
            .unwrap_or_else(|| vec![json!({"type":"text","text":m["content"]})]);
        for p in &blocks {
            if !matches!(p["type"].as_str(), Some("text" | "image_url")) {
                return Err("unsupported Qoder content block".into());
            }
        }
        if role == "assistant" && blocks.iter().any(|p| p["type"] != "text") {
            return Err("Qoder assistant history cannot contain images".into());
        }
        if role == "user" && !pending_images.is_empty() {
            let mut images = std::mem::take(&mut pending_images);
            images.append(&mut blocks);
            blocks = images;
        }
        m["content"] = json!(blocks);
        turns.push(m);
    }
    if !pending_images.is_empty() {
        turns.push(json!({"role":"user","content":pending_images}));
    }
    let tc = &config["thinking_config"];
    let thinks = if tc.is_object() {
        !tc["enabled"].is_null()
    } else {
        config["is_reasoning"] == true
    };
    let always = tc.is_object() && !tc["enabled"].is_null() && tc["disabled"].is_null();
    let asked = chat["reasoning_effort"].as_str().unwrap_or("");
    let on = thinks && (asked != "none" || always);
    let mut params = json!({"enable_thinking":on,"max_tokens":chat["max_completion_tokens"].as_u64().or(chat["max_tokens"].as_u64()).unwrap_or(32000)});
    let levels: Vec<_> = tc["enabled"]["efforts"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, _)| k.as_str())
        .collect();
    if on && !levels.is_empty() {
        let rank = [
            "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
        ];
        let effort = if asked.is_empty() {
            tc["enabled"]["efforts"]
                .as_object()
                .into_iter()
                .flatten()
                .find(|(_, v)| v["is_default"] == true)
                .map(|(k, _)| k.as_str())
                .unwrap_or("")
        } else if levels.contains(&asked) {
            asked
        } else {
            let at = rank.iter().position(|e| *e == asked).unwrap_or(3) as i32;
            levels
                .iter()
                .copied()
                .min_by_key(|l| {
                    let n = rank.iter().position(|e| e == l).unwrap_or(3) as i32;
                    ((n - at).abs(), -n)
                })
                .unwrap_or("")
        };
        if !effort.is_empty() {
            params["reasoning_effort"] = json!(effort);
        }
    }
    if let Some(n) = config["max_input_tokens"].as_u64().filter(|n| *n > 0) {
        params["context_length"] = json!(n);
    }
    let mut body = json!({"parameters":params,"business":{"product":"app","version":"1.1.49","type":"agent","id":session,"name":"AIPass session","begin_at":now,"stage":"start"},"agent_id":"agent_common","task_id":"common","session_type":"app","model_config":config,"system":[sys],"messages":turns});
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    Ok(body)
}
fn parse_call(raw: &str) -> Result<(String, Value), String> {
    if let Ok(v) = serde_json::from_str::<Value>(raw) {
        if let Some(name) = v["name"].as_str().filter(|s| !s.trim().is_empty()) {
            if v["arguments"].is_object() {
                return Ok((name.trim().into(), v["arguments"].clone()));
            }
        }
    }
    let start = raw
        .find("<function=")
        .ok_or("invalid Qoder XML tool call")?
        + 10;
    let end = start + raw[start..].find('>').ok_or("invalid Qoder XML function")?;
    let name = raw[start..end].trim();
    if name.is_empty() {
        return Err("Qoder tool name missing".into());
    }
    let close = raw[end + 1..]
        .find("</function>")
        .ok_or("unterminated Qoder function")?
        + end
        + 1;
    let mut rest = &raw[end + 1..close];
    let mut args = json!({});
    while let Some(i) = rest.find("<parameter=") {
        rest = &rest[i + 11..];
        let end = rest.find('>').ok_or("invalid Qoder parameter")?;
        let key = rest[..end].trim();
        rest = &rest[end + 1..];
        let end = rest
            .find("</parameter>")
            .ok_or("unterminated Qoder parameter")?;
        let value = rest[..end].trim();
        args[key] = serde_json::from_str(value).unwrap_or_else(|_| json!(value));
        rest = &rest[end + 12..];
    }
    Ok((name.into(), args))
}
#[derive(Default)]
struct NativeTool {
    index: usize,
    id: String,
    name: String,
    arguments: ToolArguments,
}
pub struct QoderStream {
    status: Option<u16>,
    lines: Lines,
    chat: ChatStream,
    buffer: String,
    in_call: bool,
    finish: String,
    usage: Option<Value>,
    native: std::collections::HashMap<u64, NativeTool>,
}
impl QoderStream {
    pub fn new(model: &str, id: &str) -> Self {
        Self {
            status: None,
            lines: Lines::default(),
            chat: ChatStream::new(model, id),
            buffer: String::new(),
            in_call: false,
            finish: String::new(),
            usage: None,
            native: std::collections::HashMap::new(),
        }
    }
    fn text(&mut self, text: &str) -> Result<Vec<Vec<u8>>, String> {
        self.buffer.push_str(text);
        if self.buffer.len() > MAX_FRAME {
            return Err("Qoder tool buffer exceeds limit".into());
        }
        let mut out = vec![];
        loop {
            let tag = if self.in_call {
                "</tool_call>"
            } else {
                "<tool_call>"
            };
            if let Some(i) = self.buffer.find(tag) {
                let part = self.buffer[..i].to_owned();
                self.buffer = self.buffer[i + tag.len()..].into();
                if self.in_call {
                    let (name, args) = parse_call(&part)?;
                    let index = self.chat.tools;
                    self.chat.tools += 1;
                    out.extend(self.chat.delta(json!({"tool_calls":[{"index":index,"id":format!("{}_tool_{index}",self.chat.id),"type":"function","function":{"name":name,"arguments":args.to_string()}}]})));
                } else if !part.is_empty() {
                    out.extend(self.chat.delta(json!({"content":part})));
                }
                self.in_call = !self.in_call;
                continue;
            }
            if !self.in_call {
                let keep = (1..tag.len())
                    .rev()
                    .find(|n| self.buffer.ends_with(&tag[..*n]))
                    .unwrap_or(0);
                let n = self.buffer.len() - keep;
                if n > 0 {
                    out.extend(self.chat.delta(json!({"content":self.buffer[..n]})));
                    self.buffer = self.buffer[n..].into();
                }
            }
            break;
        }
        Ok(out)
    }
    pub fn push(&mut self, data: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        let lines = self.lines.push(data)?;
        self.decode(lines)
    }
    fn decode(&mut self, lines: Vec<Vec<u8>>) -> Result<Vec<Vec<u8>>, String> {
        let mut out = vec![];
        for line in lines {
            let line = std::str::from_utf8(&line)
                .map_err(|_| "invalid Qoder stream UTF-8")?
                .trim();
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            if data.trim() == "[DONE]" {
                continue;
            }
            let env: Value =
                serde_json::from_str(data).map_err(|_| "invalid Qoder stream envelope")?;
            if env["statusCodeValue"].as_u64().is_some_and(|n| n != 200) {
                let code = env["statusCodeValue"].as_u64().unwrap_or(502);
                self.status = Some(if (400..600).contains(&code) {
                    code as u16
                } else {
                    super::failure_status(&env.to_string())
                });
                return Err("Qoder returned a stream error".into());
            }
            let inner = env["body"].as_str().ok_or("Qoder stream body missing")?;
            let chunk: Value =
                serde_json::from_str(inner).map_err(|_| "invalid Qoder stream chunk")?;
            let d = &chunk["choices"][0]["delta"];
            if let Some(s) = d["reasoning_content"].as_str() {
                out.extend(self.chat.delta(json!({"reasoning_content":s})));
            }
            if let Some(s) = d["content"].as_str() {
                out.extend(self.text(s)?);
            }
            for t in d["tool_calls"].as_array().into_iter().flatten() {
                if !t.is_object() || t.get("function").is_some_and(|v| !v.is_object()) {
                    return Err("invalid Qoder tool call".into());
                }
                let index = t["index"].as_u64().unwrap_or(0);
                if !self.native.contains_key(&index) && self.native.len() >= 1024 {
                    return Err("Qoder tool count exceeds limit".into());
                }
                let next = self.chat.tools;
                let tool = self.native.entry(index).or_insert_with(|| {
                    self.chat.tools += 1;
                    NativeTool {
                        index: next,
                        ..Default::default()
                    }
                });
                for (value, field) in [
                    (t.get("id"), &mut tool.id),
                    (t["function"].get("name"), &mut tool.name),
                ] {
                    if let Some(value) = value {
                        let value = value.as_str().ok_or("invalid Qoder tool identity")?;
                        if field.len().saturating_add(value.len()) > 4096 {
                            return Err("Qoder tool identity exceeds limit".into());
                        }
                        field.push_str(value);
                    }
                }
                let mut t = t.clone();
                t["index"] = json!(tool.index);
                if let Some(args) = t["function"].get("arguments") {
                    tool.arguments
                        .push(args.as_str().ok_or("invalid Qoder tool arguments")?)?;
                }
                out.extend(self.chat.delta(json!({"tool_calls":[t]})));
            }
            if let Some(reason) = chunk["choices"][0]["finish_reason"].as_str() {
                self.finish = reason.into();
            }
            if chunk["usage"].is_object() {
                self.usage = Some(chunk["usage"].clone());
            }
        }
        Ok(out)
    }
    pub fn finish(&mut self) -> Result<Vec<Vec<u8>>, String> {
        let lines = self.lines.finish();
        let mut out = self.decode(lines)?;
        if self.in_call || self.finish.is_empty() {
            return Err("Qoder reply ended before completion".into());
        }
        for tool in self.native.values() {
            if tool.id.is_empty() || tool.name.is_empty() {
                return Err("incomplete Qoder tool identity".into());
            }
            tool.arguments.finish()?;
        }
        if !self.buffer.is_empty() {
            out.extend(
                self.chat
                    .delta(json!({"content":std::mem::take(&mut self.buffer)})),
            );
        }
        out.extend(self.chat.stop(&self.finish, self.usage.take())?);
        Ok(out)
    }
}

impl super::ProviderStream for QoderStream {
    fn has_output(&self) -> bool {
        self.chat.begun || self.chat.done || self.in_call || !self.buffer.is_empty()
    }
    fn push(&mut self, b: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        QoderStream::push(self, b)
    }
    fn finish(&mut self) -> Result<Vec<Vec<u8>>, String> {
        QoderStream::finish(self)
    }
    fn failure_status(&self) -> Option<u16> {
        self.status
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_tool_calls_require_valid_shapes_and_complete_json() {
        let line = |tool: Value, reason: Value| {
            format!(
                "data: {}\n\n",
                json!({"statusCodeValue":200,"body":json!({"choices":[{"delta":{"tool_calls":[tool]},"finish_reason":reason}]}).to_string()})
            )
        };
        let mut s = QoderStream::new("m", "id");
        assert!(s.push(line(json!(1), Value::Null).as_bytes()).is_err());
        let mut s = QoderStream::new("m", "id");
        s.push(
            line(
                json!({"index":0,"id":"call","function":{"name":"calc","arguments":"{\"n\":"}}),
                json!("tool_calls"),
            )
            .as_bytes(),
        )
        .unwrap();
        assert!(s.finish().is_err());
        s.push(
            line(
                json!({"index":0,"function":{"arguments":"1}"}}),
                Value::Null,
            )
            .as_bytes(),
        )
        .unwrap();
        s.finish().unwrap();
    }
    #[test]
    fn native_tool_identity_can_arrive_in_separate_deltas() {
        let mut s = QoderStream::new("m", "id");
        for t in [
            json!({"index":7,"id":"call","type":"function"}),
            json!({"index":7,"function":{"name":"calc","arguments":"{}"}}),
        ] {
            let line = format!(
                "data: {}\n\n",
                json!({"statusCodeValue":200,"body":json!({"choices":[{"delta":{"tool_calls":[t]},"finish_reason":"tool_calls"}]}).to_string()})
            );
            s.push(line.as_bytes()).unwrap();
        }
        s.finish().unwrap();
    }
    #[test]
    fn late_usage_and_native_tool_indexes_survive_envelopes() {
        let mut s = QoderStream::new("m", "id");
        let values = [
            json!({"choices":[{"delta":{"tool_calls":[{"index":7,"id":"call","type":"function","function":{"name":"calc","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}),
            json!({"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":3}}),
        ];
        let mut out = vec![];
        for v in values {
            let line = format!(
                "data: {}\n\n",
                json!({"statusCodeValue":200,"body":v.to_string()})
            );
            for p in line.as_bytes().chunks(1) {
                out.extend(s.push(p).unwrap());
            }
        }
        out.extend(s.finish().unwrap());
        let text = String::from_utf8(out.concat()).unwrap();
        assert!(text.contains("\"index\":0"));
        assert!(text.contains("\"prompt_tokens\":12"));
    }
    #[test]
    fn body_codec_handles_unicode_and_padding() {
        for value in ["", "a", "你好", "{\"model\":\"Qoder\"}"] {
            assert_eq!(
                decode_body(&encode_body(value.as_bytes())).unwrap(),
                value.as_bytes()
            );
        }
        assert!(decode_body("!!!!!!!?").is_err());
    }
    #[test]
    fn fragmented_xml_calls_are_structured_and_unfinished_ones_fail() {
        let mut s = QoderStream::new("m", "id");
        assert!(s.text("<tool_").unwrap().is_empty());
        let parts = s
            .text("call><function=calc><parameter=n>3</parameter></function></tool_call>")
            .unwrap();
        let joined = String::from_utf8(parts.concat()).unwrap();
        assert!(joined.contains("calc"));
        assert!(joined.contains("\\\"n\\\":3"));
        assert!(s.finish().is_err());
    }
}
