//! Subscription labels only; never use shortened identities for account matching.
use aipass_provider_registry::{SubscriptionSnapshot, SubscriptionWindow};
use aipass_vault::EntrySummary;

pub fn provider_name(id: &str, translate: &impl Fn(&str) -> String) -> String {
    let key = format!("subscriptionName.{id}");
    let value = translate(&key);
    if value != key {
        return value;
    }
    let id = match id {
        "anthropic" | "claude-code" => "claude",
        "openai" => "codex",
        "xai" => "grok",
        _ => id,
    };
    if id == "claude" {
        return "Claude".into();
    }
    catalog()
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| id.into())
}
pub fn catalog() -> &'static [super::Provider] {
    // Preview fixtures and display names only. Live selectors use CommunityCatalog.
    static CATALOG: std::sync::OnceLock<Vec<super::Provider>> = std::sync::OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../../../../crates/aipass-agent/src/subscriptions/catalog.json"
        ))
        .expect("checked Agent catalog")
    })
}
pub fn tail(value: &str, count: usize) -> String {
    value
        .chars()
        .skip(value.chars().count().saturating_sub(count))
        .collect()
}
pub fn readable(identity: &str) -> String {
    let last = identity.rsplit(':').next().unwrap_or(identity);
    if last
        .strip_prefix("key-")
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return format!("API key {}", tail(identity, 8));
    }
    let first = identity.split(':').next().unwrap_or(identity);
    if first.contains('@') && !first.chars().any(char::is_whitespace) {
        return first.into();
    }
    if identity.contains("::") {
        return identity
            .split("::")
            .filter(|s| !s.is_empty())
            .map(|s| match s {
                "zai" => "Z.ai",
                "bigmodel" => "BigModel",
                _ => s,
            })
            .collect::<Vec<_>>()
            .join(" / ");
    }
    if identity.starts_with("arn:")
        || (identity.len() >= 20 && identity.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-'))
    {
        return tail(identity, 8);
    }
    identity.into()
}
pub struct Account {
    pub title: String,
    pub subtitle: String,
    pub account: String,
    pub scope: String,
}
pub fn account(entry: &EntrySummary, translate: &impl Fn(&str) -> String) -> Account {
    let id = entry.provider_id.as_deref().unwrap_or_default();
    let provider = provider_name(id, &|key| key.into());
    let identity = entry.account_identity.as_deref().unwrap_or_default();
    let account = readable(identity);
    let prefix = [format!("{id} · "), format!("{provider} · ")]
        .into_iter()
        .find(|p| entry.title.starts_with(p));
    let friendly = prefix
        .as_ref()
        .map(|p| readable(entry.title[p.len()..].trim()))
        .unwrap_or_default();
    let generated = prefix.is_some() || entry.title == id || entry.title == provider;
    let workspace = if identity.contains('@') {
        identity.split_once(':').map(|(_, s)| s).unwrap_or_default()
    } else {
        ""
    };
    let parts: Vec<_> = identity.split(':').collect();
    let mut scope = workspace.to_owned();
    let mut context = String::new();
    if parts.len() == 3 && parts[0] == id && parts[2].starts_with("key-") {
        if id == "kilo-gateway" {
            context = parts[1].into();
        } else if matches!(
            id,
            "mimo-plan"
                | "bailian-token-plan"
                | "bailian-coding-plan"
                | "tencent-token-plan-enterprise"
        ) {
            let key = format!("subscriptionRegion.{}", parts[1]);
            let value = translate(&key);
            if value != key {
                context = value;
            }
        }
        if scope.is_empty() && !context.is_empty() {
            scope = parts[1].into();
        }
    }
    let mut subtitle = if generated {
        if friendly.is_empty() {
            account.clone()
        } else {
            friendly
        }
    } else if !account.is_empty() && !entry.title.contains(&account) {
        account.clone()
    } else {
        String::new()
    };
    if !subtitle.is_empty() {
        if !workspace.is_empty() {
            subtitle = format!("{subtitle} ({})", tail(workspace, 8));
        } else if !context.is_empty() {
            subtitle = format!("{subtitle} ({context})");
        }
    }
    Account {
        title: if generated {
            provider_name(id, translate)
        } else {
            entry.title.clone()
        },
        subtitle,
        account: if generated {
            if account.is_empty() {
                provider
            } else {
                account
            }
        } else {
            entry.title.clone()
        },
        scope,
    }
}
pub fn used(window: &SubscriptionWindow) -> Option<f64> {
    window
        .used_percent
        .filter(|v| v.is_finite() && *v >= 0.)
        .map(|v| v.min(100.))
}
pub fn window_label(window: &SubscriptionWindow, translate: &impl Fn(&str) -> String) -> String {
    let raw = window.label.as_str();
    let lower = raw.to_ascii_lowercase();
    if matches!(raw, "Membership" | "Allowance") {
        return translate(if raw == "Membership" {
            "subscriptionUsage.membership"
        } else {
            "subscriptionUsage.allowance"
        });
    }
    let weekly = window.window_minutes == Some(10080)
        || matches!(
            lower.as_str(),
            "weekly" | "week" | "7 days" | "7-day" | "seven_day"
        );
    let five = window.window_minutes == Some(300)
        || matches!(lower.as_str(), "5 hour" | "5 hours" | "5h" | "five_hour");
    let period = if matches!(lower.as_str(), "monthly" | "month") {
        translate("subscriptionUsage.monthly")
    } else if weekly {
        translate("subscriptionUsage.weekly")
    } else if five {
        translate("subscriptionUsage.fiveHour")
    } else if window.window_minutes == Some(1440) {
        translate("subscriptionUsage.daily")
    } else if let Some(minutes) = window.window_minutes.filter(|m| *m > 0 && m % 60 == 0) {
        translate("subscriptionUsage.hours").replace("{hours}", &(minutes / 60).to_string())
    } else {
        return raw.into();
    };
    if let Some((scope, kind)) = raw
        .rsplit_once(" · ")
        .filter(|(_, kind)| matches!(kind.to_ascii_lowercase().as_str(), "primary" | "secondary"))
    {
        let _ = kind;
        return if scope.eq_ignore_ascii_case("codex") {
            period
        } else {
            format!("{scope} ({period})")
        };
    }
    if matches!(
        lower.as_str(),
        "monthly"
            | "month"
            | "weekly"
            | "week"
            | "7 days"
            | "7-day"
            | "seven_day"
            | "5 hour"
            | "5 hours"
            | "5h"
            | "five_hour"
            | "daily"
            | "24 hour"
            | "24 hours"
    ) {
        period
    } else {
        format!("{period} ({raw})")
    }
}
pub fn console_only(snapshot: &SubscriptionSnapshot) -> bool {
    // This typed snapshot always carries the explicit flag. A provider-name
    // heuristic must not override false when quota fetching becomes supported.
    snapshot.quota_unavailable
}
pub fn reconnect(error: &str) -> bool {
    let e = error.to_ascii_lowercase();
    (e.contains("account") && e.contains("chang"))
        || [
            "ownership",
            "reconnect",
            "reauth",
            "needs_login",
            "needs login",
            "login required",
            "login_required",
            "unauthorized",
        ]
        .iter()
        .any(|s| e.contains(s))
        || (e.contains("expired") && (e.contains("token") || e.contains("credential")))
}
pub fn date(value: &str, chinese: bool) -> Option<String> {
    let date =
        time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).ok()?;
    let timestamp: libc::time_t = date.unix_timestamp();
    // libc converts using the user's current local timezone, like toLocaleString.
    let mut local = std::mem::MaybeUninit::<libc::tm>::uninit();
    #[cfg(unix)]
    let valid = unsafe { !libc::localtime_r(&timestamp, local.as_mut_ptr()).is_null() };
    #[cfg(windows)]
    let valid = unsafe { libc::localtime_s(local.as_mut_ptr(), &timestamp) == 0 };
    if !valid {
        return None;
    }
    let local = unsafe { local.assume_init() };
    Some(if chinese {
        format!(
            "{}月{}日 {:02}:{:02}",
            local.tm_mon + 1,
            local.tm_mday,
            local.tm_hour,
            local.tm_min
        )
    } else {
        let months = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        format!(
            "{} {} at {:02}:{:02} {}",
            months[local.tm_mon as usize],
            local.tm_mday,
            if local.tm_hour % 12 == 0 {
                12
            } else {
                local.tm_hour % 12
            },
            local.tm_min,
            if local.tm_hour < 12 { "AM" } else { "PM" }
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_and_invalid_usage_never_become_zero() {
        let mut window = SubscriptionWindow {
            id: "quota".into(),
            label: "Weekly".into(),
            used_percent: None,
            resets_at: None,
            window_minutes: Some(10080),
            source: None,
        };
        for value in [None, Some(f64::NAN), Some(f64::INFINITY), Some(-1.)] {
            window.used_percent = value;
            assert_eq!(used(&window), None);
        }
        window.used_percent = Some(0.);
        assert_eq!(used(&window), Some(0.));
        window.used_percent = Some(123.);
        assert_eq!(used(&window), Some(100.));
    }
    #[test]
    fn explicit_quota_support_overrides_historical_provider_fallback() {
        let mut snapshot = SubscriptionSnapshot {
            source: "community:bailian-coding-plan".into(),
            ..Default::default()
        };
        assert!(!console_only(&snapshot));
        snapshot.quota_unavailable = true;
        assert!(console_only(&snapshot));
    }
    #[test]
    fn friendly_account_label_preserves_original_matching_identity() {
        let mut entry = crate::catalog::Catalog::fixture().active.remove(0);
        entry.provider_id = Some("codex".into());
        entry.title = "Codex · alice@example.invalid".into();
        entry.account_identity = Some("alice@example.invalid:workspace-12345678".into());
        let shown = account(&entry, &|key| key.into());
        assert_eq!(shown.title, "Codex");
        assert_eq!(shown.account, "alice@example.invalid");
        assert_eq!(shown.scope, "workspace-12345678");
        assert_eq!(shown.subtitle, "alice@example.invalid (12345678)");
        assert_eq!(
            entry.account_identity.as_deref(),
            Some("alice@example.invalid:workspace-12345678")
        );
    }
}
