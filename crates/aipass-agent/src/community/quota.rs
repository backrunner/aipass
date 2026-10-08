//! Subscription snapshots, refresh scheduling and route quota scopes.
use super::*;

pub(super) fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}
pub(super) fn snapshot(provider: &str, value: &Value) -> SubscriptionSnapshot {
    SubscriptionSnapshot {
        plan: value["plan"].as_str().map(str::to_owned),
        credits_remaining: value["balance"].as_str().map(str::to_owned),
        source: format!("community:{provider}"),
        observed_at: now(),
        error: value["error"].as_str().map(str::to_owned),
        stale: value["error"].is_string(),
        windows: value["windows"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(i, w)| SubscriptionWindow {
                id: if w["aside"] != true
                    && w.get("models").is_none()
                    && w.get("notModels").is_none()
                {
                    format!("community_account_{i}")
                } else {
                    format!("community_scoped_{i}")
                },
                label: w["name"].as_str().unwrap_or("Usage").to_owned(),
                used_percent: w["used"].as_f64().filter(|v| v.is_finite() && *v >= 0.),
                resets_at: w["resetsAt"].as_str().map(str::to_owned),
                window_minutes: w["span"].as_u64().map(|seconds| seconds / 60),
                source: Some(format!("community:{provider}")),
            })
            .collect(),
        ..Default::default()
    }
}
pub(crate) fn refresh(state: &Arc<AgentState>, id: Uuid) -> ServiceResult<SubscriptionSnapshot> {
    let (marker, provider, proxy, bridge) = with_vault(state, false, |vault| {
        let marker = SensitiveString::new(vault.reveal_secret(id).map_err(map_vault_error)?);
        let account = decode(read(vault, id, marker.expose())?.expose())?;
        let prefs = crate::provider_runtime::load(vault, id)?;
        let mut proxy = state
            .proxy
            .lock()
            .map_err(|_| invalid("proxy unavailable"))?;
        let global = proxy.load_config(vault)?.upstream_proxy;
        let outbound = crate::provider_runtime::outbound(&prefs)
            .map_err(invalid)?
            .unwrap_or(global);
        Ok((marker, account.provider, outbound, proxy.community_bridge()))
    })?;
    let usage = match bridge.refresh(id, marker.expose(), &proxy) {
        Ok(usage) => usage,
        // An in-flight generation does not invalidate the previous quota reading.
        Err(error) if error == ACCOUNT_BUSY => return Err(invalid(error)),
        Err(error) => json!({"error":error}),
    };
    let snapshot = snapshot(&provider, &usage);
    with_vault(state, false, |vault| {
        read(vault, id, marker.expose())?;
        vault
            .set_provider_runtime_extension(
                id,
                "community_usage_v1",
                Some(&SecretString::new(
                    json!({"observedAt":now(),"usage":usage}).to_string(),
                )),
            )
            .map_err(map_vault_error)?;
        vault
            .update_provider_subscription(id, Some(snapshot.clone()))
            .map_err(map_vault_error)?;
        state
            .proxy
            .lock()
            .map_err(|_| invalid("proxy unavailable"))?
            .refresh_provider_credentials(vault, id)?;
        Ok(())
    })?;
    state.sync_revision.fetch_add(1, Ordering::Relaxed);
    Ok(snapshot)
}
pub(crate) fn refresh_due(state: &Arc<AgentState>) {
    let due = with_vault(state, false, |vault| {
        let mut due = Vec::new();
        for entry in vault.list_provider_summaries().map_err(map_vault_error)? {
            if !is_account(vault, entry.id) {
                continue;
            }
            let preferences = crate::provider_runtime::load(vault, entry.id)?;
            if !preferences.quota_tracking {
                continue;
            }
            let previous = entry.subscription.as_ref().and_then(|s| {
                time::OffsetDateTime::parse(
                    &s.observed_at,
                    &time::format_description::well_known::Rfc3339,
                )
                .ok()
            });
            if previous.is_none_or(|t| {
                (time::OffsetDateTime::now_utc() - t).whole_seconds()
                    >= preferences.quota_refresh_seconds as i64
            }) {
                due.push(entry.id);
            }
        }
        Ok(due)
    });
    if let Ok(due) = due {
        for id in due {
            let _ = refresh(state, id);
        }
    }
}

pub(crate) fn quota(vault: &Vault, id: Uuid) -> Vec<aipass_proxy::QuotaWindow> {
    let Some(raw) = vault
        .provider_runtime_extension(id, "community_usage_v1")
        .ok()
        .flatten()
    else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(raw.expose()) else {
        return Vec::new();
    };
    let timestamp = |v: &str| {
        time::OffsetDateTime::parse(v, &time::format_description::well_known::Rfc3339)
            .ok()
            .and_then(|t| u64::try_from(t.unix_timestamp()).ok())
    };
    let Some(observed_at) = value["observedAt"].as_str().and_then(timestamp) else {
        return Vec::new();
    };
    if value["usage"]["error"].is_string() {
        return Vec::new();
    }
    value["usage"]["windows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|w| {
            if w["aside"] == true {
                return None;
            }
            let used = w["used"].as_f64().filter(|v| v.is_finite() && *v >= 0.)?;
            let strings = |field: &str| -> Option<Vec<String>> {
                w.get(field).map(|v| {
                    v.as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
            };
            Some(aipass_proxy::QuotaWindow {
                used_basis_points: (used.min(100.) * 100.).round() as u16,
                observed_at,
                resets_at: w["resetsAt"].as_str().and_then(timestamp),
                models: strings("models"),
                not_models: strings("notModels").unwrap_or_default(),
            })
        })
        .collect()
}
