//! Native Gemini GenerateContent transport behind the local OpenAI/Anthropic APIs.
use super::*;
use serde_json::{json, Value};

const CALL_PREFIX: &str = "call_aipass_gemini_";
type NativeTurn = Arc<Mutex<Vec<Value>>>;

#[derive(Default)]
pub(crate) struct SignatureLedger {
    calls: HashMap<String, (Uuid, Instant, Value, NativeTurn)>,
}
impl SignatureLedger {
    fn remember(&mut self, owner: Uuid, part: &Value, history: &NativeTurn) -> String {
        self.calls
            .retain(|_, (_, at, _, _)| at.elapsed() < Duration::from_secs(3600));
        while self.calls.len() >= 128 {
            let oldest = self
                .calls
                .iter()
                .min_by_key(|(_, (_, at, _, _))| *at)
                .map(|(k, _)| k.clone());
            if let Some(key) = oldest {
                self.calls.remove(&key);
            }
        }
        let id = format!("{CALL_PREFIX}{}", Uuid::new_v4().simple());
        self.calls.insert(
            id.clone(),
            (owner, Instant::now(), part.clone(), history.clone()),
        );
        id
    }
    fn restore(&self, owner: Uuid, id: &str) -> Result<Option<(Value, NativeTurn)>, String> {
        if !id.starts_with(CALL_PREFIX) {
            return Ok(None);
        }
        let (target, at, part, history) = self
            .calls
            .get(id)
            .ok_or("Gemini tool history expired; start a new conversation")?;
        if *target != owner || at.elapsed() >= Duration::from_secs(3600) {
            return Err(
                "Gemini thought signatures belong to another account or expired session".into(),
            );
        }
        Ok(Some((part.clone(), history.clone())))
    }
    pub(crate) fn history_owner(&self, payload: &Value) -> Result<Option<Uuid>, String> {
        let mut owner = None;
        for id in subscription_history_call_ids(payload)
            .into_iter()
            .filter(|id| id.starts_with(CALL_PREFIX))
        {
            let (target, at, _, _) = self
                .calls
                .get(id)
                .ok_or("Gemini tool history expired; restart the conversation")?;
            if at.elapsed() >= Duration::from_secs(3600) {
                return Err("Gemini tool history expired".into());
            }
            if owner.is_some_and(|o| o != *target) {
                return Err("Gemini history mixes accounts".into());
            }
            owner = Some(*target);
        }
        Ok(owner)
    }
    pub(crate) fn retain_targets(&mut self, ids: &HashSet<Uuid>) {
        self.calls.retain(|_, (id, _, _, _)| ids.contains(id));
    }
}

fn parts(content: &Value) -> Result<Vec<Value>, String> {
    if let Some(text) = content.as_str() {
        return Ok(if text.is_empty() {
            Vec::new()
        } else {
            vec![json!({"text":text})]
        });
    }
    if content.is_null() {
        return Ok(Vec::new());
    }
    content.as_array().ok_or("invalid Gemini message content")?.iter().map(|p| {
        match p["type"].as_str() {
            Some("text") => Ok(json!({"text":p["text"]})),
            Some("image_url") => {
                let url = p.pointer("/image_url/url").and_then(Value::as_str).ok_or("image URL missing")?;
                if let Some((mime,data)) = url.strip_prefix("data:").and_then(|v| v.split_once(";base64,")) {
                    Ok(json!({"inlineData":{"mimeType":mime,"data":data}}))
                } else { Err("Gemini native image inputs require inline base64 data; remote URLs are not fetched by the proxy".into()) }
            }
            _ => Err("unsupported Gemini content part".into()),
        }
    }).collect()
}

