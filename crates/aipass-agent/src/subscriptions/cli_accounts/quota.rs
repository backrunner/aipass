//! Pure provider allowance normalization.
use super::*;

pub(super) fn copilot_usage(v: &Value, models: &Value) -> Result<Value> {
    fn number(v: &Value) -> Option<f64> {
        v.as_f64()
            .or_else(|| v.as_str()?.parse().ok())
            .filter(|n| n.is_finite())
    }
    let managed = matches!(
        v["copilot_plan"]
            .as_str()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "business" | "enterprise"
    );
    let mut windows = v["quota_snapshots"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(name, q)| {
            let premium = name == "premium_interactions" || name == "premium";
            let cap = number(&q["entitlement"]);
            let used = if q["unlimited"] == true {
                // GitHub marks an organization's pooled allowance unlimited
                // even when the seat can no longer use that pool.
                if premium && q["has_quota"] == false {
                    100.0
                } else {
                    return None;
                }
            } else {
                if cap.is_some_and(|n| n <= 0.0) {
                    return None;
                }
                // Token billing rounds percent_remaining. Counts retain the
                // actual reading (e.g. 0.1 of 20000, rather than 0.1%).
                if let Some((cap, left)) = cap.zip(number(&q["quota_remaining"])) {
                    100.0 * (1.0 - left / cap)
                } else {
                    100.0 - number(&q["percent_remaining"])
                        .filter(|n| (0.0..=100.0).contains(n))?
                }
            };
            let reset = number(&q["quota_reset_at"])
                .filter(|t| *t > 0.0)
                .and_then(|t| time::OffsetDateTime::from_unix_timestamp(t as i64).ok())
                .and_then(|t| {
                    t.format(&time::format_description::well_known::Rfc3339)
                        .ok()
                })
                .or_else(|| device::date(&v["quota_reset_date_utc"]))
                .or_else(|| {
                    let date = v["quota_reset_date"].as_str()?;
                    device::date(&json!(format!("{date}T00:00:00Z")))
                });
            let mut window =
                json!({"name":name,"used":used.clamp(0.0,100.0),"span":2592000,"resetsAt":reset,"aside":name != "chat"});
            if premium {
                let mut scoped = models
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter(|(_, m)| {
                        m["premium"] == true
                            || m["premiumMultiplier"].as_f64().is_some_and(|n| n > 0.0)
                    })
                    .map(|(id, _)| id.to_owned())
                    .collect::<Vec<_>>();
                scoped.sort();
                // An unknown billing multiplier cannot exhaust unrelated free models.
                if !scoped.is_empty() {
                    window["models"] = json!(scoped);
                    window["aside"] = json!(q["overage_permitted"] == true && !managed);
                }
            }
            Some(window)
        })
        .collect::<Vec<_>>();
    // Preserve the presentation order independently of serde_json's globally
    // unified preserve_order feature or the vendor's object insertion order.
    windows.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    if windows.is_empty() && v["copilot_plan"].is_null() {
        return Err("Copilot returned no allowance data".into());
    }
    Ok(json!({"plan":v["copilot_plan"],"windows":windows}))
}

pub(super) fn gemini_usage(v: &Value) -> Result<Value> {
    let windows = v["buckets"].as_array().into_iter().flatten().filter_map(|b| {
        let model = b["modelId"].as_str().filter(|s| !s.trim().is_empty())?;
        let remaining = device::percent(&b["remainingFraction"])
            .filter(|n| (0.0..=1.0).contains(n))?;
        Some(json!({"name":model,"models":[model],"used":100.0*(1.0-remaining),"resetsAt":device::date(&b["resetTime"])}))
    }).collect::<Vec<_>>();
    if windows.is_empty() {
        return Err("Gemini returned no allowance data".into());
    }
    Ok(json!({"windows":windows}))
}

pub(super) fn codex_usage(value: &Value) -> Result<Value> {
    let base = &value["rateLimits"];
    let buckets = value["rateLimitsByLimitId"].as_object();
    let mut entries: Vec<(&str, &Value)> = match buckets {
        Some(b) if !b.is_empty() => b.iter().map(|(k, v)| (k.as_str(), v)).collect(),
        _ => vec![("codex", base)],
    };
    entries.sort_by_key(|(id, _)| *id);
    let base_id = base["limitId"].as_str().unwrap_or("codex");
    if !entries.iter().any(|(id, _)| *id == base_id)
        && (base["primary"].is_object() || base["secondary"].is_object())
    {
        entries.insert(0, (base_id, base));
    }
    let mut windows = Vec::new();
    for (id, bucket) in entries {
        for name in ["primary", "secondary"] {
            let w = &bucket[name];
            let Some(used) = w["usedPercent"]
                .as_f64()
                .filter(|v| v.is_finite() && *v >= 0.0)
            else {
                continue;
            };
            let reset = w["resetsAt"]
                .as_i64()
                .and_then(|t| time::OffsetDateTime::from_unix_timestamp(t).ok())
                .and_then(|t| {
                    t.format(&time::format_description::well_known::Rfc3339)
                        .ok()
                });
            let mut window = json!({"name":format!("{id} · {name}"),"used":used,"span":w["windowDurationMins"].as_u64().map(|n|n.saturating_mul(60)),"resetsAt":reset});
            if id != "codex" {
                window["aside"] = json!(true);
            }
            windows.push(window);
        }
    }
    if windows.is_empty() && base["planType"].is_null() {
        return Err("Codex CLI returned no allowance data".into());
    }
    let credits = &base["credits"];
    let balance = if credits["hasCredits"] == false || credits["unlimited"] == true {
        Value::Null
    } else {
        credits["balance"].clone()
    };
    Ok(
        json!({"plan":base["planType"],"balance":balance,"windows":windows,"resetCredits":value["rateLimitResetCredits"]}),
    )
}

#[cfg(test)]
mod tests;
