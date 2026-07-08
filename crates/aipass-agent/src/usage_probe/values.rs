use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const NEWAPI_QUOTA_UNITS_PER_USD: f64 = 500_000.0;

pub(super) fn data_object(value: &Value) -> &Value {
    value.get("data").unwrap_or(value)
}

pub(super) fn is_success_like(value: &Value) -> bool {
    for field in ["success", "code"] {
        if let Some(raw) = value.get(field).and_then(Value::as_bool) {
            return raw;
        }
    }
    true
}

pub(super) fn is_valid_like(value: &Value) -> bool {
    value
        .get("isValid")
        .or_else(|| value.get("is_valid"))
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

pub(super) fn response_message(value: &Value) -> Option<String> {
    string_field(value, "message")
        .or_else(|| string_field(value, "error"))
        .or_else(|| {
            value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

pub(super) fn string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(super) fn bool_field(value: &Value, field: &str) -> Option<bool> {
    value.get(field).and_then(Value::as_bool)
}

pub(super) fn number_field(value: &Value, field: &str) -> Option<f64> {
    value.get(field).and_then(number_value)
}

pub(super) fn expires_at(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if let Some(raw) = value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Ok(timestamp) = raw.parse::<i64>() {
            return unix_timestamp_to_rfc3339(timestamp);
        }
        return Some(raw.to_string());
    }
    value.as_i64().and_then(unix_timestamp_to_rfc3339)
}

pub(super) fn format_newapi_quota(value: f64) -> String {
    format_amount(value / NEWAPI_QUOTA_UNITS_PER_USD)
}

pub(super) fn format_amount(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    let mut formatted = format!("{value:.4}");
    while formatted.contains('.') && formatted.ends_with('0') {
        formatted.pop();
    }
    if formatted.ends_with('.') {
        formatted.pop();
    }
    if formatted == "-0" {
        "0".to_string()
    } else {
        formatted
    }
}

pub(super) fn is_wallet_plan_name(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("wallet") || value.contains("钱包")
}

pub(super) fn non_empty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn number_value(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|value| value as f64))
        .or_else(|| value.as_u64().map(|value| value as f64))
        .or_else(|| value.as_str().and_then(|value| value.parse::<f64>().ok()))
}

fn unix_timestamp_to_rfc3339(timestamp: i64) -> Option<String> {
    if timestamp <= 0 {
        return None;
    }
    OffsetDateTime::from_unix_timestamp(timestamp)
        .ok()
        .and_then(|value| value.format(&Rfc3339).ok())
}
