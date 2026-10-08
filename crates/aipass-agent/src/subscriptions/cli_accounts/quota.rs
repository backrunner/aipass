//! Pure provider allowance normalization.
use super::*;

pub(super) fn copilot_usage(v: &Value, models: &Value) -> Result<Value> {
    fn number(v: &Value) -> Option<f64> {
        v.as_f64()
            .or_else(|| v.as_str()?.parse().ok())
            .filter(|n| n.is_finite())
    }
    let windows = v["quota_snapshots"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(name, q)| {
            if q["unlimited"] == true {
                return None;
            }
            let used = if let Some(percent) =
                number(&q["percent_remaining"]).filter(|n| (0.0..=100.0).contains(n))
            {
                100.0 - percent
            } else {
                let cap = number(&q["entitlement"]).filter(|n| *n > 0.0)?;
                let left = number(&q["quota_remaining"]).filter(|n| *n >= 0.0)?;
                100.0 * (1.0 - left / cap)
            };
            let reset = number(&q["quota_reset_at"])
                .filter(|t| *t > 0.0)
                .and_then(|t| time::OffsetDateTime::from_unix_timestamp(t as i64).ok())
                .and_then(|t| {
                    t.format(&time::format_description::well_known::Rfc3339)
                        .ok()
                })
                .or_else(|| v["quota_reset_date_utc"].as_str().map(str::to_owned));
            let mut window =
                json!({"name":name,"used":used.max(0.0),"resetsAt":reset,"aside":true});
            if name.contains("premium") {
                let scoped = models
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter(|(_, m)| m["premiumMultiplier"].as_f64().is_some_and(|n| n > 0.0))
                    .map(|(id, _)| id.to_owned())
                    .collect::<Vec<_>>();
                // An unknown billing multiplier cannot exhaust unrelated free models.
                if !scoped.is_empty() {
                    window["models"] = json!(scoped);
                    window["aside"] = json!(false);
                }
            }
            Some(window)
        })
        .collect::<Vec<_>>();
    if windows.is_empty() && v["copilot_plan"].is_null() {
        return Err("Copilot returned no allowance data".into());
    }
    Ok(json!({"plan":v["copilot_plan"],"windows":windows}))
}

pub(super) fn codex_usage(value: &Value) -> Result<Value> {
    let base = &value["rateLimits"];
    let buckets = value["rateLimitsByLimitId"].as_object();
    let entries: Vec<(&str, &Value)> = match buckets {
        Some(b) if !b.is_empty() => b.iter().map(|(k, v)| (k.as_str(), v)).collect(),
        _ => vec![("codex", base)],
    };
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
    Ok(json!({"plan":base["planType"],"balance":base["credits"]["balance"],"windows":windows}))
}
