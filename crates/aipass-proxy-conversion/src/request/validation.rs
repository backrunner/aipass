//! Distinguish malformed input from a target wire that cannot preserve it.
//! The router may skip the latter before sending anything upstream.
use super::*;
use crate::unsupported;

pub(super) fn validate(
    from: ProxyProtocol,
    to: ProxyProtocol,
    payload: &Value,
) -> Result<(), ConversionError> {
    let source = object(payload, from)?;
    tool_choice(payload, from, to)?;
    let incompatible = |field: &str| Err(unsupported(from, to, field));
    let present = |field: &str| source.get(field).is_some_and(|v| !v.is_null());
    if from == CC {
        if let Some(n) = source.get("n").filter(|v| !v.is_null()) {
            let n = n
                .as_u64()
                .filter(|n| *n > 0)
                .ok_or_else(|| invalid(CC, "n must be a positive integer"))?;
            if n != 1 {
                return incompatible("multiple choices");
            }
        }
        let fields: &[&str] = if to == AM {
            &["response_format", "audio", "modalities", "function_call"]
        } else {
            &["functions", "function_call", "stop", "audio"]
        };
        for field in fields {
            if present(field) {
                return incompatible(field);
            }
        }
    }
    if from == AM {
        if to == RS
            && source
                .get("stop_sequences")
                .is_some_and(|v| !v.is_null() && v.as_array().is_none_or(|a| !a.is_empty()))
        {
            return incompatible("stop_sequences");
        }
        if payload
            .pointer("/output_config/format")
            .is_some_and(|v| !v.is_null())
        {
            return incompatible("structured output format");
        }
    }
    if from == RS
        && to == AM
        && payload
            .pointer("/text/format")
            .is_some_and(|v| !v.is_null())
    {
        return incompatible("structured output format");
    }
    if let Some(tools) = source.get("tools").filter(|v| !v.is_null()) {
        for tool in tools
            .as_array()
            .ok_or_else(|| invalid(from, "tools must be an array"))?
        {
            let kind = tool["type"].as_str().unwrap_or("");
            let supported = match from {
                AM => matches!(kind, "" | "custom"),
                CC => matches!(kind, "function" | "custom"),
                RS if to == AM => kind == "function",
                RS => matches!(kind, "function" | "custom"),
            };
            if !supported {
                return incompatible(&format!("tool type {kind}"));
            }
        }
    }
    // OpenAI-to-OpenAI content conversion already rejects unknown parts.
    if from != AM && to != AM {
        return Ok(());
    }
    let input = source.get(if from == RS { "input" } else { "messages" });
    if from == RS && input.is_some_and(Value::is_string) {
        return Ok(());
    }
    let items = input
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(from, "missing conversation array"))?;
    for item in items {
        if from == RS {
            match item["type"].as_str().unwrap_or("") {
                "reasoning" | "function_call" => continue,
                "function_call_output" => {
                    if item["output"].is_array() {
                        parts(&item["output"], from, to)?;
                    }
                    continue;
                }
                "" | "message" => {}
                kind => return incompatible(&format!("input item {kind}")),
            }
        }
        let role = item["role"]
            .as_str()
            .ok_or_else(|| invalid(from, "missing message role"))?;
        let roles: &[&str] = if from == AM {
            &["user", "assistant"]
        } else {
            &["user", "assistant", "system", "developer", "tool"]
        };
        if !roles.contains(&role) {
            return incompatible(&format!("message role {role}"));
        }
        let content = &item["content"];
        if !(content.is_string() || content.is_array() || role == "assistant" && content.is_null())
        {
            return Err(invalid(from, "message content must be text or an array"));
        }
        if content.is_array() {
            parts(content, from, to)?;
            // These paths extract text, so accepting images here would lose
            // them even though user/tool image blocks are representable.
            if (matches!(role, "system" | "developer") || from == CC && role == "assistant")
                && content
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| matches!(p["type"].as_str(), Some("image_url" | "input_image")))
            {
                return incompatible("image in a text-only history role");
            }
        }
        if from == CC && item.get("function_call").is_some_and(|v| !v.is_null()) {
            return incompatible("legacy function_call");
        }
    }
    Ok(())
}
fn parts(value: &Value, from: ProxyProtocol, to: ProxyProtocol) -> Result<(), ConversionError> {
    for part in value
        .as_array()
        .ok_or_else(|| invalid(from, "content must be an array"))?
    {
        let kind = part["type"]
            .as_str()
            .ok_or_else(|| invalid(from, "content block needs a type"))?;
        let allowed = match from {
            AM => matches!(
                kind,
                "text" | "image" | "tool_use" | "tool_result" | "thinking"
            ),
            CC => matches!(kind, "text" | "image_url" | "thinking"),
            RS => matches!(
                kind,
                "input_text" | "output_text" | "input_image" | "refusal"
            ),
        };
        if !allowed {
            return Err(unsupported(from, to, format!("content block {kind}")));
        }
        if kind == "tool_result" && part["content"].is_array() {
            parts(&part["content"], from, to)?;
        }
        if (kind == "image" && am_image_url(part).is_none())
            || (kind == "image_url" && !part["image_url"]["url"].is_string())
            || (kind == "input_image" && !part["image_url"].is_string())
        {
            return Err(unsupported(from, to, "image source"));
        }
    }
    Ok(())
}