pub(crate) fn prepare(
    base: &str,
    body: Bytes,
    owner: Uuid,
    ledger: &SignatureLedger,
) -> Result<(String, Bytes), String> {
    let body: Value = serde_json::from_slice(&body).map_err(|_| "Gemini requires JSON")?;
    let model = body["model"]
        .as_str()
        .ok_or("Gemini model is required")?
        .trim_start_matches("models/");
    if model.is_empty()
        || !model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
    {
        return Err("invalid Gemini model identifier".into());
    }
    if body["n"].as_u64().is_some_and(|n| n != 1) {
        return Err("Gemini adapter requires n=1".into());
    }
    let stream = body["stream"] == true;
    let base = base
        .trim_end_matches('/')
        .trim_end_matches("/v1beta")
        .trim_end_matches("/v1");
    let url = format!(
        "{base}/v1beta/models/{model}:{}{}",
        if stream {
            "streamGenerateContent"
        } else {
            "generateContent"
        },
        if stream { "?alt=sse" } else { "" }
    );
    let mut contents: Vec<Value> = Vec::new();
    let mut system = Vec::new();
    let mut names = HashMap::new();
    let mut native_ids: HashMap<String, Value> = HashMap::new();
    for message in body["messages"]
        .as_array()
        .ok_or("Gemini messages are required")?
    {
        let role = message["role"].as_str().unwrap_or("user");
        let mut blocks = if role == "tool" {
            Vec::new()
        } else {
            parts(&message["content"])?
        };
        if matches!(role, "system" | "developer") {
            system.extend(blocks);
            continue;
        }
        if role == "tool" {
            let id = message["tool_call_id"]
                .as_str()
                .ok_or("tool result is missing call ID")?;
            let name = names
                .get(id)
                .ok_or("Gemini tool result has no matching function call")?;
            let response = match &message["content"] {
                Value::String(text) => serde_json::from_str::<Value>(text)
                    .ok()
                    .filter(Value::is_object)
                    .unwrap_or_else(|| json!({"output":text})),
                value if value.is_array() => {
                    json!({"output":"Tool returned content attached below"})
                }
                _ => return Err("unsupported Gemini tool result".into()),
            };
            let mut result = json!({"functionResponse":{"name":name,"response":response}});
            if let Some(native_id) = native_ids.get(id) {
                result["functionResponse"]["id"] = native_id.clone();
            }
            blocks.push(result);
            if message["content"].is_array() {
                blocks.extend(parts(&message["content"])?);
            }
        }
        if let Some(calls) = message["tool_calls"].as_array() {
            let mut signed_history: Option<NativeTurn> = None;
            let mut originals = Vec::new();
            for call in calls {
                let id = call["id"].as_str().ok_or("tool call is missing ID")?;
                let name = call
                    .pointer("/function/name")
                    .and_then(Value::as_str)
                    .ok_or("Gemini supports function tools")?;
                names.insert(id.to_owned(), name.to_owned());
                if let Some((original, history)) = ledger.restore(owner, id)? {
                    if signed_history
                        .as_ref()
                        .is_some_and(|previous| !Arc::ptr_eq(previous, &history))
                    {
                        return Err("Gemini tool history mixes native turns".into());
                    }
                    signed_history = Some(history);
                    let args = match &call["function"]["arguments"] {
                        Value::String(v) => serde_json::from_str::<Value>(v)
                            .map_err(|_| "invalid function arguments")?,
                        v => v.clone(),
                    };
                    if original["functionCall"]["name"] != name
                        || original["functionCall"].get("args").unwrap_or(&json!({})) != &args
                    {
                        return Err("signed Gemini tool history was modified".into());
                    }
                    if let Some(native_id) = original["functionCall"].get("id") {
                        native_ids.insert(id.to_owned(), native_id.clone());
                    }
                    originals.push(original);
                } else {
                    let args = match &call["function"]["arguments"] {
                        Value::String(v) => {
                            serde_json::from_str(v).map_err(|_| "invalid function arguments")?
                        }
                        v if v.is_object() => v.clone(),
                        _ => json!({}),
                    };
                    let part = json!({"functionCall":{"name":name,"args":args}});
                    if call
                        .pointer("/extra_content/google/thought_signature")
                        .is_some()
                    {
                        return Err(
                            "unowned Gemini thought signature; replay the proxy-issued call ID"
                                .into(),
                        );
                    }
                    blocks.push(part);
                }
            }
            if let Some(history) = signed_history {
                let history = history.lock().map_err(|_| "Gemini session unavailable")?;
                let expected: Vec<_> = history
                    .iter()
                    .filter(|part| part.get("functionCall").is_some())
                    .cloned()
                    .collect();
                if originals.len() != calls.len() || originals != expected {
                    return Err("signed Gemini tool calls were removed, reordered or mixed with foreign calls".into());
                }
                let original_text: String = history
                    .iter()
                    .filter(|part| part["thought"] != true)
                    .filter_map(|part| part["text"].as_str())
                    .collect();
                let replay_text: String = blocks
                    .iter()
                    .filter_map(|part| part["text"].as_str())
                    .collect();
                if replay_text != original_text {
                    return Err("signed Gemini assistant text was modified".into());
                }
                blocks = history.clone();
            }
        }
        let role = if role == "assistant" { "model" } else { "user" };
        if !blocks.is_empty() {
            if let Some(previous) = contents.last_mut().filter(|c| c["role"] == role) {
                previous["parts"].as_array_mut().unwrap().extend(blocks);
            } else {
                contents.push(json!({"role":role,"parts":blocks}));
            }
        }
    }
    let mut out = json!({"contents":contents});
    if !system.is_empty() {
        out["systemInstruction"] = json!({"parts":system});
    }
    let mut generation = serde_json::Map::new();
    for (from, to) in [
        ("temperature", "temperature"),
        ("top_p", "topP"),
        ("max_tokens", "maxOutputTokens"),
        ("max_completion_tokens", "maxOutputTokens"),
        ("seed", "seed"),
    ] {
        if let Some(value) = body.get(from) {
            generation.insert(to.into(), value.clone());
        }
    }
    if let Some(stop) = body.get("stop") {
        generation.insert(
            "stopSequences".into(),
            if stop.is_string() {
                json!([stop])
            } else {
                stop.clone()
            },
        );
    }
    if let Some(effort) = body.get("reasoning_effort").and_then(Value::as_str) {
        let config = if model.starts_with("gemini-2.5") {
            json!({"thinkingBudget":match effort {"none"=>0,"low"=>1024,"medium"=>8192,_=>24576},"includeThoughts":true})
        } else {
            json!({"thinkingLevel":if effort == "none" {"minimal"} else {effort},"includeThoughts":true})
        };
        generation.insert("thinkingConfig".into(), config);
    }
    if let Some(config) = body.pointer("/extra_body/google/thinking_config") {
        if generation.contains_key("thinkingConfig") {
            return Err("conflicting Gemini thinking controls".into());
        }
        generation.insert("thinkingConfig".into(), config.clone());
    }
    if let Some("json_object" | "json_schema") = body
        .pointer("/response_format/type")
        .and_then(Value::as_str)
    {
        generation.insert("responseMimeType".into(), json!("application/json"));
        if let Some(schema) = body.pointer("/response_format/json_schema/schema") {
            generation.insert("responseJsonSchema".into(), schema.clone());
        }
    }
    out["generationConfig"] = json!(generation);
    if let Some(tools) = body["tools"].as_array() {
        let declarations = tools.iter().map(|t| {
            if t["type"] != "function" { return Err("Gemini supports function tools on this route"); }
            Ok(json!({"name":t["function"]["name"],"description":t["function"]["description"],"parametersJsonSchema":t["function"]["parameters"]}))
        }).collect::<Result<Vec<_>,_>>()?;
        out["tools"] = json!([{"functionDeclarations":declarations}]);
    }
    if let Some(choice) = body.get("tool_choice") {
        out["toolConfig"] = json!({"functionCallingConfig":match choice.as_str() {
            Some("none")=>json!({"mode":"NONE"}),Some("required")=>json!({"mode":"ANY"}),Some("auto")=>json!({"mode":"AUTO"}),
            _ if choice.pointer("/function/name").is_some()=>json!({"mode":"ANY","allowedFunctionNames":[choice["function"]["name"]]}),
            _ => return Err("unsupported Gemini tool choice".into()),
        }});
    }
    Ok((
        url,
        Bytes::from(serde_json::to_vec(&out).map_err(|_| "could not encode Gemini request")?),
    ))
}

