use super::connect::{text, ChatStream, Lines, ToolArguments};
use serde_json::{json, Value};
pub fn generate_body(chat: &Value, date: &str, environment: &str) -> Result<Value, String> {
    let (mut system, mut messages, mut results) = (vec![], vec![], vec![]);
    let mut names = std::collections::HashMap::new();
    for m in chat["messages"]
        .as_array()
        .ok_or("Command Code messages must be an array")?
    {
        let role = m["role"].as_str().unwrap_or("");
        if matches!(role, "system" | "developer") {
            system.push(text(&m["content"]));
            continue;
        }
        if role == "tool" {
            let id = m["tool_call_id"]
                .as_str()
                .ok_or("tool result is missing its call id")?;
            let name = names
                .remove(id)
                .ok_or("tool result has no unique matching call")?;
            results.push(json!({"type":"tool-result","toolCallId":id,"toolName":name,"output":{"type":"text","value":text(&m["content"])}}));
            continue;
        }
        if !results.is_empty() {
            messages.push(json!({"role":"tool","content":std::mem::take(&mut results)}));
        }
        let mut parts = vec![];
        if role == "assistant" {
            if let Some(t) = m["reasoning_content"]
                .as_str()
                .or(m["reasoning"].as_str())
                .filter(|s| !s.is_empty())
            {
                parts.push(json!({"type":"reasoning","text":t}));
            }
            let t = text(&m["content"]);
            if !t.is_empty() {
                parts.push(json!({"type":"text","text":t}));
            }
            for call in m["tool_calls"].as_array().into_iter().flatten() {
                let id = call["id"].as_str().ok_or("tool call is missing its id")?;
                let name = call["function"]["name"]
                    .as_str()
                    .ok_or("tool call is missing its name")?;
                if names.insert(id.to_owned(), name.to_owned()).is_some() {
                    return Err("duplicate Command Code tool id".into());
                }
                let input: Value =
                    serde_json::from_str(call["function"]["arguments"].as_str().unwrap_or("{}"))
                        .map_err(|_| "invalid tool arguments")?;
                parts.push(
                    json!({"type":"tool-call","toolCallId":id,"toolName":name,"input":input}),
                );
            }
        } else if let Some(t) = m["content"].as_str() {
            parts.push(json!({"type":"text","text":t}));
        } else {
            for p in m["content"].as_array().into_iter().flatten() {
                match p["type"].as_str() {
                    Some("text") => parts.push(p.clone()),
                    Some("image_url") => {
                        let url = p["image_url"]
                            .as_str()
                            .or(p["image_url"]["url"].as_str())
                            .ok_or("image URL missing")?;
                        let mut image = json!({"type":"image","image":url});
                        if let Some(mime) = url
                            .strip_prefix("data:")
                            .and_then(|s| s.split_once(';'))
                            .map(|p| p.0)
                        {
                            image["mimeType"] = json!(mime);
                        }
                        parts.push(image);
                    }
                    _ => return Err("unsupported Command Code input part".into()),
                }
            }
        }
        if !parts.is_empty() {
            messages.push(json!({"role":role,"content":parts}));
        }
    }
    if !results.is_empty() {
        messages.push(json!({"role":"tool","content":results}));
    }
    if chat["tool_choice"] == "required" || chat["tool_choice"].is_object() {
        return Err("Command Code Go cannot enforce this tool choice".into());
    }
    let mut tools = vec![];
    for t in chat["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|_| chat["tool_choice"] != "none")
    {
        if t["type"] != "function" {
            return Err("Command Code Go only supports function tools".into());
        }
        tools.push(json!({"name":t["function"]["name"],"description":t["function"]["description"].as_str().unwrap_or(""),"input_schema":t["function"]["parameters"]}));
    }
    let mut params = json!({"model":chat["model"],"messages":messages,"tools":tools,"system":system.join("\n\n"),"max_tokens":chat["max_completion_tokens"].as_u64().or(chat["max_tokens"].as_u64()).unwrap_or(64000),"stream":true});
    for k in ["temperature", "reasoning_effort"] {
        if !chat[k].is_null() {
            params[k] = chat[k].clone();
        }
    }
    Ok(
        json!({"config":{"workingDir":".","date":date,"environment":environment,"structure":[],"isGitRepo":false,"currentBranch":"","mainBranch":"","gitStatus":"","recentCommits":[]},"memory":null,"taste":null,"skills":null,"permissionMode":"standard","params":params}),
    )
}
pub struct GoStream {
    status: Option<u16>,
    lines: Lines,
    chat: ChatStream,
    cache_write: u64,
}
impl GoStream {
    pub fn new(model: &str, id: &str) -> Self {
        Self {
            status: None,
            lines: Lines::default(),
            chat: ChatStream::new(model, id),
            cache_write: 0,
        }
    }
    pub fn push(&mut self, data: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        let lines = self.lines.push(data)?;
        self.lines(lines)
    }
    fn lines(&mut self, lines: Vec<Vec<u8>>) -> Result<Vec<Vec<u8>>, String> {
        let mut out = vec![];
        for line in lines {
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let v: Value =
                serde_json::from_slice(&line).map_err(|_| "invalid Command Code stream JSON")?;
            if self.chat.done {
                return Err("Command Code sent events after completion".into());
            }
            match v["type"].as_str().unwrap_or("") {
                "text-delta" => out.extend(self.chat.delta(json!({"content":v["text"]}))),
                "reasoning-delta" => {
                    out.extend(self.chat.delta(json!({"reasoning_content":v["text"]})))
                }
                "tool-call" => {
                    if v["providerExecuted"] == true {
                        continue;
                    }
                    let raw = v
                        .get("input")
                        .filter(|v| !v.is_null())
                        .or_else(|| v.get("args").filter(|v| !v.is_null()))
                        .cloned()
                        .unwrap_or(json!({}));
                    let args = if let Some(s) = raw.as_str() {
                        let _: Value = serde_json::from_str(s)
                            .map_err(|_| "invalid Command Code tool arguments")?;
                        s.to_owned()
                    } else {
                        raw.to_string()
                    };
                    let mut check = ToolArguments::default();
                    check.push(&args)?;
                    check.finish()?;
                    let name = v["toolName"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .ok_or("Command Code tool name missing")?;
                    let id = v["toolCallId"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("call_{}", uuid::Uuid::new_v4().simple()));
                    let i = self.chat.tools;
                    self.chat.tools += 1;
                    out.extend(self.chat.delta(json!({"tool_calls":[{"index":i,"id":id,"type":"function","function":{"name":name,"arguments":args}}]})));
                }
                "cache-write-tokens" => {
                    self.cache_write = self
                        .cache_write
                        .max(v["cacheWriteTokens"].as_u64().unwrap_or(0))
                }
                "finish" => {
                    let t = &v["totalUsage"];
                    let input = t["inputTokens"].as_u64().unwrap_or(0);
                    let output = t["outputTokens"].as_u64().unwrap_or(0);
                    let total = input
                        .checked_add(output)
                        .ok_or("Command Code token usage overflow")?;
                    let usage = json!({"prompt_tokens":input,"completion_tokens":output,"total_tokens":total,"prompt_tokens_details":{"cached_tokens":t["inputTokenDetails"]["cacheReadTokens"].as_u64().unwrap_or(0),"cache_write_tokens":self.cache_write.max(t["inputTokenDetails"]["cacheWriteTokens"].as_u64().unwrap_or(0))},"completion_tokens_details":{"reasoning_tokens":t["outputTokenDetails"]["reasoningTokens"].as_u64().unwrap_or(0)}});
                    let stop = match v["finishReason"].as_str() {
                        Some("tool-calls") => "tool_calls",
                        Some("length") => "length",
                        Some("content-filter") => "content_filter",
                        _ => "stop",
                    };
                    out.extend(self.chat.stop(stop, Some(usage))?);
                }
                "abort" => return Err("Command Code aborted generation".into()),
                "error" => {
                    self.status = Some(super::failure_status(&v.to_string()));
                    return Err("Command Code reported a stream error".into());
                }
                _ => {}
            }
        }
        Ok(out)
    }
    pub fn finish(&mut self) -> Result<Vec<Vec<u8>>, String> {
        let lines = self.lines.finish();
        let out = self.lines(lines)?;
        self.chat.finish()?;
        Ok(out)
    }
}

impl super::ProviderStream for GoStream {
    fn has_output(&self) -> bool {
        self.chat.begun || self.chat.done
    }
    fn push(&mut self, b: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        GoStream::push(self, b)
    }
    fn finish(&mut self) -> Result<Vec<Vec<u8>>, String> {
        GoStream::finish(self)
    }
    fn failure_status(&self) -> Option<u16> {
        self.status
    }
}

pub fn fit_effort<'a>(want: &'a str, levels: &[&'a str]) -> &'a str {
    let want = if want == "ultra" && !levels.contains(&"ultra") {
        "max"
    } else {
        want
    };
    if levels.is_empty() || levels.contains(&want) {
        return want;
    }
    let ranks = [
        "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
    ];
    let Some(at) = ranks.iter().position(|e| *e == want) else {
        return want;
    };
    levels
        .iter()
        .filter(|e| **e != "none")
        .filter_map(|e| {
            ranks
                .iter()
                .position(|x| x == e)
                .map(|n| ((n.abs_diff(at), std::cmp::Reverse(n)), *e))
        })
        .min_by_key(|(rank, _)| *rank)
        .map(|(_, e)| e)
        .unwrap_or(want)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tool_identity_is_present_and_arguments_are_an_object() {
        let mut s = GoStream::new("m", "id");
        let data = s
            .push(b"{\"type\":\"tool-call\",\"toolName\":\"calc\",\"input\":{}}\n")
            .unwrap();
        let text = String::from_utf8(data.concat()).unwrap();
        assert!(text.contains("call_"));
        for input in ["[]", "\"bad\"", "{\"x\":"] {
            let mut s = GoStream::new("m", "id");
            let line = format!(
                "{}\n",
                json!({"type":"tool-call","toolName":"calc","input":input})
            );
            assert!(s.push(line.as_bytes()).is_err());
        }
    }
    #[test]
    fn events_after_finish_and_usage_overflow_fail() {
        let mut s = GoStream::new("m", "id");
        assert!(s
            .push(b"{\"type\":\"finish\"}\n{\"type\":\"text-delta\",\"text\":\"late\"}\n")
            .is_err());
        let mut s = GoStream::new("m", "id");
        let line = format!(
            "{}\n",
            json!({"type":"finish","totalUsage":{"inputTokens":u64::MAX,"outputTokens":1}})
        );
        assert!(s.push(line.as_bytes()).is_err());
    }
    #[test]
    fn go_tool_history_is_typed_and_orphans_are_rejected() {
        let c = json!({"messages":[{"role":"assistant","content":null,"tool_calls":[{"id":"a","function":{"name":"weather","arguments":"{\"city\":\"北京\"}"}}]},{"role":"tool","tool_call_id":"a","content":"sunny"}]});
        let body = generate_body(&c, "2026-10-03", "macos").unwrap();
        assert_eq!(
            body["params"]["messages"][1]["content"][0]["toolName"],
            "weather"
        );
        assert!(generate_body(
            &json!({"messages":[{"role":"tool","tool_call_id":"missing"}]}),
            "",
            ""
        )
        .is_err());
    }
    #[test]
    fn stream_requires_finish_and_keeps_cache_usage() {
        let mut s = GoStream::new("test", "id");
        s.push(b"{\"type\":\"text-delta\",\"text\":\"ok\"}\n")
            .unwrap();
        assert!(s.finish().is_err());
        let data=s.push(b"{\"type\":\"cache-write-tokens\",\"cacheWriteTokens\":9}\n{\"type\":\"finish\",\"totalUsage\":{\"inputTokens\":10,\"outputTokens\":3}}\n").unwrap().concat();
        assert!(String::from_utf8(data)
            .unwrap()
            .contains("\"cache_write_tokens\":9"));
        s.finish().unwrap();
    }
}
