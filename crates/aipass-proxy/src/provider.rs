//! Provider contracts selected from vault metadata, never guessed from URLs.
use super::*;
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProviderProfile {
    #[default]
    Generic,
    OpenAi,
    DeepSeek,
    Kimi,
    Mistral,
    GeminiCompatible,
}

pub(crate) fn prepare(
    profile: ProviderProfile,
    protocol: ProxyProtocol,
    bytes: Bytes,
) -> Result<Bytes, String> {
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

#[cfg(test)]
mod tests {
    use super::*;
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
