//! Cursor's bidirectional Connect protocol. This module never executes a tool:
//! only caller-declared MCP calls are returned to the caller.
use super::connect::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const NAMESPACE: &str = "aipass";
const CALL: &str = "CallDynamicTool";
const LEVELS: [&str; 8] = [
    "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
];

pub fn split_model(id: &str) -> (String, String) {
    let (mut base, fast) = id
        .strip_suffix("-fast")
        .map(|s| (s, true))
        .unwrap_or((id, false));
    let mut thinking = false;
    if let Some(s) = base.strip_suffix("-thinking") {
        base = s;
        thinking = true;
    }
    let mut effort = "";
    for level in [
        "extra-high",
        "xhigh",
        "minimal",
        "none",
        "low",
        "medium",
        "high",
        "max",
    ] {
        if let Some(s) = base
            .strip_suffix(&format!("-{level}"))
            .filter(|s| !s.is_empty())
        {
            base = s;
            effort = if level == "extra-high" {
                "xhigh"
            } else {
                level
            };
            break;
        }
    }
    if let Some(s) = base.strip_suffix("-thinking") {
        base = s;
        thinking = true;
    }
    (
        format!(
            "{base}{}{}",
            if thinking { "-thinking" } else { "" },
            if fast { "-fast" } else { "" }
        ),
        effort.into(),
    )
}
pub fn variants(raw: &[Value], family: &str) -> BTreeMap<String, String> {
    let rows: Vec<_> = raw
        .iter()
        .filter(|v| split_model(v["id"].as_str().unwrap_or("")).0 == family)
        .collect();
    let mut out = BTreeMap::new();
    let mut default = None;
    for row in &rows {
        let id = row["id"].as_str().unwrap_or("");
        let effort = split_model(id).1;
        out.entry(effort.clone()).or_insert_with(|| id.to_owned());
        let word = match effort.as_str() {
            "xhigh" => "extra high",
            x => x,
        };
        if effort.is_empty()
            || (default.is_none()
                && !row["name"]
                    .as_str()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(word))
        {
            default = Some(id.to_owned());
        }
    }
    if !rows.is_empty() {
        out.insert(
            "".into(),
            default
                .or_else(|| out.get("medium").cloned())
                .unwrap_or_else(|| rows[0]["id"].as_str().unwrap_or("").into()),
        );
    }
    out
}
pub fn model_id(raw: &[Value], model: &str, effort: &str, fast: bool) -> String {
    let (family, named_effort) = split_model(model);
    let raw_variant = !named_effort.is_empty() && raw.iter().any(|v| v["id"] == model);
    let mut family = if raw_variant { family } else { model.into() };
    if fast && !family.ends_with("-fast") && !variants(raw, &format!("{family}-fast")).is_empty() {
        family.push_str("-fast");
    }
    let vs = variants(raw, &family);
    let want = if effort.is_empty() {
        named_effort.as_str()
    } else {
        effort
    };
    let id = if let Some(id) = vs.get(want) {
        id.as_str()
    } else if raw_variant {
        model
    } else {
        let mut choice = vs.get("").map(String::as_str).unwrap_or(model);
        if let Some(at) = LEVELS.iter().position(|l| *l == want) {
            let levels: Vec<_> = LEVELS
                .iter()
                .enumerate()
                .filter(|(_, l)| vs.contains_key(**l))
                .collect();
            let unnamed = levels.iter().all(|(_, l)| vs.get(**l) != vs.get(""));
            let in_between = levels
                .first()
                .zip(levels.last())
                .is_some_and(|((low, _), (high, _))| at > *low && at < *high);
            if !(unnamed && in_between) {
                if let Some((_, l)) = levels
                    .into_iter()
                    .filter(|(_, l)| **l != "none")
                    .min_by_key(|(i, _)| (i.abs_diff(at), std::cmp::Reverse(*i)))
                {
                    choice = vs.get(*l).unwrap();
                }
            }
        }
        choice
    };
    if id == "auto" {
        return "default".into();
    }
    raw.iter()
        .find(|v| v["id"] == id)
        .and_then(|v| v["run"].as_str())
        .unwrap_or(id)
        .into()
}
fn pb_value(v: &Value, depth: usize) -> Result<Vec<u8>, String> {
    if depth > 64 {
        return Err("Cursor tool value nesting exceeds limit".into());
    }
    Ok(match v {
        Value::Null => uint(1, 0),
        Value::Bool(b) => uint(4, u64::from(*b)),
        Value::Number(n) => {
            if n.as_u64().is_some_and(|n| n > 9_007_199_254_740_991)
                || n.as_i64().is_some_and(|n| n < -9_007_199_254_740_991)
            {
                return Err("Cursor protobuf cannot preserve this integer exactly".into());
            }
            [
                varint(2 << 3 | 1),
                n.as_f64()
                    .ok_or("invalid Cursor number")?
                    .to_le_bytes()
                    .to_vec(),
            ]
            .concat()
        }
        Value::String(s) => string(3, s),
        Value::Array(a) => {
            let mut b = Vec::new();
            for v in a {
                b.extend(bytes(1, &pb_value(v, depth + 1)?));
            }
            bytes(6, &b)
        }
        Value::Object(o) => {
            let mut b = Vec::new();
            let mut keys: Vec<_> = o.keys().collect();
            keys.sort();
            for k in keys {
                b.extend(bytes(
                    1,
                    &[string(1, k), bytes(2, &pb_value(&o[k], depth + 1)?)].concat(),
                ));
            }
            bytes(5, &b)
        }
    })
}
fn pb_any(b: &[u8], depth: usize) -> Result<Value, String> {
    if depth > 64 {
        return Err("Cursor tool value nesting exceeds limit".into());
    }
    for (n, f) in fields(b)? {
        match (n, f) {
            (1, _) => return Ok(Value::Null),
            (2, Field::Fixed64(n)) => {
                return serde_json::Number::from_f64(f64::from_bits(n))
                    .map(Value::Number)
                    .ok_or_else(|| "invalid Cursor number".into())
            }
            (3, Field::Bytes(b)) => {
                return Ok(json!(
                    std::str::from_utf8(b).map_err(|_| "invalid Cursor UTF-8")?
                ))
            }
            (4, Field::Int(n)) => return Ok(json!(n != 0)),
            (5, Field::Bytes(b)) => {
                let mut v = json!({});
                for (n, f) in fields(b)? {
                    if let (1, Field::Bytes(b)) = (n, f) {
                        let kv = fields(b)?;
                        v[field_text(&kv, 1)] =
                            pb_any(field_bytes(&kv, 2).unwrap_or_default(), depth + 1)?;
                    }
                }
                return Ok(v);
            }
            (6, Field::Bytes(b)) => {
                let mut v = vec![];
                for (n, f) in fields(b)? {
                    if let (1, Field::Bytes(b)) = (n, f) {
                        v.push(pb_any(b, depth + 1)?);
                    }
                }
                return Ok(json!(v));
            }
            _ => {}
        }
    }
    Ok(Value::Null)
}
fn tool_def(t: &Value) -> Result<Vec<u8>, String> {
    let f = &t["function"];
    let name = f["name"].as_str().ok_or("Cursor tool name missing")?;
    let schema = f
        .get("parameters")
        .cloned()
        .unwrap_or(json!({"type":"object","properties":{}}));
    Ok([
        string(1, name),
        string(2, f["description"].as_str().unwrap_or("")),
        bytes(3, &pb_value(&schema, 0)?),
        string(4, NAMESPACE),
        string(5, name),
        string(6, &schema.to_string()),
    ]
    .concat())
}
fn environment() -> Vec<u8> {
    [
        string(
            1,
            if cfg!(target_os = "macos") {
                "darwin"
            } else {
                std::env::consts::OS
            },
        ),
        string(2, "/tmp"),
        string(10, "UTC"),
    ]
    .concat()
}
fn native_call_id(id: &str) -> String {
    if id.starts_with("call_") {
        id.replacen("__fc_", "\nfc_", 1)
    } else {
        id.into()
    }
}
fn result(id: &str, text: &str, error: bool) -> Value {
    json!({"type":"tool-result","toolCallId":native_call_id(id),"toolName":CALL,"result":serde_json::from_str::<Value>(text).unwrap_or(json!(text)),"experimental_content":[{"type":"text","text":text}],"isError":error})
}
fn flush(out: &mut Vec<Value>, pending: &mut BTreeSet<String>, results: &mut Vec<Value>) {
    for id in std::mem::take(pending) {
        results.push(result(
            &id,
            "Tool use was interrupted and did not produce a result.",
            true,
        ));
    }
    if !results.is_empty() {
        out.push(json!({"role":"tool","content":std::mem::take(results)}));
    }
}

