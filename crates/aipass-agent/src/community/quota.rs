//! Subscription snapshots, refresh scheduling and route quota scopes.
use super::*;

pub(super) fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}
pub(super) fn snapshot(provider: &str, value: &Value) -> SubscriptionSnapshot {
    SubscriptionSnapshot {
        sections: if crate::provider_plugins::is_plugin(provider) {
            value
                .get("sections")
                .and_then(|s| serde_json::from_value(s.clone()).ok())
                .unwrap_or_default()
        } else {
            Vec::new()
        },
        billing_period_ends_at: value["until"]
            .as_str()
            .and_then(|s| {
                time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
            })
            .and_then(|t| {
                t.format(&time::format_description::well_known::Rfc3339)
                    .ok()
            }),
        quota_unavailable: value["quotaUnavailable"] == true,
        plan: value["plan"].as_str().map(str::to_owned),
        credits_remaining: value["balance"].as_str().map(str::to_owned).or_else(|| {
            value["balance"]
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.0)
                .map(|n| n.to_string())
        }),
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
    let initial_marker = with_vault(state, false, |vault| {
        vault
            .reveal_secret(id)
            .map(SensitiveString::new)
            .map_err(map_vault_error)
    })?;
    super::scope::read_runtime(state, id, initial_marker.expose())?;
    let (marker, provider, console_url, proxy, bridge, local) =
        with_vault(state, false, |vault| {
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
            let console_url = super::console::account_url(&account);
            Ok((
                marker,
                account.provider,
                console_url,
                outbound,
                proxy.community_bridge(),
                account.local_binding.is_some(),
            ))
        })?;
    let usage = match bridge.refresh(id, marker.expose(), &proxy) {
        Ok(usage) => usage,
        // An in-flight generation does not invalidate the previous quota reading.
        Err(error) if error == ACCOUNT_BUSY => return Err(invalid(error)),
        Err(error) if local => return Err(invalid(error)),
        Err(error) => json!({"error":error}),
    };
    let mut snapshot = snapshot(&provider, &usage);
    snapshot.console_url = console_url;
    with_vault(state, false, |vault| {
        read(vault, id, marker.expose())?;
        if snapshot.error.is_some() {
            if let Some(previous) = vault
                .get_provider_summary(id)
                .map_err(map_vault_error)?
                .subscription
            {
                snapshot = failed_snapshot(previous, &snapshot);
            }
        }
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
            // Failed snapshots retain the last successful reading's timestamp.
            // Schedule retries from the attempt receipt instead of that cached date.
            let receipt = vault
                .provider_runtime_extension(entry.id, "community_usage_v1")
                .map_err(map_vault_error)?;
            let attempted_at = receipt.as_ref().and_then(|raw| {
                serde_json::from_str::<Value>(raw.expose())
                    .ok()
                    .and_then(|v| v["observedAt"].as_str().map(str::to_owned))
            });
            let previous = attempted_at
                .as_deref()
                .or_else(|| entry.subscription.as_ref().map(|s| s.observed_at.as_str()))
                .and_then(|date| {
                    time::OffsetDateTime::parse(
                        date,
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
    if vault
        .get_provider_summary(id)
        .ok()
        .and_then(|e| e.provider_id)
        .as_deref()
        == Some("codex")
    {
        if let Ok(prefs) = crate::provider_runtime::load(vault, id) {
            // Admission performs a live CLI allowance read before generation
            // when reset is enabled and paid credits remain disabled.
            if prefs.subscription.quota_use_limit_percent.is_none()
                && (prefs.subscription.allow_paid_credits || prefs.subscription.auto_reset_credits)
            {
                return Vec::new();
            }
        }
    }
    native_quota(vault, id)
}

pub(crate) fn native_quota(vault: &Vault, id: Uuid) -> Vec<aipass_proxy::QuotaWindow> {
    let raw = vault
        .provider_runtime_extension(id, "community_usage_v1")
        .ok()
        .flatten();
    let value = raw
        .and_then(|raw| serde_json::from_str::<Value>(raw.expose()).ok())
        .unwrap_or_else(|| json!({}));
    let value = super::feedback::usage(vault, id, value);
    let timestamp = |v: &str| {
        time::OffsetDateTime::parse(v, &time::format_description::well_known::Rfc3339)
            .ok()
            .and_then(|t| u64::try_from(t.unix_timestamp()).ok())
    };
    let observed_at = value["observedAt"]
        .as_str()
        .and_then(timestamp)
        .unwrap_or(0);
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
                span_seconds: w["span"].as_u64(),
                model_family: None,
                used_basis_points: (used.min(100.) * 100.).round() as u16,
                observed_at: w["observedAt"].as_u64().unwrap_or(observed_at),
                resets_at: w["resetsAt"].as_str().and_then(timestamp),
                models: strings("models"),
                not_models: strings("notModels").unwrap_or_default(),
            })
        })
        .collect()
}

// Keep the last reading visibly stale when a refresh fails. Routing still uses
// the failed raw response above and never treats these cached bars as current.
fn failed_snapshot(
    mut previous: SubscriptionSnapshot,
    failure: &SubscriptionSnapshot,
) -> SubscriptionSnapshot {
    previous.error = failure.error.clone();
    previous.stale = true;
    previous
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    #[test]
    fn numeric_credits_and_failed_refresh_preserve_the_last_reading() {
        let old = snapshot(
            "codex",
            &json!({"balance":0,"windows":[{"name":"weekly","used":29,"span":604800}]}),
        );
        assert_eq!(old.credits_remaining.as_deref(), Some("0"));
        assert_eq!(old.windows[0].window_minutes, Some(10080));
        let failed = failed_snapshot(
            old.clone(),
            &snapshot("codex", &json!({"error":"read failed"})),
        );
        assert_eq!(failed.windows, old.windows);
        assert_eq!(failed.observed_at, old.observed_at);
        assert_eq!(failed.error.as_deref(), Some("read failed"));
        assert!(failed.stale);
        assert_eq!(snapshot("codex", &json!({})).credits_remaining, None);
    }
    #[test]
    fn a_billing_period_end_is_preserved_without_claiming_subscription_cancellation() {
        let snapshot = snapshot("zed", &json!({"until":"2026-11-01T00:00:00Z","plan":"pro"}));
        assert_eq!(
            snapshot.billing_period_ends_at.as_deref(),
            Some("2026-11-01T00:00:00Z")
        );
        assert!(snapshot.subscription_expires_at.is_none());
        assert!(snapshot.subscription_renews_at.is_none());
    }
}