pub(crate) fn response(
    value: Value,
    owner: Uuid,
    ledger: &mut SignatureLedger,
    streaming: bool,
    next_tool: &mut u64,
) -> Result<Value, String> {
    response_with_history(
        value,
        owner,
        ledger,
        streaming,
        next_tool,
        &NativeTurn::default(),
    )
}

fn response_with_history(
    value: Value,
    owner: Uuid,
    ledger: &mut SignatureLedger,
    streaming: bool,
    next_tool: &mut u64,
    history: &NativeTurn,
) -> Result<Value, String> {
    let value = value.get("response").cloned().unwrap_or(value);
    if let Some(parts) = value
        .pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)
    {
        let mut saved = history.lock().map_err(|_| "Gemini session unavailable")?;
        saved.extend(parts.iter().cloned());
        if serde_json::to_vec(&*saved)
            .map_err(|_| "invalid Gemini history")?
            .len()
            > 512 * 1024
        {
            return Err("Gemini native turn exceeds signature history limit".into());
        }
    }
    if let Some(error) = value.get("error") {
        return Ok(json!({"error":error}));
    }
    if value.pointer("/promptFeedback/blockReason").is_some() {
        return Err("Gemini blocked the prompt".into());
    }
    let candidate = value.pointer("/candidates/0");
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut calls = Vec::new();
    for part in candidate
        .and_then(|c| c.pointer("/content/parts"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(value) = part["text"].as_str() {
            if part["thought"] == true {
                reasoning.push_str(value);
            } else {
                text.push_str(value);
            }
        } else if let Some(call) = part.get("functionCall") {
            if part.to_string().len() > 64 * 1024 {
                return Err("Gemini function call exceeds bridge limit".into());
            }
            let id = ledger.remember(owner, part, history);
            let mut call = json!({"id":id,"type":"function","function":{"name":call["name"],"arguments":call.get("args").unwrap_or(&json!({})).to_string()}});
            if streaming {
                call["index"] = json!(*next_tool);
                *next_tool += 1;
            }
            calls.push(call);
        } else if part.get("inlineData").is_some() {
            return Err("Gemini generated non-text content on a text route".into());
        }
    }
    let finish = candidate.and_then(|c| c["finishReason"].as_str());
    let finish = match finish {
        Some("STOP") => json!(if calls.is_empty() && *next_tool == 0 {
            "stop"
        } else {
            "tool_calls"
        }),
        Some("MAX_TOKENS") => json!("length"),
        Some("SAFETY" | "RECITATION" | "BLOCKLIST" | "PROHIBITED_CONTENT") => {
            json!("content_filter")
        }
        None => Value::Null,
        Some(_) => return Err("Gemini generation did not complete successfully".into()),
    };
    let mut message = json!({"role":"assistant"});
    if !text.is_empty() || !streaming {
        message["content"] = json!(text);
    }
    if !reasoning.is_empty() {
        message["reasoning_content"] = json!(reasoning);
    }
    if !calls.is_empty() {
        message["tool_calls"] = json!(calls);
    }
    let mut choice = json!({"index":0,"finish_reason":finish});
    choice[if streaming { "delta" } else { "message" }] = message;
    let mut out = json!({"id":value.get("responseId").cloned().unwrap_or(json!("gemini-response")),"object":if streaming {"chat.completion.chunk"}else{"chat.completion"},"model":value["modelVersion"],"choices":[choice]});
    if let Some(usage) = value.get("usageMetadata") {
        let input = usage["promptTokenCount"].as_u64().unwrap_or(0);
        let output = usage["candidatesTokenCount"].as_u64().unwrap_or(0)
            + usage["thoughtsTokenCount"].as_u64().unwrap_or(0);
        out["usage"] = json!({"prompt_tokens":input,"completion_tokens":output,"total_tokens":input+output,"prompt_tokens_details":{"cached_tokens":usage["cachedContentTokenCount"].as_u64().unwrap_or(0)},"completion_tokens_details":{"reasoning_tokens":usage["thoughtsTokenCount"].as_u64().unwrap_or(0)}});
    }
    if !streaming && finish.is_null() {
        return Err("Gemini response is missing finishReason".into());
    }
    Ok(out)
}