pub struct Run {
    pub first: Vec<u8>,
    blobs: BTreeMap<Vec<u8>, Vec<u8>>,
    tools: Vec<Value>,
}
pub fn request(chat: &Value, id: &str, session: &str) -> Result<Run, String> {
    if chat["tool_choice"] == "required" {
        return Err("Cursor cannot enforce required tool choice".into());
    }
    let mut tools = vec![];
    if chat["tool_choice"] != "none" {
        for t in chat["tools"].as_array().into_iter().flatten() {
            if t["type"] != "function" {
                return Err("Cursor only supports function tools".into());
            }
            if chat["tool_choice"].is_object()
                && t["function"]["name"] != chat["tool_choice"]["function"]["name"]
            {
                continue;
            }
            tool_def(t)?;
            tools.push(t.clone());
        }
    }
    let messages = chat["messages"]
        .as_array()
        .ok_or("Cursor messages must be an array")?;
    let mut system = messages
        .iter()
        .filter(|m| m["role"] == "system" || m["role"] == "developer")
        .map(|m| text(&m["content"]))
        .collect::<Vec<_>>()
        .join("\n\n");
    if !tools.is_empty() {
        system.push_str(&format!("\n\n<dynamic_tool_catalog>\nThe tools below are available in the MCP namespace \"{NAMESPACE}\". Call one with `{CALL}` (namespace \"{NAMESPACE}\", toolName, arguments). Their schemas are given here, so there is no need to call `GetDynamicTools` first.\n"));
        for t in &tools {
            let f = &t["function"];
            system.push_str(&format!(
                "<tool name={} >\n{}\ninput schema: {}\n</tool>\n",
                f["name"],
                f["description"].as_str().unwrap_or(""),
                f["parameters"]
            ));
        }
        system.push_str("</dynamic_tool_catalog>");
    }
    let mut out = vec![];
    if !system.is_empty() {
        out.push(json!({"role":"system","content":system}));
    }
    let mut pending = BTreeSet::new();
    let mut results = vec![];
    let mut last = ".".to_owned();
    for m in messages {
        match m["role"].as_str().unwrap_or("") {
            "system" | "developer" => {}
            "tool" => {
                let id = m["tool_call_id"]
                    .as_str()
                    .ok_or("Cursor tool result id missing")?;
                if !pending.remove(id) {
                    return Err("Cursor tool result has no unique matching call".into());
                }
                results.push(result(id, &text(&m["content"]), false));
            }
            "assistant" => {
                flush(&mut out, &mut pending, &mut results);
                let mut content = vec![];
                let t = text(&m["content"]);
                if !t.is_empty() {
                    content.push(json!({"type":"text","text":t}));
                }
                for call in m["tool_calls"].as_array().into_iter().flatten() {
                    if call["type"] != "function" {
                        return Err("Cursor only supports function tool history".into());
                    }
                    let cid = call["id"].as_str().ok_or("Cursor tool id missing")?;
                    if !pending.insert(cid.to_owned()) {
                        return Err("duplicate Cursor tool call id".into());
                    }
                    let args: Value = serde_json::from_str(
                        call["function"]["arguments"].as_str().unwrap_or("{}"),
                    )
                    .map_err(|_| "invalid Cursor tool arguments")?;
                    content.push(json!({"type":"tool-call","toolCallId":native_call_id(cid),"toolName":CALL,"args":{"namespace":NAMESPACE,"toolName":call["function"]["name"],"arguments":args}}));
                }
                if !content.is_empty() {
                    out.push(json!({"role":"assistant","content":content}));
                }
            }
            "user" => {
                flush(&mut out, &mut pending, &mut results);
                let parts = m["content"]
                    .as_array()
                    .cloned()
                    .unwrap_or_else(|| vec![json!({"type":"text","text":m["content"]})]);
                let mut content = vec![];
                let mut first = true;
                for p in parts {
                    match p["type"].as_str().unwrap_or("") {
                        "text" => {
                            let t = p["text"].as_str().ok_or("invalid Cursor text")?;
                            if first && !t.is_empty() {
                                last = t.into();
                                first = false;
                            }
                            content.push(p);
                        }
                        "image_url" => {
                            let u = p["image_url"]
                                .as_str()
                                .or(p["image_url"]["url"].as_str())
                                .ok_or("Cursor image URL missing")?;
                            let (mime, b) = u
                                .strip_prefix("data:")
                                .and_then(|u| u.split_once(";base64,"))
                                .ok_or("Cursor requires inline base64 images")?;
                            let raw = STANDARD
                                .decode(b)
                                .map_err(|_| "invalid Cursor image data")?;
                            if raw.len() > MAX_FRAME {
                                return Err("Cursor image exceeds limit".into());
                            }
                            let hex = raw.iter().map(|b| format!("{b:02x}")).collect::<String>();
                            content.push(json!({"type":"image","mimeType":mime,"image":{"__type":"Uint8Array","hex":hex}}));
                        }
                        _ => return Err("Cursor cannot preserve this attachment type".into()),
                    }
                }
                out.push(json!({"role":"user","content":content}));
            }
            _ => return Err("unsupported Cursor message role".into()),
        }
    }
    flush(&mut out, &mut pending, &mut results);
    let mut blobs = BTreeMap::new();
    let mut total = 0usize;
    let mut put = |b: Vec<u8>| -> Result<Vec<u8>, String> {
        total += b.len();
        if b.len() > MAX_FRAME || total > 64 * 1024 * 1024 {
            return Err("Cursor conversation exceeds limit".into());
        }
        let id = Sha256::digest(&b).to_vec();
        blobs.insert(id.clone(), b);
        Ok(id)
    };
    let mut state = vec![];
    for m in out {
        state.extend(bytes(
            1,
            &put(serde_json::to_vec(&m).map_err(|_| "invalid Cursor message")?)?,
        ));
    }
    let mid = uuid::Uuid::new_v4().to_string();
    let user = [string(1, &last), string(2, &mid), uint(4, 1)].concat();
    let turn = bytes(1, &[bytes(1, &put(user)?), string(10, &mid)].concat());
    state.extend(bytes(8, &put(turn)?));
    state.extend(uint(10, 1));
    state.extend(string(22, "cli"));
    let mut rc = bytes(4, &environment());
    let mut mcp = vec![];
    for t in &tools {
        let d = tool_def(t)?;
        rc.extend(bytes(7, &d));
        mcp.extend(bytes(1, &d));
    }
    let action = bytes(2, &bytes(2, &rc));
    let key = chat["prompt_cache_key"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(session);
    let conv = if key.is_empty() {
        uuid::Uuid::new_v4().to_string()
    } else {
        let first = messages
            .iter()
            .find(|m| m["role"] == "user")
            .or(messages.first())
            .map(Value::to_string)
            .unwrap_or_default();
        let h = Sha256::digest(format!("cursor conversation\0{key}\0{first}"));
        let mut b: [u8; 16] = h[..16].try_into().unwrap();
        b[6] = (b[6] & 15) | 64;
        b[8] = (b[8] & 63) | 128;
        uuid::Uuid::from_bytes(b).to_string()
    };
    let rr = [
        bytes(1, &state),
        bytes(2, &action),
        bytes(3, &[string(1, id), string(3, id), string(4, id)].concat()),
        bytes(4, &mcp),
        string(5, &conv),
        bytes(9, &string(1, id)),
        uint(19, 1),
    ]
    .concat();
    Ok(Run {
        first: frame(0, &bytes(1, &rr)),
        blobs,
        tools,
    })
}
pub fn failure(status: u16, v: &Value) -> (u16, String) {
    let e = v.get("error").unwrap_or(v);
    let code = e["code"].as_str().unwrap_or("");
    let mut msg = e["message"]
        .as_str()
        .unwrap_or("Cursor request failed")
        .to_owned();
    for d in e["details"].as_array().into_iter().flatten() {
        if let Some(detail) = d["debug"]["details"]["detail"]
            .as_str()
            .filter(|s| !s.is_empty())
        {
            msg = detail.into();
        }
    }
    let low = msg.to_lowercase();
    let status = if low.contains("region") || code == "permission_denied" {
        403
    } else if code == "unauthenticated" || status == 401 {
        401
    } else if code == "resource_exhausted"
        || ["quota", "rate limit", "usage limit"]
            .iter()
            .any(|s| low.contains(s))
    {
        429
    } else if code == "invalid_argument"
        || ["too long", "context length", "too many tokens"]
            .iter()
            .any(|s| low.contains(s))
    {
        400
    } else if code == "unavailable" {
        503
    } else if !code.is_empty() {
        502
    } else {
        status
    };
    (status, msg)
}
#[derive(Default)]
pub struct Batch {
    pub outgoing: Vec<Vec<u8>>,
    pub chunks: Vec<Vec<u8>>,
    pub error: Option<(u16, String)>,
}
pub struct CursorStream {
    frames: Frames,
    chat: ChatStream,
    blobs: BTreeMap<Vec<u8>, Vec<u8>>,
    tools: Vec<Value>,
    listed: usize,
    ended: bool,
    usage: Option<Value>,
}
impl CursorStream {
    pub fn new(model: &str, id: &str, run: Run) -> Self {
        Self {
            frames: Frames::default(),
            chat: ChatStream::new(model, id),
            blobs: run.blobs,
            tools: run.tools,
            listed: 0,
            ended: false,
            usage: None,
        }
    }
    pub fn done(&self) -> bool {
        self.ended
    }
    pub fn has_output(&self) -> bool {
        self.chat.begun || self.chat.done
    }
    pub fn heartbeat() -> Vec<u8> {
        frame(0, &bytes(7, &[]))
    }
    fn stop(&mut self, batch: &mut Batch) -> Result<(), String> {
        self.ended = true;
        batch
            .chunks
            .extend(self.chat.stop("stop", self.usage.take())?);
        Ok(())
    }
    pub fn push(&mut self, data: &[u8]) -> Result<Batch, String> {
        let mut batch = Batch::default();
        for (flags, data) in self.frames.push(data)? {
            if self.ended {
                break;
            }
            let data = uncompress(flags, &data)?;
            if flags & 2 != 0 {
                let v: Value =
                    serde_json::from_slice(&data).map_err(|_| "invalid Cursor Connect end")?;
                if !v["error"].is_null() {
                    batch.error = Some(failure(200, &v));
                    self.ended = true;
                    break;
                }
                if !self.chat.begun {
                    return Err("Cursor returned an empty reply".into());
                }
                self.stop(&mut batch)?;
                break;
            }
            for (n, f) in fields(&data)? {
                let Field::Bytes(b) = f else { continue };
                match n {
                    1 => {
                        for (n, f) in fields(b)? {
                            let Field::Bytes(b) = f else { continue };
                            let f = fields(b)?;
                            match n {
                                1 | 4 => {
                                    let t = field_text(&f, 1);
                                    if !t.is_empty() {
                                        batch.chunks.extend(self.chat.delta(if n == 1 {
                                            json!({"content":t})
                                        } else {
                                            json!({"reasoning_content":t})
                                        }));
                                    }
                                }
                                14 => {
                                    let prompt = field_int(&f, 1);
                                    let output = field_int(&f, 2);
                                    let total = prompt
                                        .checked_add(output)
                                        .ok_or("Cursor token usage overflow")?;
                                    self.usage = Some(
                                        json!({"prompt_tokens":prompt,"completion_tokens":output,"total_tokens":total,"prompt_tokens_details":{"cached_tokens":field_int(&f,3),"cache_write_tokens":field_int(&f,4)},"completion_tokens_details":{"reasoning_tokens":field_int(&f,5)}}),
                                    );
                                    if self.chat.tools == 0 {
                                        self.stop(&mut batch)?;
                                    }
                                }
                                27 => {
                                    self.listed = field_int(&f, 1)
                                        .try_into()
                                        .map_err(|_| "invalid Cursor tool count")?;
                                    if self.listed > 1024 {
                                        return Err("Cursor tool count exceeds limit".into());
                                    }
                                }
                                _ => {}
                            }
                            if self.ended {
                                break;
                            }
                        }
                    }
                    2 => self.exec(b, &mut batch)?,
                    4 => {
                        let f = fields(b)?;
                        let id = field_int(&f, 1);
                        for (n, v) in &f {
                            if let Field::Bytes(b) = v {
                                let reply = match n {
                                    2 => {
                                        let k = fields(b)?;
                                        let key = field_bytes(&k, 1).unwrap_or_default();
                                        let res = if let Some(b) = self.blobs.get(key) {
                                            bytes(1, b)
                                        } else {
                                            bytes(2, &string(1, "blob not found"))
                                        };
                                        Some(bytes(2, &res))
                                    }
                                    3 => Some(bytes(3, &[])),
                                    _ => None,
                                };
                                if let Some(res) = reply {
                                    batch
                                        .outgoing
                                        .push(frame(0, &bytes(3, &[uint(1, id), res].concat())));
                                }
                            }
                        }
                    }
                    _ => {}
                }
                if self.ended {
                    break;
                }
            }
            if !self.ended && self.listed > 0 && self.chat.tools >= self.listed {
                self.stop(&mut batch)?;
            }
        }
        Ok(batch)
    }
    fn exec(&mut self, b: &[u8], batch: &mut Batch) -> Result<(), String> {
        let f = fields(b)?;
        let id = field_int(&f, 1);
        let exec = field_text(&f, 15);
        let mut answer = None;
        for (n, v) in &f {
            let Field::Bytes(b) = v else { continue };
            match n {
                11 => {
                    let a = fields(b)?;
                    let full = field_text(&a, 1);
                    let name = field_text(&a, 5);
                    let name = if name.is_empty() {
                        full.strip_prefix("aipass-").unwrap_or(&full).to_owned()
                    } else {
                        name
                    };
                    if !self.tools.iter().any(|t| t["function"]["name"] == name) {
                        break;
                    }
                    let mut args = json!({});
                    for (n, v) in &a {
                        if let (2, Field::Bytes(b)) = (n, v) {
                            let kv = fields(b)?;
                            args[field_text(&kv, 1)] =
                                pb_any(field_bytes(&kv, 2).unwrap_or_default(), 0)?;
                        }
                    }
                    let cid = field_text(&a, 3).replace('\n', "__");
                    let cid = if cid.is_empty() {
                        format!("call_{}", uuid::Uuid::new_v4().simple())
                    } else {
                        cid
                    };
                    let index = self.chat.tools;
                    self.chat.tools += 1;
                    batch.chunks.extend(self.chat.delta(json!({"tool_calls":[{"index":index,"id":cid,"type":"function","function":{"name":name,"arguments":args.to_string()}}]})));
                    return Ok(());
                }
                36 => {
                    let mut srv = [
                        string(1, NAMESPACE),
                        string(2, NAMESPACE),
                        string(7, "connected"),
                    ]
                    .concat();
                    for t in &self.tools {
                        srv.extend(bytes(5, &tool_def(t)?));
                    }
                    answer = Some(bytes(36, &bytes(1, &bytes(1, &srv))));
                    break;
                }
                10 => {
                    answer = Some(bytes(10, &bytes(1, &bytes(1, &bytes(4, &environment())))));
                    break;
                }
                _ => {}
            }
        }
        let response = if let Some(a) = answer {
            bytes(2, &[uint(1, id), string(15, &exec), a].concat())
        } else {
            bytes(
                5,
                &bytes(2, &[uint(1, id), string(2, "not available")].concat()),
            )
        };
        batch.outgoing.push(frame(0, &response));
        batch
            .outgoing
            .push(frame(0, &bytes(5, &bytes(1, &uint(1, id)))));
        Ok(())
    }
    pub fn finish(&self) -> Result<(), String> {
        self.frames.finish()?;
        if self.ended {
            self.chat.finish()
        } else {
            Err("Cursor reply ended before the turn or declared tool calls completed".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn content_and_connect_failure_in_one_chunk_is_a_submitted_generation() {
        let mut s = CursorStream::new("m", "id", request(&chat(), "m", "s").unwrap());
        let body = [
            frame(0, &bytes(1, &bytes(1, &string(1, "content")))),
            frame(2, br#"{"error":{"code":"resource_exhausted"}}"#),
        ]
        .concat();
        let batch = s.push(&body).unwrap();
        assert!(batch.error.is_some());
        assert!(!batch.chunks.is_empty());
        assert!(s.has_output());
    }
    fn chat() -> Value {
        json!({"messages":[{"role":"user","content":"hello"}],"tools":[{"type":"function","function":{"name":"search","parameters":{"type":"object"}}}]})
    }
    #[test]
    fn model_family_keeps_fast_and_thinking_and_uses_nearest_effort() {
        let raw = vec![
            json!({"id":"grok-low","name":"Grok Low","run":"L"}),
            json!({"id":"grok-high","name":"Grok High","run":"H"}),
            json!({"id":"grok-high-fast","name":"Grok High Fast","run":"HF"}),
        ];
        assert_eq!(
            split_model("claude-thinking-extra-high-fast"),
            ("claude-thinking-fast".into(), "xhigh".into())
        );
        assert_eq!(model_id(&raw, "grok", "medium", false), "H");
        assert_eq!(model_id(&raw, "grok", "high", true), "HF");
    }
    #[test]
    fn protobuf_values_reject_lossy_large_integers() {
        let v = json!({"flag":true,"items":[null,"text",4.5]});
        assert_eq!(pb_any(&pb_value(&v, 0).unwrap(), 0).unwrap(), v);
        assert!(pb_value(&json!(9007199254740993u64), 0).is_err());
    }
    #[test]
    fn caller_tools_are_returned_and_foreign_execution_is_refused() {
        let run = request(&chat(), "model", "session").unwrap();
        let mut s = CursorStream::new("model", "id", run);
        let unknown = bytes(2, &[uint(1, 1), bytes(11, &string(5, "shell"))].concat());
        let out = s.push(&frame(0, &unknown)).unwrap();
        assert_eq!(out.outgoing.len(), 2);
        assert!(out.chunks.is_empty());
        let args = bytes(
            2,
            &[
                string(1, "q"),
                bytes(2, &pb_value(&json!("needle"), 0).unwrap()),
            ]
            .concat(),
        );
        let call = bytes(
            2,
            &[
                uint(1, 2),
                bytes(
                    11,
                    &[string(5, "search"), string(3, "call_a\nfc_b"), args].concat(),
                ),
            ]
            .concat(),
        );
        let out = s.push(&frame(0, &call)).unwrap();
        let text = String::from_utf8(out.chunks.concat()).unwrap();
        assert!(text.contains("call_a__fc_b"));
        assert!(text.contains("needle"));
        assert!(s.finish().is_err());
        let count = bytes(1, &bytes(27, &uint(1, 1)));
        s.push(&frame(0, &count)).unwrap();
        s.finish().unwrap();
    }
    #[test]
    fn completion_preserves_cache_and_reasoning_usage() {
        let mut s = CursorStream::new("m", "id", request(&chat(), "m", "s").unwrap());
        let payload = bytes(
            1,
            &[
                bytes(1, &string(1, "hi")),
                bytes(
                    14,
                    &[
                        uint(1, 100),
                        uint(2, 7),
                        uint(3, 80),
                        uint(4, 5),
                        uint(5, 3),
                    ]
                    .concat(),
                ),
            ]
            .concat(),
        );
        let mut out = vec![];
        for b in frame(0, &payload) {
            out.extend(s.push(&[b]).unwrap().chunks);
        }
        s.finish().unwrap();
        let text = String::from_utf8(out.concat()).unwrap();
        assert!(text.contains("\"prompt_tokens\":100"));
        assert!(text.contains("\"cache_write_tokens\":5"));
        assert!(text.contains("\"reasoning_tokens\":3"));
    }
    #[test]
    fn conversation_is_session_bound_and_repairs_missing_tool_results() {
        let mut c = chat();
        c["messages"] = json!([{"role":"assistant","tool_calls":[{"id":"call_a","type":"function","function":{"name":"search","arguments":"{}"}}]},{"role":"user","content":"continue"}]);
        let run = request(&c, "m", "s").unwrap();
        assert!(run
            .blobs
            .values()
            .any(|b| String::from_utf8_lossy(b).contains("interrupted")));
        c["messages"] = json!([{"role":"tool","tool_call_id":"orphan","content":"x"}]);
        assert!(request(&c, "m", "s").is_err());
    }
}
