//! Public group models and member-model bindings, independent of transports.
use super::*;
use serde_json::Value;

pub(crate) fn model_id(route: &ResolvedRoute) -> Option<String> {
    route
        .targets
        .iter()
        .any(|target| target.config.enabled && target.config.model.is_some())
        .then(|| format!("group/{}", route.config.id))
}

pub(crate) fn validate_request(
    route: &ResolvedRoute,
    model: Option<&str>,
    payload: Option<&Value>,
) -> Result<(), String> {
    let Some(expected) = model_id(route) else {
        return Ok(());
    };
    if model != Some(expected.as_str()) {
        return Err(format!("this routing group requires model {expected}"));
    }
    if !payload.is_some_and(Value::is_object) {
        return Err("group model routing requires a JSON object".into());
    }
    if route.targets.iter().any(|target| {
        target.config.enabled
            && target
                .config
                .model
                .as_deref()
                .is_none_or(|model| model.trim().is_empty())
    }) {
        return Err("every enabled group member must bind an upstream model".into());
    }
    Ok(())
}

/// Opaque history can continue only on its recorded account. Ordinary full
/// history and caller-owned tool IDs remain eligible for protocol conversion.
pub(crate) fn history_owner(
    state: &RuntimeState,
    route: &ResolvedRoute,
    payload: &Value,
    session: Option<&str>,
    owner: Option<Uuid>,
) -> Result<Option<Uuid>, String> {
    if model_id(route).is_none() || !has_bound_history(payload) {
        return Ok(owner);
    }
    let origin_key = payload
        .get("previous_response_id")
        .and_then(Value::as_str)
        .or_else(|| {
            payload.get("conversation").and_then(|value| {
                value
                    .as_str()
                    .or_else(|| value.get("id").and_then(Value::as_str))
            })
        })
        .or(session);
    let recorded = origin_key.and_then(|session| {
        let affinities = state.session_affinity.lock().ok()?;
        let affinity = affinities.get(&(route.config.id, session.to_owned()))?;
        (affinity.last_used.elapsed() < SESSION_AFFINITY_TTL).then_some(affinity.target_id)
    });
    if owner.is_some() && recorded.is_some() && owner != recorded {
        return Err("history mixes provider-bound sessions".into());
    }
    owner.or(recorded).map(Some).ok_or_else(|| {
        "provider-bound history has no recorded origin; resend full portable history".into()
    })
}

fn has_bound_history(value: &Value) -> bool {
    ["previous_response_id", "conversation"].iter().any(|key| {
        value
            .get(key)
            .is_some_and(|v| !v.is_null() && v.as_str() != Some(""))
    }) || value
        .get("messages")
        .or_else(|| value.get("input"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|item| {
            bound_block(item)
                || item
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .any(bound_block)
        })
}

fn bound_block(value: &Value) -> bool {
    match value.get("type").and_then(Value::as_str) {
        Some("redacted_thinking") => true,
        Some("thinking") => value.get("signature").is_some_and(|v| !v.is_null()),
        Some("reasoning") => value.get("encrypted_content").is_some_and(|v| !v.is_null()),
        _ => false,
    }
}

pub(crate) fn models_response(model: &str, protocol: ProxyProtocol) -> Response<BoxBody> {
    // Advertise only the configured abstraction; individual members can have
    // different limits, prices and features, so do not invent shared capabilities.
    let api = match protocol {
        ProxyProtocol::OpenAiResponses => "responses",
        ProxyProtocol::OpenAiChatCompletions => "chat_completions",
        ProxyProtocol::AnthropicMessages => "anthropic_messages",
    };
    let entry = if protocol == ProxyProtocol::AnthropicMessages {
        serde_json::json!({"id":model,"type":"model","display_name":model,"created_at":"1970-01-01T00:00:00Z","api_types":[api]})
    } else {
        serde_json::json!({"id":model,"object":"model","created":0,"owned_by":"aipass","api_types":[api]})
    };
    let mut payload =
        serde_json::json!({"data":[entry],"has_more":false,"first_id":model,"last_id":model});
    if protocol != ProxyProtocol::AnthropicMessages {
        payload["object"] = serde_json::json!("list");
    }
    Response::builder()
        .header(header::CONTENT_TYPE, "application/json")
        .body(
            Full::new(Bytes::from(payload.to_string()))
                .map_err(|never| -> BoxError { match never {} })
                .boxed_unsync(),
        )
        .expect("static model response")
}
