//! In-process boundary to the trusted Agent's genuine CLI runtime.
use super::*;
pub type SubscriptionStream =
    Pin<Box<dyn Stream<Item = Result<Bytes, Box<dyn StdError + Send + Sync>>> + Send>>;
pub type SubscriptionFuture =
    Pin<Box<dyn std::future::Future<Output = Result<SubscriptionResponse, String>> + Send>>;
pub struct SubscriptionResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: SubscriptionStream,
}
pub trait SubscriptionBackend: Send + Sync {
    fn request_protocol(
        &self,
        target: ResolvedTarget,
        payload: serde_json::Value,
        _protocol: ProxyProtocol,
        session: Option<String>,
    ) -> SubscriptionFuture {
        self.request(target, payload, session)
    }
    fn models(&self, _target: ResolvedTarget) -> SubscriptionFuture {
        Box::pin(async { Err("model discovery is unavailable for this subscription".into()) })
    }
    fn request(
        &self,
        target: ResolvedTarget,
        payload: serde_json::Value,
        session: Option<String>,
    ) -> SubscriptionFuture;
    fn revoke(&self);
    fn retain_targets(&self, _targets: &[&ResolvedTarget]) -> Vec<Uuid> {
        self.revoke();
        Vec::new()
    }
    fn history_owner(&self, _payload: &serde_json::Value) -> Result<Option<Uuid>, String> {
        Ok(None)
    }
}
impl From<reqwest::Response> for SubscriptionResponse {
    fn from(response: reqwest::Response) -> Self {
        Self {
            status: response.status(),
            headers: response.headers().clone(),
            body: Box::pin(
                response
                    .bytes_stream()
                    .map(|r| r.map_err(|e| -> BoxError { Box::new(e) })),
            ),
        }
    }
}

