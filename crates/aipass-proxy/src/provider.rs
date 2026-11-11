//! Provider contracts selected from vault metadata, never guessed from URLs.
use super::*;
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProviderProfile {
    #[default]
    Generic,
    Anthropic,
    OpenAi,
    DeepSeek,
    Kimi,
    Mistral,
    GeminiCompatible,
    Grok,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PreparationError {
    #[error("{0}")]
    Invalid(&'static str),
    #[error("{0}")]
    Incompatible(&'static str),
}

impl From<&'static str> for PreparationError {
    fn from(message: &'static str) -> Self {
        Self::Invalid(message)
    }
}

pub(crate) fn prepare(
    profile: ProviderProfile,
    protocol: ProxyProtocol,
    bytes: Bytes,
) -> Result<Bytes, PreparationError> {
    if protocol == ProxyProtocol::AnthropicMessages {
        let Ok(mut body) = serde_json::from_slice::<Value>(&bytes) else {
            return Ok(bytes);
        };
        return if aipass_proxy_conversion::normalize_claude_thinking(
            &mut body,
            profile == ProviderProfile::Anthropic,
        ) {
            Ok(Bytes::from(body.to_string()))
        } else {
            Ok(bytes)
        };
    }
    if profile == ProviderProfile::Generic || protocol != ProxyProtocol::OpenAiChatCompletions {
        return Ok(bytes);
    }
    let mut body: Value = serde_json::from_slice(&bytes).map_err(|_| "provider requires JSON")?;
    let obj = body
        .as_object_mut()
        .ok_or("provider requires a JSON object")?;
    let (from, to) = if profile == ProviderProfile::OpenAi {
        ("max_tokens", "max_completion_tokens")
    } else {
        ("max_completion_tokens", "max_tokens")
    };
    if let Some(value) = obj.remove(from) {
        obj.entry(to).or_insert(value);
    }
    match profile {
        ProviderProfile::Mistral => mistral_reasoning(obj)?,
        ProviderProfile::DeepSeek | ProviderProfile::Kimi => {
            let model = obj
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let kimi3 = profile == ProviderProfile::Kimi && model.starts_with("kimi-k3");
            let always_thinking = profile == ProviderProfile::Kimi
                && (kimi3 || model.starts_with("kimi-k2.7-code") || model.contains("thinking"));
            if kimi3 && obj.contains_key("thinking") {
                return Err("Kimi K3 uses reasoning_effort and does not accept thinking".into());
            }
            if let Some(effort) = obj
                .get("reasoning_effort")
                .and_then(Value::as_str)
                .map(str::to_owned)
            {
                if kimi3 {
                    if !matches!(effort.as_str(), "low" | "high" | "max") {
                        return Err("Kimi K3 reasoning_effort must be low, high or max".into());
                    }
                } else {
                    if always_thinking && effort == "none" {
                        return Err("this Kimi model cannot disable thinking".into());
                    }
                    obj.entry("thinking").or_insert_with(
                        || json!({"type":if effort == "none" { "disabled" } else { "enabled" }}),
                    );
                    if effort == "none" || profile == ProviderProfile::Kimi {
                        obj.remove("reasoning_effort");
                    }
                }
            }
            if always_thinking && obj.get("thinking").is_some_and(|v| v["type"] == "disabled") {
                return Err("this Kimi model cannot disable thinking".into());
            }
            let thinking = obj.get("thinking").is_some_and(|v| v["type"] == "enabled")
                || obj
                    .get("model")
                    .and_then(Value::as_str)
                    .is_some_and(|m| m.contains("reasoner") || m.contains("thinking"));
            if thinking
                || always_thinking
                || (profile == ProviderProfile::Kimi && model.starts_with("kimi-k2.6"))
            {
                for key in [
                    "temperature",
                    "top_p",
                    "presence_penalty",
                    "frequency_penalty",
                ] {
                    obj.remove(key);
                }
            }
            if let Some(messages) = obj.get_mut("messages").and_then(Value::as_array_mut) {
                for message in messages {
                    if message["role"] == "developer" {
                        message["role"] = json!("system");
                    }
                    // Preserve exact prior reasoning for tool continuations. An
                    // absent value stays absent: inventing it would hide lost history.
                }
            }
        }
        ProviderProfile::GeminiCompatible
            if obj.get("reasoning_effort").is_some()
                && obj
                    .get("extra_body")
                    .and_then(|v| v.pointer("/google/thinking_config"))
                    .is_some() =>
        {
            return Err(
                "Gemini accepts either reasoning_effort or google.thinking_config, not both".into(),
            );
        }
        _ => {}
    }
    serde_json::to_vec(&body)
        .map(Bytes::from)
        .map_err(|_| "could not encode provider request".into())
}

// Mistral replays assistant reasoning in typed content, including when a
// Responses/Messages history was first converted to Chat. Never infer this
// contract from a model name or an arbitrary gateway URL.
fn mistral_reasoning(body: &mut serde_json::Map<String, Value>) -> Result<(), PreparationError> {
    let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    for message in messages {
        if message["role"] != "assistant" {
            continue;
        }
        let Some(message) = message.as_object_mut() else {
            continue;
        };
        for key in ["reasoning_content", "reasoning"] {
            if message
                .get(key)
                .is_some_and(|v| !v.is_null() && !v.is_string())
            {
                return Err("Mistral assistant reasoning must be text".into());
            }
        }
        let reasoning = message.get("reasoning").and_then(Value::as_str);
        let content = message.get("reasoning_content").and_then(Value::as_str);
        if reasoning.zip(content).is_some_and(|(a, b)| a != b) {
            return Err(PreparationError::Incompatible(
                "Mistral assistant reasoning aliases must agree",
            ));
        }
        let thinking = content.or(reasoning).unwrap_or("").to_owned();
        message.remove("reasoning");
        message.remove("reasoning_content");
        if thinking.trim().is_empty() {
            continue;
        }
        let mut parts = match message.remove("content") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::String(text)) if text.is_empty() => Vec::new(),
            Some(Value::String(text)) => vec![json!({"type":"text","text":text})],
            Some(Value::Array(parts)) => parts,
            _ => return Err("Mistral assistant content must be text or content parts".into()),
        };
        if parts.iter().any(|part| part["type"] == "thinking") {
            return Err(PreparationError::Incompatible(
                "Mistral assistant reasoning cannot use both fields and content parts",
            ));
        }
        parts.insert(
            0,
            json!({"type":"thinking","thinking":[{"type":"text","text":thinking}]}),
        );
        message.insert("content".into(), json!(parts));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mistral_reasoning_keeps_tools_and_rejects_conflicting_representations() {
        let converted = |message: Value| {
            prepare(
                ProviderProfile::Mistral,
                ProxyProtocol::OpenAiChatCompletions,
                Bytes::from(json!({"messages":[message]}).to_string()),
            )
            .map(|bytes| serde_json::from_slice::<Value>(&bytes).unwrap()["messages"][0].clone())
        };
        let native = json!({"role":"assistant","content":[{"type":"thinking","thinking":[{"type":"text","text":"native"}]},{"type":"text","text":"answer"}],"tool_calls":[]});
        assert_eq!(converted(native.clone()).unwrap(), native);
        for absent in [json!(null), json!(""), json!(" \t")] {
            let message = converted(json!({"role":"assistant","content":null,"reasoning_content":absent,"tool_calls":[{"id":"call_fixture"}]})).unwrap();
            assert!(message.get("reasoning_content").is_none());
            assert!(message["content"].is_null());
            assert_eq!(message["tool_calls"], json!([{"id":"call_fixture"}]));
        }
        for reasoning in [json!("exact"), json!("")] {
            let output = converted(json!({"role":"assistant","content":[{"type":"text","text":"answer"}],"reasoning":reasoning,"reasoning_content":reasoning})).unwrap();
            assert!(output.get("reasoning").is_none() && output.get("reasoning_content").is_none());
        }
        for invalid in [
            json!({"role":"assistant","reasoning":"one","reasoning_content":"two"}),
            json!({"role":"assistant","reasoning_content":42}),
            json!({"role":"assistant","reasoning_content":"one","content":42}),
            json!({"role":"assistant","reasoning_content":"one","content":[{"type":"thinking","thinking":"two"}]}),
        ] {
            assert!(converted(invalid).is_err());
        }
        let user = json!({"role":"tool","content":{"reasoning_content":"user data"},"tool_call_id":"call_fixture"});
        assert_eq!(converted(user.clone()).unwrap(), user);
        let generic = Bytes::from(
            json!({"messages":[{"role":"assistant","reasoning_content":"exact"}]}).to_string(),
        );
        assert_eq!(
            prepare(
                ProviderProfile::Generic,
                ProxyProtocol::OpenAiChatCompletions,
                generic.clone()
            )
            .unwrap(),
            generic
        );
    }
    #[test]
    fn kimi_models_have_distinct_thinking_contracts() {
        let prepare_kimi = |body: Value| {
            prepare(
                ProviderProfile::Kimi,
                ProxyProtocol::OpenAiChatCompletions,
                Bytes::from(body.to_string()),
            )
            .map(|bytes| serde_json::from_slice::<Value>(&bytes).unwrap())
        };
        let k3 = prepare_kimi(json!({"model":"kimi-k3","reasoning_effort":"high","temperature":0.5,"messages":[{"role":"assistant","reasoning_content":"exact"}]})).unwrap();
        assert_eq!(k3["reasoning_effort"], "high");
        assert!(k3.get("thinking").is_none() && k3.get("temperature").is_none());
        assert_eq!(k3["messages"][0]["reasoning_content"], "exact");
        assert!(prepare_kimi(json!({"model":"kimi-k3","reasoning_effort":"none"})).is_err());
        assert!(
            prepare_kimi(json!({"model":"kimi-k2.7-code","thinking":{"type":"disabled"}})).is_err()
        );
        assert_eq!(
            prepare_kimi(json!({"model":"kimi-k2.6","reasoning_effort":"none"})).unwrap()
                ["thinking"]["type"],
            "disabled"
        );
    }
    #[test]
    fn profiles_preserve_reasoning_replay_and_do_not_touch_gateways() {
        let input = Bytes::from(json!({"model":"deepseek-reasoner", "max_completion_tokens":4096,"temperature":0.3,
            "messages":[{"role":"assistant","reasoning_content":"exact prior thinking","tool_calls":[]}]}).to_string());
        assert_eq!(
            prepare(
                ProviderProfile::Generic,
                ProxyProtocol::OpenAiChatCompletions,
                input.clone()
            )
            .unwrap(),
            input
        );
        let body: Value = serde_json::from_slice(
            &prepare(
                ProviderProfile::DeepSeek,
                ProxyProtocol::OpenAiChatCompletions,
                input,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(body["max_tokens"], 4096);
        assert!(body.get("temperature").is_none());
        assert_eq!(
            body["messages"][0]["reasoning_content"],
            "exact prior thinking"
        );
    }
}