pub(crate) fn stream(
    source: UpstreamBodyStream,
    owner: Uuid,
    ledger: Arc<Mutex<SignatureLedger>>,
) -> UpstreamBodyStream {
    Box::pin(stream::unfold(
        (source, Vec::<u8>::new(), 0u64, false, NativeTurn::default()),
        move |(mut source, mut buffer, mut next_tool, mut terminal, history)| {
            let ledger = ledger.clone();
            async move {
                loop {
                    if let Some(end) = sse_event_boundary_end(&buffer) {
                        let event = buffer.drain(..end).collect::<Vec<_>>();
                        let Some(data) = sse_event_data(&event) else {
                            continue;
                        };
                        let normalized = serde_json::from_slice(&data)
                            .map_err(|_| "invalid Gemini stream JSON".to_string())
                            .and_then(|value| {
                                response_with_history(
                                    value,
                                    owner,
                                    &mut *ledger
                                        .lock()
                                        .map_err(|_| "Gemini session unavailable")?,
                                    true,
                                    &mut next_tool,
                                    &history,
                                )
                            });
                        let value = match normalized {
                            Ok(value) => value,
                            Err(error) => {
                                return Some((
                                    Err(std::io::Error::other(error).into()),
                                    (
                                        Box::pin(stream::empty()) as UpstreamBodyStream,
                                        Vec::new(),
                                        next_tool,
                                        true,
                                        history,
                                    ),
                                ))
                            }
                        };
                        if value
                            .pointer("/choices/0/finish_reason")
                            .is_some_and(|v| !v.is_null())
                        {
                            terminal = true;
                        }
                        return Some((
                            Ok(Bytes::from(format!("data: {value}\n\n"))),
                            (source, buffer, next_tool, terminal, history),
                        ));
                    }
                    match source.next().await {
                        Some(Ok(bytes)) => {
                            buffer.extend_from_slice(&bytes);
                            if buffer.len() > 4 * 1024 * 1024 {
                                return Some((
                                    Err(std::io::Error::other("Gemini event exceeds bridge limit")
                                        .into()),
                                    (
                                        Box::pin(stream::empty()) as UpstreamBodyStream,
                                        Vec::new(),
                                        next_tool,
                                        true,
                                        history,
                                    ),
                                ));
                            }
                        }
                        Some(Err(error)) => {
                            return Some((
                                Err(error),
                                (source, buffer, next_tool, terminal, history),
                            ))
                        }
                        None if terminal => return None,
                        None => {
                            return Some((
                                Err(
                                    std::io::Error::other("Gemini stream ended before completion")
                                        .into(),
                                ),
                                (
                                    Box::pin(stream::empty()) as UpstreamBodyStream,
                                    Vec::new(),
                                    next_tool,
                                    true,
                                    history,
                                ),
                            ))
                        }
                    }
                }
            }
        },
    ))
}