/// Only protocol-owned call identifiers are inspected; schemas and user JSON
/// are never interpreted as routing state.
pub fn subscription_history_call_ids(payload: &serde_json::Value) -> Vec<&str> {
    let mut ids = Vec::new();
    for message in payload
        .get("messages")
        .or_else(|| payload.get("input"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
    {
        for field in ["call_id", "tool_call_id"] {
            if let Some(id) = message[field].as_str() {
                ids.push(id);
            }
        }
        for call in message["tool_calls"].as_array().into_iter().flatten() {
            if let Some(id) = call["id"].as_str() {
                ids.push(id);
            }
        }
        for block in message["content"].as_array().into_iter().flatten() {
            let field = match block["type"].as_str() {
                Some("tool_use") => "id",
                Some("tool_result") => "tool_use_id",
                _ => continue,
            };
            if let Some(id) = block[field].as_str() {
                ids.push(id);
            }
        }
    }
    ids
}

/// Normalizes a bundled provider's native wire to the proxy's shared Chat
/// Completions boundary, including signed Gemini tool continuations.
#[derive(Clone, Default)]
pub struct SubscriptionCodec {
    gemini: Arc<Mutex<gemini::SignatureLedger>>,
}
impl SubscriptionCodec {
    pub fn history_owner(&self, body: &serde_json::Value) -> Result<Option<Uuid>, String> {
        self.gemini
            .lock()
            .map_err(|_| "signature ledger unavailable")?
            .history_owner(body)
    }
    pub fn retain_targets(&self, ids: &HashSet<Uuid>) {
        if let Ok(mut ledger) = self.gemini.lock() {
            ledger.retain_targets(ids);
        }
    }
    pub fn prepare(
        &self,
        wire: &str,
        body: serde_json::Value,
        owner: Uuid,
    ) -> Result<serde_json::Value, String> {
        self.prepare_protocol(wire, ProxyProtocol::OpenAiChatCompletions, body, owner)
    }
    pub fn prepare_protocol(
        &self,
        wire: &str,
        source: ProxyProtocol,
        mut body: serde_json::Value,
        owner: Uuid,
    ) -> Result<serde_json::Value, String> {
        body["stream"] = serde_json::json!(true);
        if wire == "gemini" {
            body = BuiltinConversionPlugin
                .convert_request(source, ProxyProtocol::OpenAiChatCompletions, body)
                .map_err(|e| e.to_string())?;
            let (_, bytes) = gemini::prepare(
                "https://generativelanguage.googleapis.com",
                Bytes::from(body.to_string()),
                owner,
                &*self
                    .gemini
                    .lock()
                    .map_err(|_| "signature ledger unavailable")?,
            )?;
            return serde_json::from_slice(&bytes)
                .map_err(|_| "invalid native Gemini request".into());
        }
        BuiltinConversionPlugin
            .convert_request(source, community_protocol(wire)?, body)
            .map_err(|e| e.to_string())
    }
    pub async fn normalize(
        &self,
        response: SubscriptionResponse,
        wire: &str,
        owner: Uuid,
        streaming: bool,
    ) -> Result<SubscriptionResponse, String> {
        self.normalize_protocol(
            response,
            wire,
            owner,
            streaming,
            ProxyProtocol::OpenAiChatCompletions,
        )
        .await
    }
    pub async fn normalize_protocol(
        &self,
        mut response: SubscriptionResponse,
        wire: &str,
        owner: Uuid,
        streaming: bool,
        output: ProxyProtocol,
    ) -> Result<SubscriptionResponse, String> {
        if !response.status.is_success() {
            return Ok(response);
        }
        let chat = ProxyProtocol::OpenAiChatCompletions;
        let native = if wire == "gemini" {
            chat
        } else {
            community_protocol(wire)?
        };
        let source = if wire == "gemini" {
            gemini::stream(response.body, owner, self.gemini.clone())
        } else {
            response.body
        };
        response.headers.remove(header::CONTENT_LENGTH);
        response.headers.remove(header::CONTENT_ENCODING);
        if streaming {
            response.headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/event-stream"),
            );
            response.body = convert_sse_stream(source, native, output);
        } else {
            // Native AM/Responses content, signed thinking and provider fields
            // survive a same-protocol request without a lossy Chat round trip.
            let collect_protocol = if native == chat {
                ProxyProtocol::OpenAiResponses
            } else {
                native
            };
            let mut source = convert_sse_stream(source, native, collect_protocol);
            let mut bytes = Vec::new();
            while let Some(chunk) = tokio::time::timeout(Duration::from_secs(120), source.next())
                .await
                .map_err(|_| "community response idle timeout")?
            {
                let chunk = chunk.map_err(|_| "community generation stream failed")?;
                if bytes.len() + chunk.len() > 128 * 1024 * 1024 {
                    return Err("community generation exceeds bridge limit".into());
                }
                bytes.extend_from_slice(&chunk);
            }
            let value = if collect_protocol == ProxyProtocol::AnthropicMessages {
                collect_anthropic(&bytes)?
            } else {
                let complete = codex::collect_response(&bytes).map_err(|(_, e)| e.to_owned())?;
                serde_json::from_slice(&complete).map_err(|_| "invalid completed response")?
            };
            let value = BuiltinConversionPlugin
                .convert_response(collect_protocol, output, value)
                .map_err(|e| e.to_string())?;
            response.headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            );
            response.body = Box::pin(stream::once(
                async move { Ok(Bytes::from(value.to_string())) },
            ));
        }
        Ok(response)
    }
}
fn collect_anthropic(bytes: &[u8]) -> Result<serde_json::Value, String> {
    use serde_json::{json, Value};
    let mut message = Value::Null;
    let mut offset = 0;
    let mut complete = false;
    let mut arguments: HashMap<usize, String> = HashMap::new();
    while offset < bytes.len() {
        let end = sse_event_boundary_end(&bytes[offset..])
            .map(|n| offset + n)
            .unwrap_or(bytes.len());
        let data = sse_event_data(&bytes[offset..end]);
        offset = end;
        let Some(data) = data else {
            continue;
        };
        let event: Value = serde_json::from_slice(&data).map_err(|_| "invalid Messages event")?;
        match event["type"].as_str() {
            Some("message_start") => {
                message = event["message"].clone();
                message["content"] = json!([]);
            }
            Some("content_block_start") => {
                let i = event["index"]
                    .as_u64()
                    .filter(|i| *i <= 4096)
                    .ok_or("invalid Messages block")? as usize;
                let blocks = message["content"]
                    .as_array_mut()
                    .ok_or("Messages block before start")?;
                while blocks.len() <= i {
                    blocks.push(Value::Null);
                }
                blocks[i] = event["content_block"].clone();
            }
            Some("content_block_delta") => {
                let i = event["index"].as_u64().ok_or("invalid Messages block")? as usize;
                let block = message["content"]
                    .as_array_mut()
                    .and_then(|v| v.get_mut(i))
                    .ok_or("unknown Messages block")?;
                let delta = &event["delta"];
                match delta["type"].as_str() {
                    Some("input_json_delta") => arguments.entry(i).or_default().push_str(
                        delta["partial_json"]
                            .as_str()
                            .ok_or("invalid tool arguments")?,
                    ),
                    Some("text_delta" | "thinking_delta" | "signature_delta") => {
                        let field = match delta["type"].as_str() {
                            Some("text_delta") => "text",
                            Some("thinking_delta") => "thinking",
                            _ => "signature",
                        };
                        let mut text = block[field].as_str().unwrap_or("").to_owned();
                        text.push_str(delta[field].as_str().ok_or("invalid Messages delta")?);
                        block[field] = json!(text);
                    }
                    Some("citations_delta") => {
                        if !block["citations"].is_array() {
                            block["citations"] = json!([]);
                        }
                        block["citations"]
                            .as_array_mut()
                            .unwrap()
                            .push(delta["citation"].clone());
                    }
                    _ => {
                        return Err(
                            "unsupported Messages delta; use streaming for this response".into(),
                        )
                    }
                }
            }
            Some("message_delta") => {
                if let Some(delta) = event["delta"].as_object() {
                    for (k, v) in delta {
                        message[k] = v.clone();
                    }
                }
                if let Some(usage) = event["usage"].as_object() {
                    if !message["usage"].is_object() {
                        message["usage"] = json!({});
                    }
                    for (k, v) in usage {
                        message["usage"][k] = v.clone();
                    }
                }
            }
            Some("message_stop") => complete = true,
            Some("error") => return Err("Messages generation failed".into()),
            _ => {}
        }
    }
    if !complete || !message.is_object() {
        return Err("Messages stream ended before completion".into());
    }
    for (i, args) in arguments {
        message["content"][i]["input"] =
            serde_json::from_str(&args).map_err(|_| "invalid completed tool arguments")?;
    }
    Ok(message)
}
fn community_protocol(wire: &str) -> Result<ProxyProtocol, String> {
    match wire {
        "chat" => Ok(ProxyProtocol::OpenAiChatCompletions),
        "responses" => Ok(ProxyProtocol::OpenAiResponses),
        "anthropic" => Ok(ProxyProtocol::AnthropicMessages),
        _ => Err("unsupported community model protocol".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    fn response(text: String) -> SubscriptionResponse {
        SubscriptionResponse {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: Box::pin(stream::iter(
                text.into_bytes()
                    .chunks(7)
                    .map(|b| Ok(Bytes::copy_from_slice(b)))
                    .collect::<Vec<_>>(),
            )),
        }
    }
    async fn collect(mut response: SubscriptionResponse) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        while let Some(chunk) = response.body.next().await {
            out.extend_from_slice(&chunk.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }
    fn event(value: Value) -> String {
        format!("data: {value}\n\n")
    }
    #[tokio::test]
    async fn native_messages_keep_signed_thinking_tools_and_cache_usage() {
        let codec = SubscriptionCodec::default();
        let owner = Uuid::new_v4();
        let am = ProxyProtocol::AnthropicMessages;
        let request = json!({"model":"claude","messages":[{"role":"assistant","content":[{"type":"thinking","thinking":"private reasoning","signature":"opaque-signature"}]}],"thinking":{"type":"adaptive"},"context_management":{"edits":[]}});
        let prepared = codec
            .prepare_protocol("anthropic", am, request.clone(), owner)
            .unwrap();
        assert_eq!(prepared["messages"], request["messages"]);
        assert_eq!(
            prepared["context_management"],
            request["context_management"]
        );
        let events = [
            json!({"type":"message_start","message":{"id":"m","type":"message","role":"assistant","model":"claude","usage":{"input_tokens":9,"cache_read_input_tokens":6}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"想"}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"signature"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"tool-1","name":"read","input":{}}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"file\"}"}}),
            json!({"type":"content_block_stop","index":1}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":4}}),
            json!({"type":"message_stop"}),
        ];
        let wire = events.into_iter().map(event).collect::<String>();
        let streamed = codec
            .normalize_protocol(response(wire.clone()), "anthropic", owner, true, am)
            .await
            .unwrap();
        assert_eq!(collect(streamed).await.unwrap(), wire.as_bytes());
        let completed = codec
            .normalize_protocol(response(wire), "anthropic", owner, false, am)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&collect(completed).await.unwrap()).unwrap();
        assert_eq!(value["content"][0]["signature"], "signature");
        assert_eq!(value["content"][1]["input"]["path"], "file");
        assert_eq!(value["usage"]["cache_read_input_tokens"], 6);
    }
    #[tokio::test]
    async fn native_responses_preserve_custom_tools_and_reject_incomplete_eof() {
        let codec = SubscriptionCodec::default();
        let owner = Uuid::new_v4();
        let rs = ProxyProtocol::OpenAiResponses;
        let request = json!({"model":"grok","input":[{"type":"reasoning","encrypted_content":"opaque"}],"tools":[{"type":"custom","name":"patch","format":{"type":"text"}}]});
        assert_eq!(
            codec
                .prepare_protocol("responses", rs, request.clone(), owner)
                .unwrap()["tools"],
            request["tools"]
        );
        let value = json!({"id":"resp_1","object":"response","status":"completed","output":[{"type":"reasoning","id":"rs_1","encrypted_content":"opaque"},{"type":"custom_tool_call","call_id":"call_1","name":"patch","input":"patch text"}],"usage":{"input_tokens":4,"output_tokens":8}});
        let wire = event(json!({"type":"response.completed","response":value}));
        let output = codec
            .normalize_protocol(response(wire), "responses", owner, false, rs)
            .await
            .unwrap();
        let out: Value = serde_json::from_slice(&collect(output).await.unwrap()).unwrap();
        assert_eq!(out["output"], value["output"]);
        let incomplete = codec
            .normalize_protocol(
                response(event(
                    json!({"type":"response.output_text.delta","delta":"partial"}),
                )),
                "responses",
                owner,
                true,
                rs,
            )
            .await
            .unwrap();
        assert!(collect(incomplete).await.is_err());
    }
    #[tokio::test]
    async fn chat_completion_collects_late_usage_and_parallel_tools() {
        let codec = SubscriptionCodec::default();
        let wire=[json!({"id":"c","model":"m","choices":[{"index":0,"delta":{"content":"你好","tool_calls":[{"index":0,"id":"a","type":"function","function":{"name":"read","arguments":"{}"}},{"index":1,"id":"b","type":"function","function":{"name":"write","arguments":"{}"}}]}}]}),json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),json!({"choices":[],"usage":{"prompt_tokens":5,"completion_tokens":9}})].into_iter().map(event).collect::<String>()+"data: [DONE]\n\n";
        let response = codec
            .normalize(response(wire), "chat", Uuid::new_v4(), false)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&collect(response).await.unwrap()).unwrap();
        assert_eq!(
            value["choices"][0]["message"]["tool_calls"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(value["usage"]["completion_tokens"], 9);
    }
}
