use super::connect::*;
use serde_json::{json, Value};
fn double(id: u32, value: f64) -> Vec<u8> {
    [varint((id as u64) << 3 | 1), value.to_le_bytes().to_vec()].concat()
}
fn message(
    role: u64,
    text: &str,
    calls: &[Value],
    call_id: &str,
    images: &[Value],
) -> Result<Vec<u8>, String> {
    let mut out = [string(1, &uuid::Uuid::new_v4().to_string()), uint(2, role)].concat();
    if !text.is_empty() {
        out.extend(string(3, text));
    }
    for call in calls {
        let args = call["function"]["arguments"].as_str().unwrap_or("{}");
        let _: Value = serde_json::from_str(args).map_err(|_| "invalid Devin tool arguments")?;
        out.extend(bytes(
            6,
            &[
                string(1, call["id"].as_str().ok_or("tool call id missing")?),
                string(
                    2,
                    call["function"]["name"]
                        .as_str()
                        .ok_or("tool name missing")?,
                ),
                string(3, args),
            ]
            .concat(),
        ));
    }
    if !call_id.is_empty() {
        out.extend(string(7, call_id));
    }
    for image in images {
        let url = image["image_url"]
            .as_str()
            .or(image["image_url"]["url"].as_str())
            .ok_or("Devin image URL missing")?;
        let (mime, data) = url
            .strip_prefix("data:")
            .and_then(|s| s.split_once(","))
            .ok_or("Devin requires inline base64 images")?;
        if !mime.ends_with(";base64") {
            return Err("Devin requires base64 images".into());
        }
        out.extend(bytes(
            10,
            &[string(1, data), string(2, mime.trim_end_matches(";base64"))].concat(),
        ));
    }
    Ok(out)
}
pub fn request(chat: &Value, uid: &str, key: &str, os: &str) -> Result<Vec<u8>, String> {
    let mut system = vec![];
    let mut tools = vec![];
    if chat["tool_choice"] != "none" {
        for t in chat["tools"].as_array().into_iter().flatten() {
            if t["type"] != "function" {
                return Err("Devin only supports function tools".into());
            }
            tools.push(t.clone());
        }
    }
    if chat["tool_choice"] == "required" || chat["tool_choice"].is_object() {
        return Err("Devin cannot enforce this tool choice".into());
    }
    for m in chat["messages"]
        .as_array()
        .ok_or("Devin messages must be an array")?
    {
        if m["role"] == "system" || m["role"] == "developer" {
            system.push(text(&m["content"]));
        }
    }
    let descriptions = tools
        .iter()
        .filter(|t| {
            t["function"]["description"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
        })
        .map(|t| {
            format!(
                "<tool name=\"{}\">\n{}\n</tool>\n",
                t["function"]["name"].as_str().unwrap_or(""),
                t["function"]["description"].as_str().unwrap_or("")
            )
        })
        .collect::<String>();
    if !descriptions.is_empty() {
        system.push(format!(
            "<tool_descriptions>\n{descriptions}</tool_descriptions>"
        ));
    }
    let instructions = system.join("\n\n");
    let mut turns = vec![];
    let mut pending: Vec<Value> = vec![];
    let mut history_tools = std::collections::BTreeSet::new();
    let mut first_user = true;
    for m in chat["messages"].as_array().unwrap() {
        let role = m["role"].as_str().unwrap_or("");
        if matches!(role, "system" | "developer") {
            continue;
        }
        if role == "tool" {
            let id = m["tool_call_id"].as_str().unwrap_or("");
            let pos = pending
                .iter()
                .position(|c| c["id"] == id)
                .ok_or("Devin tool result has no unique matching call")?;
            pending.remove(pos);
            turns.push(message(4, &text(&m["content"]), &[], id, &[])?);
            continue;
        }
        for call in pending.drain(..) {
            turns.push(message(
                4,
                "Tool use was interrupted and did not produce a result.",
                &[],
                call["id"].as_str().unwrap_or(""),
                &[],
            )?);
        }
        let calls = m["tool_calls"].as_array().cloned().unwrap_or_default();
        let mut content = text(&m["content"]);
        let images: Vec<_> = m["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| p["type"] == "image_url")
            .cloned()
            .collect();
        if role == "user" && first_user {
            first_user = false;
            if !instructions.is_empty() {
                content = format!("{instructions}\n\n{content}");
            }
        }
        if role == "assistant" && content.is_empty() && calls.is_empty() {
            continue;
        }
        for call in &calls {
            history_tools.insert(
                call["function"]["name"]
                    .as_str()
                    .ok_or("tool name missing")?
                    .to_owned(),
            );
        }
        turns.push(message(
            if role == "assistant" { 2 } else { 1 },
            &content,
            &calls,
            "",
            &images,
        )?);
        pending = calls;
    }
    for call in pending.drain(..) {
        turns.push(message(
            4,
            "Tool use was interrupted and did not produce a result.",
            &[],
            call["id"].as_str().unwrap_or(""),
            &[],
        )?);
    }
    if first_user {
        turns.insert(
            0,
            message(
                1,
                &format!("{instructions}\n\nPlease proceed with the task."),
                &[],
                "",
                &[],
            )?,
        );
    }
    let meta = [
        string(1, "devin-cli"),
        string(2, "3000.11.3"),
        string(3, key),
        string(4, "en"),
        string(5, os),
        string(7, "3000.11.3"),
        string(12, "chisel"),
        string(28, "chisel"),
    ]
    .concat();
    let mut out = bytes(1, &meta);
    for msg in turns {
        out.extend(bytes(3, &msg));
    }
    out.extend(uint(7, 5));
    let params = [
        uint(1, 1),
        uint(
            2,
            chat["max_completion_tokens"]
                .as_u64()
                .or(chat["max_tokens"].as_u64())
                .unwrap_or(128000),
        ),
        uint(3, 400),
        double(5, chat["temperature"].as_f64().unwrap_or(1.0)),
        uint(7, 40),
        double(8, chat["top_p"].as_f64().unwrap_or(0.95)),
    ]
    .concat();
    out.extend(bytes(8, &params));
    let mut offered = std::collections::BTreeSet::new();
    for tool in tools {
        let f = &tool["function"];
        let name = f["name"].as_str().ok_or("tool name missing")?;
        offered.insert(name.to_owned());
        let desc = if f["description"].as_str().is_some_and(|s| !s.is_empty()) {
            format!("Described under <tool name=\"{name}\"> in <tool_descriptions>, in the instructions.")
        } else {
            name.into()
        };
        out.extend(bytes(
            10,
            &[
                string(1, name),
                string(2, &desc),
                string(3, &f["parameters"].to_string()),
            ]
            .concat(),
        ));
    }
    for name in history_tools.difference(&offered) {
        out.extend(bytes(
            10,
            &[
                string(1, name),
                string(2, "Tool"),
                string(3, "{\"type\":\"object\",\"properties\":{}}"),
            ]
            .concat(),
        ));
    }
    out.extend(string(21, uid));
    Ok(frame(0, &out))
}
pub fn variant(families: &Value, model: &str, effort: &str) -> String {
    let rank = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];
    let effort = if effort == "ultra" { "max" } else { effort };
    for family in families.as_array().into_iter().flatten() {
        let uid = family["uid"].as_str().unwrap_or("");
        let named = |id: &str| {
            id == uid
                || family["aliases"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|v| v == id)
        };
        let tier = if named(model) {
            ""
        } else if model.strip_suffix("-fast").is_some_and(named) {
            "fast"
        } else if model.strip_suffix("-priority").is_some_and(named) {
            "priority"
        } else {
            continue;
        };
        let models = family["models"].as_array().cloned().unwrap_or_default();
        if tier.is_empty() && effort.is_empty() {
            return models
                .first()
                .and_then(|v| v["id"].as_str())
                .unwrap_or(model)
                .into();
        }
        let at = rank.iter().position(|e| *e == effort).unwrap_or(3) as i32;
        let best = models
            .iter()
            .filter_map(|m| {
                let id = m["id"].as_str()?;
                let lower = id.to_lowercase().replace('_', "-");
                let stripped = if tier.is_empty() {
                    lower.as_str()
                } else {
                    lower.strip_suffix(&format!("-{tier}"))?
                };
                let level = stripped.rsplit('-').next()?;
                let n = rank.iter().position(|l| *l == level)? as i32;
                Some(((n - at).abs(), -n, id))
            })
            .min();
        if let Some((_, _, id)) = best {
            return id.into();
        }
    }
    model.into()
}
pub struct DevinStream {
    status: Option<u16>,
    frames: Frames,
    chat: ChatStream,
    ended: bool,
    stop: u64,
    usage: [u64; 4],
    tool: Option<usize>,
    arguments: ToolArguments,
}
impl DevinStream {
    pub fn new(model: &str, id: &str) -> Self {
        Self {
            status: None,
            frames: Frames::default(),
            chat: ChatStream::new(model, id),
            ended: false,
            stop: 0,
            usage: [0; 4],
            tool: None,
            arguments: ToolArguments::default(),
        }
    }
    pub fn push(&mut self, data: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        let mut out = vec![];
        for (flags, data) in self.frames.push(data)? {
            if self.ended {
                return Err("Devin sent data after Connect completion".into());
            }
            let data = uncompress(flags, &data)?;
            if flags & 2 != 0 {
                let end: Value = serde_json::from_slice(&data)
                    .map_err(|_| "invalid Devin Connect completion")?;
                if !end["error"].is_null() {
                    self.status = Some(super::failure_status(&end["error"].to_string()));
                    return Err("Devin returned a Connect error".into());
                }
                self.ended = true;
                continue;
            }
            for (n, f) in fields(&data)? {
                match (n, f) {
                    (3 | 9, Field::Bytes(b)) => {
                        let text = std::str::from_utf8(b).map_err(|_| "invalid Devin text")?;
                        if !text.is_empty() {
                            out.extend(self.chat.delta(if n == 3 {
                                json!({"content":text})
                            } else {
                                json!({"reasoning_content":text})
                            }));
                        }
                    }
                    (6, Field::Bytes(b)) => {
                        let f = fields(b)?;
                        let name = field_text(&f, 2);
                        if !name.is_empty() {
                            if self.tool.is_some() {
                                self.arguments.finish()?;
                            }
                            if self.chat.tools >= 1024 {
                                return Err("Devin tool count exceeds limit".into());
                            }
                            let i = self.chat.tools;
                            self.chat.tools += 1;
                            self.tool = Some(i);
                            self.arguments = ToolArguments::default();
                            let id = field_text(&f, 1);
                            out.extend(self.chat.delta(json!({"tool_calls":[{"index":i,"id":if id.is_empty(){format!("{}_tool_{i}",self.chat.id)}else{id},"type":"function","function":{"name":name,"arguments":""}}]})));
                        }
                        for (n, value) in f {
                            if let (3, Field::Bytes(b)) = (n, value) {
                                let args = std::str::from_utf8(b)
                                    .map_err(|_| "invalid Devin tool arguments")?;
                                if !args.is_empty() {
                                    let i = self.tool.ok_or("Devin tool arguments have no call")?;
                                    self.arguments.push(args)?;
                                    out.extend(self.chat.delta(json!({"tool_calls":[{"index":i,"function":{"arguments":args}}]})));
                                }
                            }
                        }
                    }
                    (5, Field::Int(n)) => self.stop = n,
                    (7, Field::Bytes(b)) => {
                        let f = fields(b)?;
                        for (i, n) in [2, 3, 4, 5].into_iter().enumerate() {
                            let v = field_int(&f, n);
                            if v > 0 {
                                self.usage[i] = v;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(out)
    }
    pub fn finish(&mut self) -> Result<Vec<Vec<u8>>, String> {
        self.frames.finish()?;
        if !self.ended {
            return Err("Devin reply ended without Connect completion".into());
        }
        if self.tool.is_some() {
            self.arguments.finish()?;
        }
        let prompt = self.usage[0]
            .checked_add(self.usage[2])
            .and_then(|n| n.checked_add(self.usage[3]))
            .ok_or("Devin token usage overflow")?;
        let total = prompt
            .checked_add(self.usage[1])
            .ok_or("Devin token usage overflow")?;
        let usage = json!({"prompt_tokens":prompt,"completion_tokens":self.usage[1],"total_tokens":total,"prompt_tokens_details":{"cached_tokens":self.usage[3],"cache_write_tokens":self.usage[2]}});
        self.chat.stop(
            if self.stop == 10 {
                "tool_calls"
            } else if self.stop == 3 {
                "length"
            } else {
                "stop"
            },
            Some(usage),
        )
    }
}

impl super::ProviderStream for DevinStream {
    fn has_output(&self) -> bool {
        self.chat.begun || self.chat.done
    }
    fn push(&mut self, b: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        DevinStream::push(self, b)
    }
    fn finish(&mut self) -> Result<Vec<Vec<u8>>, String> {
        DevinStream::finish(self)
    }
    fn failure_status(&self) -> Option<u16> {
        self.status
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tool_json_must_be_complete_before_the_next_call_or_success() {
        let call = |args: &str| {
            frame(
                0,
                &bytes(
                    6,
                    &[string(1, "call"), string(2, "calc"), string(3, args)].concat(),
                ),
            )
        };
        let mut s = DevinStream::new("m", "id");
        s.push(&call("{\"n\":")).unwrap();
        s.push(&frame(2, b"{}")).unwrap();
        assert!(s.finish().is_err());
        let mut s = DevinStream::new("m", "id");
        s.push(&call("{\"n\":")).unwrap();
        assert!(s.push(&call("{}")).is_err());
        let mut s = DevinStream::new("m", "id");
        s.push(&call("{\"n\":")).unwrap();
        s.push(&frame(0, &bytes(6, &string(3, "3}")))).unwrap();
        s.push(&frame(2, b"{}")).unwrap();
        assert!(String::from_utf8(s.finish().unwrap().concat())
            .unwrap()
            .contains("[DONE]"));
    }
    #[test]
    fn malformed_token_usage_returns_error_without_overflow() {
        let mut s = DevinStream::new("m", "id");
        s.push(&frame(
            0,
            &bytes(7, &[uint(2, u64::MAX), uint(3, 1)].concat()),
        ))
        .unwrap();
        s.push(&frame(2, b"{}")).unwrap();
        assert!(s.finish().is_err());
    }
    #[test]
    fn model_effort_ties_prefer_higher_and_keep_fast_tier() {
        let f = json!([{"uid":"claude","models":[{"id":"claude-low"},{"id":"claude-high"},{"id":"claude-high-fast"},{"id":"claude-low-fast"}]}]);
        assert_eq!(variant(&f, "claude", "medium"), "claude-high");
        assert_eq!(variant(&f, "claude-fast", "medium"), "claude-high-fast");
    }
    #[test]
    fn late_usage_survives_until_connect_end() {
        let mut s = DevinStream::new("model", "id");
        s.push(&frame(0, &string(3, "hello"))).unwrap();
        assert!(s.finish().is_err());
        s.push(&frame(
            0,
            &bytes(7, &[uint(2, 10), uint(3, 4), uint(5, 7)].concat()),
        ))
        .unwrap();
        s.push(&frame(2, b"{}")).unwrap();
        let out = String::from_utf8(s.finish().unwrap().concat()).unwrap();
        assert!(out.contains("\"prompt_tokens\":17"));
        assert!(out.contains("\"cached_tokens\":7"));
    }
}
