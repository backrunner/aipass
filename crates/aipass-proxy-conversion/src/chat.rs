//! State shared by Chat's two streaming conversion paths.
use serde_json::Value;

#[derive(Default)]
pub(crate) struct Choice {
    selected: Option<u64>,
}
impl Choice {
    pub(crate) fn select<'a>(&mut self, choices: &'a [Value]) -> Option<&'a Value> {
        for choice in choices {
            let index = index(&choice["index"]);
            if self.selected.is_some() && index.is_some() && self.selected != index {
                continue;
            }
            // An unindexed role or usage prelude says nothing about which
            // choice owns the answer. Missing indices later mean continuation.
            let activity = choice["finish_reason"].as_str().is_some()
                || choice["delta"].as_object().is_some_and(|d| {
                    d.iter()
                        .any(|(k, v)| k != "role" && !v.is_null() && v.as_str() != Some(""))
                });
            if self.selected.is_none() && index.is_none() && !activity {
                continue;
            }
            self.selected.get_or_insert(index.unwrap_or(0));
            return Some(choice);
        }
        None
    }
}

pub(crate) fn index(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        let n = value
            .as_f64()
            .or_else(|| value.as_str()?.parse::<f64>().ok())?;
        (n.is_finite() && n >= 0.0 && n.fract() == 0.0 && n < u64::MAX as f64).then_some(n as u64)
    })
}

pub(crate) fn tool_index(call: &Value, position: usize) -> Result<u64, crate::ConversionError> {
    match call.get("index") {
        Some(value) => index(value).ok_or_else(|| {
            crate::invalid(
                crate::ProxyProtocol::OpenAiChatCompletions,
                "invalid tool index",
            )
        }),
        None => Ok(position as u64),
    }
}

/// Identity metadata may be repeated wholesale by relays. Arguments remain
/// deltas, and must never use this deduplication rule.
pub(crate) fn identity(current: &mut String, fragment: &str) {
    if current == fragment || fragment.is_empty() {
        return;
    }
    if fragment.starts_with(current.as_str()) {
        *current = fragment.to_owned();
    } else {
        current.push_str(fragment);
    }
}