pub(crate) fn models_url(base: &str, query: Option<&str>) -> Result<String, String> {
    let mut url = reqwest::Url::parse(&format!(
        "{}/v1beta/models",
        base.trim_end_matches('/')
            .trim_end_matches("/v1beta")
            .trim_end_matches("/v1")
    ))
    .map_err(|_| "invalid Gemini model endpoint")?;
    if let Some(query) = query {
        for (key, value) in reqwest::Url::parse(&format!("http://localhost/?{query}"))
            .map_err(|_| "invalid models query")?
            .query_pairs()
        {
            if matches!(key.as_ref(), "pageToken" | "pageSize") {
                url.query_pairs_mut().append_pair(&key, &value);
            }
        }
    }
    Ok(url.to_string())
}

pub(crate) fn models(payload: Bytes) -> Result<Bytes, String> {
    let root: Value =
        serde_json::from_slice(&payload).map_err(|_| "invalid Gemini model catalog")?;
    let models = root["models"].as_array().ok_or("missing Gemini models")?;
    let data = models.iter().filter(|m| m["supportedGenerationMethods"].as_array().is_some_and(|v| v.iter().any(|v| v == "generateContent"))).filter_map(|m| {
        let name = m["name"].as_str()?.trim_start_matches("models/");
        Some(json!({"id":name,"object":"model","owned_by":"google","display_name":m["displayName"],"context_window":m["inputTokenLimit"],"max_output_tokens":m["outputTokenLimit"]}))
    }).collect::<Vec<_>>();
    serde_json::to_vec(&json!({"object":"list","data":data,"nextPageToken":root["nextPageToken"]}))
        .map(Bytes::from)
        .map_err(|_| "could not encode Gemini catalog".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn code_assist_envelope_retains_tool_signatures_and_usage() {
        let mut ledger = SignatureLedger::default();
        let native = json!({"response":{"responseId":"r","modelVersion":"gemini-3","candidates":[{"content":{"parts":[{"functionCall":{"name":"lookup","args":{"city":"Paris"}},"thoughtSignature":"signed-state"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":30,"candidatesTokenCount":5}},"traceId":"private-trace"});
        let chat = response(native, Uuid::new_v4(), &mut ledger, false, &mut 0).unwrap();
        assert_eq!(chat["usage"]["prompt_tokens"], 30);
        assert_eq!(
            chat["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
            "lookup"
        );
        assert!(!chat.to_string().contains("private-trace"));
    }
    #[test]
    fn tool_roundtrip_restores_exact_signature_only_for_its_owner() {
        let mut ledger = SignatureLedger::default();
        let owner = Uuid::new_v4();
        let native = json!({"responseId":"r","modelVersion":"gemini-3","candidates":[{"content":{"parts":[{"functionCall":{"name":"lookup","args":{"city":"Paris"}},"thoughtSignature":"signed-state"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":30,"candidatesTokenCount":5,"thoughtsTokenCount":10,"cachedContentTokenCount":20}});
        let chat = response(native, owner, &mut ledger, false, &mut 0).unwrap();
        assert_eq!(chat["usage"]["completion_tokens"], 15);
        let id = chat["choices"][0]["message"]["tool_calls"][0]["id"]
            .as_str()
            .unwrap();
        let body = Bytes::from(json!({"model":"gemini-3","messages":[chat["choices"][0]["message"],{"role":"tool","tool_call_id":id,"content":"Sunny"}]}).to_string());
        let (url, prepared) = prepare(
            "https://generativelanguage.googleapis.com",
            body.clone(),
            owner,
            &ledger,
        )
        .unwrap();
        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-3:generateContent"
        );
        let prepared: Value = serde_json::from_slice(&prepared).unwrap();
        assert_eq!(
            prepared["contents"][0]["parts"][0]["thoughtSignature"],
            "signed-state"
        );
        assert_eq!(
            prepared["contents"][1]["parts"][0]["functionResponse"]["name"],
            "lookup"
        );
        assert!(prepare("https://example.test", body, Uuid::new_v4(), &ledger).is_err());
    }
    #[tokio::test]
    async fn streaming_replays_signed_text_and_parallel_tools_as_one_native_turn() {
        let owner = Uuid::new_v4();
        let ledger = Arc::new(Mutex::new(SignatureLedger::default()));
        let native_parts = vec![
            json!({"text":"considering", "thought":true,"thoughtSignature":"text-signature"}),
            json!({"text":"Checking"}),
            json!({"functionCall":{"name":"first","args":{"n":1},"id":"native-1"},"thoughtSignature":"call-signature"}),
            json!({"functionCall":{"name":"second","args":{},"id":"native-2"}}),
        ];
        let chunks: Vec<_> = native_parts.iter().enumerate().map(|(i,p)| Ok(Bytes::from(format!("data: {}\n\n", json!({"candidates":[{"content":{"parts":[p]},"finishReason":if i==3 {json!("STOP")} else {Value::Null}}]}))))).collect();
        let mut output = stream(Box::pin(stream::iter(chunks)), owner, ledger.clone());
        let mut calls = Vec::new();
        while let Some(chunk) = output.next().await {
            let chunk = chunk.unwrap();
            let value: Value = serde_json::from_slice(&sse_event_data(&chunk).unwrap()).unwrap();
            if let Some(c) = value
                .pointer("/choices/0/delta/tool_calls")
                .and_then(Value::as_array)
            {
                calls.extend(c.iter().cloned());
            }
        }
        let mut request = json!({"model":"gemini-3","messages":[{"role":"assistant","content":"Checking","tool_calls":calls},{"role":"tool","tool_call_id":calls[0]["id"],"content":"ok"},{"role":"tool","tool_call_id":calls[1]["id"],"content":"ok"}]});
        let (_, body) = prepare(
            "https://example.test",
            Bytes::from(request.to_string()),
            owner,
            &ledger.lock().unwrap(),
        )
        .unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["contents"][0]["parts"], json!(native_parts));
        assert_eq!(
            body["contents"][1]["parts"][0]["functionResponse"]["id"],
            "native-1"
        );
        request["messages"][0]["tool_calls"]
            .as_array_mut()
            .unwrap()
            .remove(1);
        assert!(prepare(
            "https://example.test",
            Bytes::from(request.to_string()),
            owner,
            &ledger.lock().unwrap()
        )
        .is_err());
    }
    #[tokio::test]
    async fn incomplete_native_stream_is_transport_failure_never_replayable_rejection() {
        let chunks: UpstreamBodyStream = Box::pin(stream::iter(vec![Ok(Bytes::from_static(
            b"data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"partial\"}]}}]}\n\n",
        ))]));
        let mut output = stream(
            chunks,
            Uuid::new_v4(),
            Arc::new(Mutex::new(SignatureLedger::default())),
        );
        assert!(output
            .next()
            .await
            .unwrap()
            .unwrap()
            .windows(7)
            .any(|w| w == b"partial"));
        assert!(output.next().await.unwrap().is_err());
        assert!(output.next().await.is_none());
    }
}
