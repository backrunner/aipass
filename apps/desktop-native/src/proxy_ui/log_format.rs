//! Logfmt syntax stays presentation-only; the copied text is never HTML.
use super::*;
use gpui::{FontWeight, HighlightStyle};
use std::ops::Range;
#[derive(Clone, Copy, Debug, PartialEq)]
enum Tone {
    Muted,
    Info,
    Success,
    Warning,
    Danger,
    Text,
}
fn level(level: &str) -> Tone {
    match level.to_ascii_lowercase().as_str() {
        "error" | "fatal" => Tone::Danger,
        "warn" | "warning" => Tone::Warning,
        "info" => Tone::Info,
        _ => Tone::Muted,
    }
}
fn tone(key: &str, value: &str, severity: &str) -> Tone {
    if matches!(key, "status" | "http_status") {
        let number = value
            .strip_prefix("Some(")
            .and_then(|v| v.strip_suffix(')'))
            .unwrap_or(value);
        if let Ok(code) = number.parse::<u16>() {
            return match code {
                500..=599 => Tone::Danger,
                300..=499 => Tone::Warning,
                200..=299 => Tone::Success,
                100..=199 => Tone::Info,
                _ => Tone::Text,
            };
        }
    }
    if matches!(key, "event" | "transport") {
        return Tone::Info;
    }
    if key.ends_with("_id") {
        return Tone::Muted;
    }
    if key == "outcome" {
        if value == "success" {
            return Tone::Success;
        }
        if value == "failure" {
            return Tone::Danger;
        }
    }
    if matches!(
        key,
        "error" | "error_type" | "error_code" | "message" | "detail" | "reason"
    ) {
        return match level(severity) {
            Tone::Danger => Tone::Danger,
            Tone::Warning => Tone::Warning,
            _ => Tone::Text,
        };
    }
    if value.parse::<f64>().is_ok()
        || matches!(value, "true" | "false")
        || value
            .strip_prefix("Some(")
            .and_then(|v| v.strip_suffix(')'))
            .is_some_and(|v| v.parse::<u64>().is_ok())
    {
        Tone::Info
    } else {
        Tone::Text
    }
}
fn color(tone: Tone, t: Theme) -> gpui::Hsla {
    match tone {
        Tone::Muted => t.tertiary,
        Tone::Info => t.accent,
        Tone::Success => t.success,
        Tone::Warning => t.warning,
        Tone::Danger => t.danger,
        Tone::Text => t.text,
    }
    .into()
}
/// Byte ranges always start/end at ASCII delimiters, preserving UTF-8 values.
fn fields(message: &str) -> Vec<(Range<usize>, Range<usize>)> {
    let bytes = message.as_bytes();
    let mut result = vec![];
    let mut i = 0;
    while i < bytes.len() {
        if i > 0 && !bytes[i - 1].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        if !(bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
            i += 1;
            continue;
        }
        i += 1;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || b"_.-".contains(&bytes[i])) {
            i += 1;
        }
        if i == bytes.len() || bytes[i] != b'=' {
            continue;
        }
        let key = start..i;
        i += 1;
        let start = i;
        if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
            let quote = bytes[i];
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(bytes.len());
                } else if bytes[i] == quote {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
        } else {
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
        }
        result.push((key, start..i));
    }
    result
}
pub(super) fn line(
    entry: &ProxyLogEntry,
    entries: &[aipass_vault::EntrySummary],
    config: Option<&ProxyConfig>,
    t: Theme,
) -> (String, Vec<(Range<usize>, HighlightStyle)>) {
    let parsed = fields(&entry.message);
    let value = |key: &str| {
        parsed
            .iter()
            .find(|(k, _)| &entry.message[k.clone()] == key)
            .map(|(_, v)| &entry.message[v.clone()])
    };
    let provider = value("provider_entry_id").or_else(|| value("provider_id"));
    let target = value("target_id")
        .and_then(|id| Uuid::parse_str(id).ok())
        .and_then(|id| {
            config.and_then(|c| {
                c.routes
                    .iter()
                    .flat_map(|r| &r.targets)
                    .find(|t| t.id == id)
            })
        });
    let provider = provider.map(|id| {
        entries
            .iter()
            .find(|e| e.id.to_string() == id)
            .map(|e| e.title.as_str())
            .unwrap_or(id)
    });
    let at = time::OffsetDateTime::from_unix_timestamp(entry.timestamp)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    let at = at.to_offset(time::UtcOffset::local_offset_at(at).unwrap_or(time::UtcOffset::UTC));
    let mut text = format!("{:02}:{:02}:{:02}", at.hour(), at.minute(), at.second());
    let mut highlights = vec![(
        0..text.len(),
        HighlightStyle {
            color: Some(t.tertiary.into()),
            ..Default::default()
        },
    )];
    text.push(' ');
    let begin = text.len();
    text.push_str(&format!("[{}]", entry.level.to_uppercase()));
    highlights.push((
        begin..text.len(),
        HighlightStyle {
            color: Some(color(level(&entry.level), t)),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        },
    ));
    for identity in provider.into_iter().chain(target.map(|t| t.label.as_str())) {
        text.push(' ');
        let begin = text.len();
        text.push('[');
        text.push_str(identity);
        text.push(']');
        highlights.push((
            begin..text.len(),
            HighlightStyle {
                color: Some(t.text.into()),
                font_weight: Some(FontWeight::SEMIBOLD),
                ..Default::default()
            },
        ));
    }
    text.push(' ');
    let base = text.len();
    text.push_str(&entry.message);
    text.push('\n');
    for (key, value) in parsed {
        highlights.push((
            base + key.start..base + key.end,
            HighlightStyle {
                color: Some(t.tertiary.into()),
                ..Default::default()
            },
        ));
        highlights.push((
            base + value.start..base + value.end,
            HighlightStyle {
                color: Some(color(
                    tone(
                        &entry.message[key],
                        &entry.message[value.clone()],
                        &entry.level,
                    ),
                    t,
                )),
                ..Default::default()
            },
        ));
    }
    (text, highlights)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quoted_error_does_not_inject_a_status_field_or_break_unicode() {
        let text = r#"event=request_failed error="错误 status=200 \"quoted\"" status=Some(503) outcome=failure"#;
        let parts = fields(text);
        let keys = parts
            .iter()
            .map(|(k, _)| &text[k.clone()])
            .collect::<Vec<_>>();
        assert_eq!(keys, vec!["event", "error", "status", "outcome"]);
        assert_eq!(tone("status", "Some(503)", "info"), Tone::Danger);
        assert_eq!(tone("error", "status=200", "error"), Tone::Danger);
        for (k, v) in parts {
            assert!(text.is_char_boundary(k.start) && text.is_char_boundary(v.end));
        }
    }
}
