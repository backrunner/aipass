//! Claude Code /usage text and reset-time normalization.
use super::*;

pub(super) fn parse_usage(
    text: &str,
    now: chrono::DateTime<Utc>,
) -> Result<Vec<SubscriptionWindow>, String> {
    let ansi = regex::Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").unwrap();
    let text = ansi.replace_all(text, "");
    let denied = regex::Regex::new(r"(?i)\b(401|403)\b|not (logged|signed) in|signed out|sign-in (has )?expired|unauthorized|forbidden|authentication (failed|required)|invalid (access )?token|hit your limit|using your overages").unwrap();
    if denied.is_match(&text) {
        return Err("Claude Code /usage requires sign-in or reports an account limit".into());
    }
    let line = regex::Regex::new(
        r"^Current (session|week(?: \(([^)]+)\))?):\s*([0-9.]+)% used(?:\s*·\s*resets (.+))?$",
    )
    .unwrap();
    let mut windows = Vec::new();
    for text in text.lines().map(str::trim) {
        let Some(c) = line.captures(text) else {
            continue;
        };
        let percent: f64 = c[3]
            .parse()
            .map_err(|_| "Claude Code /usage returned an invalid percentage")?;
        if !percent.is_finite() || !(0.0..=100.0).contains(&percent) {
            return Err("Claude Code /usage returned an invalid percentage".into());
        }
        let weekly = &c[1] != "session";
        let scope = c
            .get(2)
            .map(|m| m.as_str())
            .filter(|s| !s.eq_ignore_ascii_case("all models"));
        let id = if weekly {
            scope
                .map(|s| format!("seven_day_{}", s.to_lowercase()))
                .unwrap_or_else(|| "seven_day".into())
        } else {
            "five_hour".into()
        };
        let label = if weekly {
            scope
                .map(|s| format!("7d {s}"))
                .unwrap_or_else(|| "7d".into())
        } else {
            "5h".into()
        };
        let minutes = if weekly { 10080 } else { 300 };
        let resets_at = c
            .get(4)
            .map(|m| reset_time(m.as_str(), now, minutes))
            .transpose()?;
        if windows.iter().any(|w: &SubscriptionWindow| w.id == id) {
            return Err("Claude Code /usage returned duplicate windows".into());
        }
        windows.push(SubscriptionWindow {
            id,
            label,
            used_percent: Some(percent),
            resets_at,
            window_minutes: Some(minutes.into()),
            source: Some("claude-code-usage".into()),
        });
    }
    if windows.is_empty() {
        return Err("Claude Code /usage is unavailable or this output format is unsupported; update Claude Code and retry".into());
    }
    Ok(windows)
}

pub(super) fn reset_time(
    text: &str,
    now: chrono::DateTime<Utc>,
    minutes: u32,
) -> Result<String, String> {
    let (date, zone) = text
        .trim_end_matches(')')
        .rsplit_once(" (")
        .ok_or("Claude Code /usage reset has no timezone")?;
    let zone: chrono_tz::Tz = zone
        .parse()
        .map_err(|_| "Claude Code /usage returned an unknown timezone")?;
    let date = date.replace("AM", "am").replace("PM", "pm");
    let hours = regex::Regex::new(r"(^|[^:0-9])(\d{1,2})(am|pm)\b").unwrap();
    let date = hours.replace_all(&date, "${1}${2}:00${3}");
    let local = now.with_timezone(&zone);
    let mut values = Vec::new();
    for fmt in [
        "%b %e, %Y at %I:%M%P",
        "%b %e, %Y at %I%P",
        "%b %e, %Y, %I:%M%P",
        "%b %e, %Y, %I%P",
    ] {
        if let Ok(value) = chrono::NaiveDateTime::parse_from_str(&date, fmt) {
            values.push(value);
        }
    }
    for year in (local.year() - 1)..=(local.year() + 1) {
        for fmt in [
            "%Y %b %e at %I:%M%P",
            "%Y %b %e at %I%P",
            "%Y %b %e, %I:%M%P",
            "%Y %b %e, %I%P",
        ] {
            if let Ok(value) = chrono::NaiveDateTime::parse_from_str(&format!("{year} {date}"), fmt)
            {
                values.push(value);
            }
        }
    }
    for fmt in ["%I:%M%P", "%I%P"] {
        if let Ok(time) = chrono::NaiveTime::parse_from_str(&date, fmt) {
            for day in -1..=1 {
                values.push((local.date_naive() + chrono::Duration::days(day)).and_time(time));
            }
        }
    }
    let earliest = now - chrono::Duration::days(1);
    let latest = now + chrono::Duration::minutes(i64::from(minutes)) + chrono::Duration::hours(2);
    let mut instants = Vec::new();
    for value in values {
        let resolved = zone.from_local_datetime(&value);
        for value in [resolved.earliest(), resolved.latest()]
            .into_iter()
            .flatten()
        {
            let value = value.with_timezone(&Utc);
            if value >= earliest && value <= latest {
                instants.push(value);
            }
        }
    }
    instants.sort();
    instants.dedup();
    instants
        .iter()
        .find(|v| **v >= now)
        .or_else(|| instants.last())
        .map(|v| v.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .ok_or_else(|| "Claude Code /usage returned an invalid reset date".into())
}
