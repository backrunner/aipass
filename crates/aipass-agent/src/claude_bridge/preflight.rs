//! Separate invalid caller input from capabilities unavailable on this target.
use super::*;

#[derive(Debug)]
pub(super) struct Error {
    message: String,
    incompatible: bool,
}
impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}
impl Error {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            incompatible: false,
        }
    }
    pub(super) fn unsupported(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            incompatible: true,
        }
    }
    pub(super) fn into_response(self) -> SubscriptionResponse {
        if self.incompatible {
            SubscriptionResponse::incompatible_target(&self.message)
        } else {
            SubscriptionResponse::rejected_request(hyper::StatusCode::BAD_REQUEST, &self.message)
        }
    }
}
impl From<String> for Error {
    fn from(message: String) -> Self {
        Self::invalid(message)
    }
}
impl From<&str> for Error {
    fn from(message: &str) -> Self {
        Self::invalid(message)
    }
}

pub(super) fn check(
    payload: &Value,
    session: Option<&str>,
) -> Result<(bool, Option<session_options::Options>, cache::Plan), Error> {
    if payload.to_string().len() > MAX_LINE as usize {
        return Err("Claude input exceeds bridge limit".into());
    }
    let messages = payload["messages"]
        .as_array()
        .ok_or("Claude messages are required")?;
    if messages.last().is_none_or(|m| m["role"] != "user") {
        return Err("Claude ACP requires a final user turn".into());
    }
    if let Some(parts) = messages.last().and_then(|m| m["content"].as_array()) {
        let native_results = parts.iter().any(|part| {
            part["type"] == "tool_result"
                && part["tool_use_id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("toolu_aipass_claude_"))
        });
        if native_results && parts.iter().any(|part| part["type"] != "tool_result") {
            return Err("return pending Claude tool results alone; embed tool images in their result and send new user instructions after the turn completes".into());
        }
    }
    let options = session_options::Options::read(payload, session)?;
    let cache = cache::Plan::read(payload)?;
    let mut unsupported = None;
    if let Some(tools) = payload.get("tools").filter(|v| !v.is_null()) {
        let tools = tools.as_array().ok_or("Claude tools must be an array")?;
        let mut names = std::collections::HashSet::new();
        for tool in tools {
            let name = tool["name"]
                .as_str()
                .ok_or("Claude tool name is required")?;
            if name.is_empty()
                || name == "aipass_acp_respond"
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                || !names.insert(name)
            {
                return Err("Claude tool names must be unique, non-reserved identifiers".into());
            }
            if let Some(schema) = tool.get("input_schema").filter(|v| !v.is_null()) {
                if !schema.is_object() {
                    return Err("Claude tool input_schema must be an object".into());
                }
                if ["oneOf", "anyOf", "allOf", "$ref"]
                    .iter()
                    .any(|key| schema.get(key).is_some())
                    || schema.get("type").is_some_and(|kind| kind != "object")
                {
                    unsupported = Some("Claude Code tools require an object root without root-level combinators or references; choose a compatible target rather than weakening the schema");
                }
            }
        }
    }
    // Validate shape before deciding which target can implement the options.
    for field in ["temperature", "top_p", "top_k"] {
        if let Some(value) = payload.get(field).filter(|v| !v.is_null()) {
            if !value.is_number() {
                return Err(format!("{field} must be a number").into());
            }
            unsupported = Some("Claude Code subscription does not expose sampling controls");
        }
    }
    if let Some(stops) = payload.get("stop_sequences").filter(|v| !v.is_null()) {
        let stops = stops
            .as_array()
            .filter(|v| v.iter().all(Value::is_string))
            .ok_or("stop_sequences must be an array of strings")?;
        if !stops.is_empty() {
            unsupported = Some("Claude Code subscription does not expose stop-sequence controls");
        }
    }
    if let Some(choice) = payload.get("tool_choice").filter(|v| !v.is_null()) {
        match choice["type"].as_str() {
            Some("auto") => {}
            Some("any" | "none") => {
                unsupported = Some("Claude Code subscription requires automatic tool choice")
            }
            Some("tool") if choice["name"].as_str().is_some_and(|s| !s.is_empty()) => {
                unsupported = Some("Claude Code subscription requires automatic tool choice");
            }
            _ => return Err("invalid Claude tool choice".into()),
        }
        if let Some(value) = choice.get("disable_parallel_tool_use") {
            let disabled = value
                .as_bool()
                .ok_or("disable_parallel_tool_use must be a boolean")?;
            if disabled {
                unsupported = Some("Claude Code subscription cannot enforce serial tool calls");
            }
        }
    }
    if let Some(config) = payload.get("output_config").filter(|v| !v.is_null()) {
        if !config.is_object() {
            return Err("output_config must be an object".into());
        }
        if config.get("format").is_some_and(|v| !v.is_null()) {
            unsupported =
                Some("Claude Code subscription does not expose this structured output format");
        }
    }
    let web_search = native_tools::search_requested(payload)?;
    if let Some(message) = unsupported {
        return Err(Error::unsupported(message));
    }
    Ok((web_search, options, cache))
}