// Both APIs can preserve serial/parallel tool execution constraints.
pub(super) fn parallel(
    from: ProxyProtocol,
    payload: &Value,
) -> Result<Option<bool>, ConversionError> {
    let value = if from == AM {
        payload.pointer("/tool_choice/disable_parallel_tool_use")
    } else {
        payload.get("parallel_tool_calls")
    };
    value
        .filter(|v| !v.is_null())
        .map(|v| {
            v.as_bool()
                .map(|b| if from == AM { !b } else { b })
                .ok_or_else(|| invalid(from, "parallel tool constraint must be boolean"))
        })
        .transpose()
}
pub(super) fn apply_parallel(to: ProxyProtocol, parallel: Option<bool>, output: &mut Value) {
    if let Some(parallel) = parallel {
        if to != AM {
            output["parallel_tool_calls"] = json!(parallel);
        } else if output["tools"].as_array().is_some_and(|t| !t.is_empty())
            && output["tool_choice"]["type"] != "none"
        {
            if !output["tool_choice"].is_object() {
                output["tool_choice"] = json!({"type":"auto"});
            }
            output["tool_choice"]["disable_parallel_tool_use"] = json!(!parallel);
        }
    }
}

fn tool_choice(
    payload: &Value,
    from: ProxyProtocol,
    to: ProxyProtocol,
) -> Result<(), ConversionError> {
    let Some(choice) = payload.get("tool_choice").filter(|v| !v.is_null()) else {
        return Ok(());
    };
    if from != AM && choice.is_string() {
        return if matches!(choice.as_str(), Some("auto" | "none" | "required")) {
            Ok(())
        } else {
            Err(unsupported(from, to, "tool_choice restriction"))
        };
    }
    if !choice.is_object() {
        return Err(invalid(from, "tool_choice must be a supported selector"));
    }
    let kind = choice["type"]
        .as_str()
        .ok_or_else(|| invalid(from, "tool_choice needs a type"))?;
    let supported = match from {
        AM => matches!(kind, "auto" | "any" | "none" | "tool"),
        CC => matches!(kind, "function" | "custom"),
        RS => kind == "function" || kind == "custom" && to == CC,
    };
    if !supported {
        return Err(unsupported(from, to, "tool_choice restriction"));
    }
    if matches!(kind, "tool" | "function" | "custom") {
        let name = if from == CC {
            &choice[kind]["name"]
        } else {
            &choice["name"]
        };
        if !name.as_str().is_some_and(|s| !s.is_empty()) {
            return Err(invalid(from, "selected tool needs a name"));
        }
    }
    Ok(())
}
