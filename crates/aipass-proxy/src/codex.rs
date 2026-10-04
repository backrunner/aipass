//! ChatGPT's Codex subscription backend is Responses-only and stream-only.
//! These adaptations are selected by the agent's trusted credential metadata.

use super::*;
use serde_json::{json, Value};

// Minimum catalog contract implemented by this adapter. A newer genuine client
// can request its own catalog without downgrading to this compatibility floor.
const CLIENT_VERSION: &str = "0.159.0";

fn version_tuple(value: &str) -> Option<[u32; 3]> {
    let parts = value
        .split('.')
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    parts.try_into().ok()
}

pub(crate) fn client_version(headers: &HeaderMap) -> String {
    let requested = headers
        .get("version")
        .and_then(|v| v.to_str().ok())
        .or_else(|| {
            headers
                .get(header::USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .filter(|v| v.to_ascii_lowercase().starts_with("codex"))
                .and_then(|v| {
                    v.split_once('/')
                        .map(|(_, v)| v.split_whitespace().next().unwrap_or(""))
                })
        });
    requested
        .filter(|v| v.len() <= 24 && version_tuple(v) > version_tuple(CLIENT_VERSION))
        .unwrap_or(CLIENT_VERSION)
        .to_owned()
}

pub(crate) fn models_url(base: &str, headers: &HeaderMap) -> Result<String, String> {
    let mut url = reqwest::Url::parse(&format!("{}/models", base.trim_end_matches('/')))
        .map_err(|_| "invalid Codex model endpoint".to_string())?;
    url.query_pairs_mut()
        .append_pair("client_version", &client_version(headers));
    Ok(url.into())
}

pub(crate) fn normalize_models(payload: Bytes) -> Result<Bytes, String> {
    let mut root: Value =
        serde_json::from_slice(&payload).map_err(|_| "invalid Codex model catalog")?;
    let models = root
        .get("models")
        .and_then(Value::as_array)
        .ok_or("missing Codex models")?;
    let mut models = models
        .iter()
        .filter(|m| m["visibility"] != "hide" && m["slug"].as_str().is_some_and(|s| !s.is_empty()))
        .collect::<Vec<_>>();
    models.sort_by_key(|m| m["priority"].as_i64().unwrap_or(i64::MAX));
    let data = models.into_iter().map(|m| json!({
        "id":m["slug"], "object":"model", "owned_by":"openai", "display_name":m["display_name"],
        "context_window":m["context_window"], "max_context_window":m["max_context_window"],
        "supported_reasoning_levels":m["supported_reasoning_levels"],
        "capabilities":{"input_modalities":m["input_modalities"], "output_modalities":["text"], "supports_tool_use":true},
        "api_types":["responses"]
    })).collect::<Vec<_>>();
    if data.is_empty() {
        return Err("Codex returned no visible models".into());
    }
    root["data"] = json!(data);
    root["object"] = json!("list");
    serde_json::to_vec(&root)
        .map(Bytes::from)
        .map_err(|_| "could not encode Codex catalog".into())
}

pub(crate) fn prepare(payload: Bytes) -> Result<Bytes, String> {
    let mut payload: Value = serde_json::from_slice(&payload)
        .map_err(|_| "Codex subscription requires a JSON request".to_string())?;
    let object = payload
        .as_object_mut()
        .ok_or_else(|| "Codex subscription requires a JSON object".to_string())?;
    // The backend does not store responses. Never silently discard a parent
    // reference and submit an orphaned tool result as a new conversation.
    for field in ["previous_response_id", "conversation"] {
        if object
            .get(field)
            .is_some_and(|v| !v.is_null() && v.as_str() != Some(""))
        {
            return Err(format!(
                "Codex subscription requires full input history instead of {field}"
            ));
        }
        object.remove(field);
    }
    for field in [
        "max_output_tokens",
        "max_completion_tokens",
        "temperature",
        "top_p",
        "user",
        "safety_identifier",
    ] {
        object.remove(field);
    }
    if object.get("service_tier").and_then(Value::as_str) != Some("priority") {
        object.remove("service_tier");
    }
    object.insert("store".into(), json!(false));
    object.insert("stream".into(), json!(true));
    object.entry("instructions").or_insert_with(|| json!(""));
    if let Some(Value::String(text)) = object.get("input") {
        object.insert("input".into(), json!([{"type":"message", "role":"user", "content":[{"type":"input_text", "text":text}]}]));
    }
    if let Some(items) = object.get_mut("input").and_then(Value::as_array_mut) {
        for item in items {
            if item["type"] == "item_reference" {
                return Err("Codex subscription cannot resolve stored input references".into());
            }
            if let Some(item) = item.as_object_mut() {
                // Keep function call IDs; only the stored response item ID is
                // account-local and invalid on this stateless backend.
                item.remove("id");
                if item.get("role").and_then(Value::as_str) == Some("system") {
                    item.insert("role".into(), json!("developer"));
                }
            }
        }
    }
    serde_json::to_vec(&payload)
        .map(Bytes::from)
        .map_err(|_| "could not encode Codex request".into())
}

pub(crate) fn headers(headers: &mut HeaderMap, payload: &[u8]) {
    let version = client_version(headers);
    if let Ok(value) = HeaderValue::from_str(&version) {
        headers.insert("version", value);
    }
    headers.insert(
        header::ACCEPT,
        HeaderValue::from_static("text/event-stream"),
    );
    headers
        .entry("openai-beta")
        .or_insert(HeaderValue::from_static("responses=experimental"));
    headers
        .entry("originator")
        .or_insert(HeaderValue::from_static("codex_cli_rs"));
    if let Ok(payload) = serde_json::from_slice::<Value>(payload) {
        if let Some(key) = payload.get("prompt_cache_key").and_then(Value::as_str) {
            if let Ok(value) = HeaderValue::from_str(key) {
                headers.insert("session_id", value.clone());
                headers.insert("conversation_id", value);
            }
        }
    }
}

/// Preserve the terminal response's usage/status and its completed output
/// items, even when response.completed.output is empty (as Codex may send).
/// The bool says whether the upstream explicitly rejected generation; an EOF
/// or malformed frame cannot authorize replaying a possibly completed call.
pub(crate) fn collect_response(bytes: &[u8]) -> Result<Bytes, (bool, &'static str)> {
    let mut output = std::collections::BTreeMap::new();
    let mut terminal = None;
    let mut offset = 0;
    while offset < bytes.len() {
        let end = sse_event_boundary_end(&bytes[offset..]).map_or(bytes.len(), |end| offset + end);
        let event = &bytes[offset..end];
        offset = end;
        let Some(data) = sse_event_data(event) else {
            continue;
        };
        if trim_ascii(&data) == b"[DONE]" {
            continue;
        }
        let value: Value =
            serde_json::from_slice(&data).map_err(|_| (false, "invalid Codex stream event"))?;
        match value.get("type").and_then(Value::as_str) {
            Some("error" | "response.failed") => {
                return Err((true, "Codex upstream returned an error"))
            }
            Some("response.output_item.done") => {
                let index = value
                    .get("output_index")
                    .and_then(Value::as_u64)
                    .ok_or((false, "Codex output item is missing its index"))?;
                let item = value
                    .get("item")
                    .filter(|v| v.is_object())
                    .ok_or((false, "Codex output item is missing its payload"))?;
                output.insert(index, item.clone());
            }
            Some("response.completed" | "response.incomplete") => {
                let response = value
                    .get("response")
                    .filter(|v| v.is_object())
                    .ok_or((false, "Codex completion is missing its response"))?;
                terminal = Some(response.clone());
            }
            _ => {}
        }
    }
    let mut response = terminal.ok_or((false, "Codex stream ended before completion"))?;
    if response
        .get("output")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        response["output"] = json!(output.into_values().collect::<Vec<_>>());
    }
    serde_json::to_vec(&response)
        .map(Bytes::from)
        .map_err(|_| (false, "could not encode Codex response"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_catalog_preserves_capabilities_and_negotiates_version() {
        let bytes = Bytes::from(json!({"models":[
            {"slug":"hidden","visibility":"hide"},
            {"slug":"second","priority":2,"input_modalities":["text"]},
            {"slug":"first","priority":1,"context_window":200000,"supported_reasoning_levels":[{"effort":"high"}],"input_modalities":["text","image"]}
        ]}).to_string());
        let catalog: Value = serde_json::from_slice(&normalize_models(bytes).unwrap()).unwrap();
        assert_eq!(catalog["data"].as_array().unwrap().len(), 2);
        assert_eq!(catalog["data"][0]["id"], "first");
        assert_eq!(catalog["data"][0]["context_window"], 200000);
        let mut headers = HeaderMap::new();
        headers.insert("version", HeaderValue::from_static("0.160.1"));
        assert!(
            models_url("https://chatgpt.com/backend-api/codex", &headers)
                .unwrap()
                .ends_with("/models?client_version=0.160.1")
        );
        headers.insert("version", HeaderValue::from_static("not-a-version"));
        assert_eq!(client_version(&headers), CLIENT_VERSION);
    }

    #[test]
    fn preparation_keeps_user_intent_and_tool_identity() {
        let body = prepare(Bytes::from(json!({"model":"x", "stream":false, "store":true, "temperature":0.3, "max_output_tokens":10, "instructions":"Keep this", "tool_choice":{"type":"function","name":"lookup"}, "parallel_tool_calls":false,
            "input":[{"id":"msg_old","role":"system","content":"policy"},{"id":"fc_old","type":"function_call","call_id":"call_1","name":"lookup","arguments":"{}"},{"type":"function_call_output","call_id":"call_1","output":"answer"}]}).to_string())).unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["stream"], true);
        assert_eq!(body["store"], false);
        assert_eq!(body["instructions"], "Keep this");
        assert_eq!(body["input"][0]["role"], "developer");
        assert_eq!(body["input"][1]["call_id"], "call_1");
        assert!(body["input"][1].get("id").is_none());
        assert!(body.get("temperature").is_none());
        assert_eq!(body["tool_choice"]["name"], "lookup");
        assert_eq!(body["parallel_tool_calls"], false);
        assert!(prepare(Bytes::from_static(
            br#"{"input":[],"previous_response_id":"resp_old"}"#
        ))
        .is_err());
    }

    #[test]
    fn collection_requires_completion_and_keeps_tools_and_usage() {
        let events = [
            json!({"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","call_id":"call_1","arguments":"{}"}}),
            json!({"type":"response.incomplete","response":{"id":"resp_1","status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"output":[],"usage":{"input_tokens":10,"output_tokens":2}}}),
        ];
        let bytes = events
            .iter()
            .map(|v| format!("data: {v}\n\n"))
            .collect::<String>();
        let value: Value =
            serde_json::from_slice(&collect_response(bytes.as_bytes()).unwrap()).unwrap();
        assert_eq!(value["output"][0]["call_id"], "call_1");
        assert_eq!(value["usage"]["input_tokens"], 10);
        assert_eq!(value["status"], "incomplete");
        assert!(!collect_response(b"data: [DONE]\n\n").unwrap_err().0);
        assert!(
            collect_response(b"data: {\"type\":\"response.failed\"}\n\n")
                .unwrap_err()
                .0
        );
    }
}
