//! Bounded error diagnostics. Only error fields are retained from JSON;
//! credentials and headers are redacted before any text reaches disk.
use super::*;
use serde_json::Value;

const BODY_LIMIT: usize = 16 * 1024;
const DETAIL_LIMIT: usize = 2048;

pub(crate) async fn read_error(
    response: reqwest::Response,
    target: &ResolvedTarget,
    local_tokens: &[&str],
) -> String {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    let result = tokio::time::timeout(Duration::from_secs(1), async {
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| ())?;
            let remaining = BODY_LIMIT - body.len();
            body.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
            if body.len() == BODY_LIMIT {
                break;
            }
        }
        Ok::<_, ()>(())
    })
    .await;
    let detail = if body.is_empty() {
        if matches!(result, Ok(Ok(()))) {
            "empty upstream error response".into()
        } else {
            "upstream error body unavailable (read failed or timed out)".into()
        }
    } else {
        error_detail(&body, target, local_tokens)
    };
    body.zeroize();
    detail
}

pub(crate) fn error_detail(bytes: &[u8], target: &ResolvedTarget, local_tokens: &[&str]) -> String {
    let bytes = &bytes[..bytes.len().min(BODY_LIMIT)];
    let raw = match serde_json::from_slice::<Value>(bytes) {
        Ok(value) => error_fields(&value),
        // Never treat a truncated JSON payload as plain text: unrelated
        // input/output fields may precede the error message.
        Err(_)
            if matches!(
                bytes.iter().find(|byte| !byte.is_ascii_whitespace()),
                Some(b'{' | b'[')
            ) =>
        {
            "malformed or oversized upstream JSON error response".into()
        }
        Err(_) => String::from_utf8_lossy(bytes).into_owned(),
    };
    sanitize(raw, target, local_tokens, bytes.len() >= BODY_LIMIT)
}

fn error_fields(value: &Value) -> String {
    let error = value
        .pointer("/response/error")
        .filter(|v| !v.is_null())
        .or_else(|| value.get("error").filter(|v| !v.is_null()))
        .unwrap_or(value);
    if let Some(message) = error.as_str() {
        return message.to_owned();
    }
    ["code", "type", "message", "detail", "error_description"]
        .iter()
        .filter_map(|key| {
            let value = error.get(*key).or_else(|| value.get(*key))?;
            let text = match value {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => return None,
            };
            Some(format!("{key}: {text}"))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn sanitize(
    mut raw: String,
    target: &ResolvedTarget,
    local_tokens: &[&str],
    truncated: bool,
) -> String {
    for secret in std::iter::once(target.api_key.as_str())
        .chain(local_tokens.iter().copied())
        .chain(
            target
                .config
                .headers
                .iter()
                .map(|(_, value)| value.as_str()),
        )
    {
        if !secret.is_empty() {
            raw = raw.replace(secret, "[redacted]");
        }
    }
    // Providers sometimes include supplied credentials or URLs in free-text
    // messages. Retain explanatory text before sensitive fields, never their values.
    let lower = raw.to_ascii_lowercase();
    let sensitive = [
        "authorization",
        "bearer ",
        "api_key:",
        "api_key=",
        "api key:",
        "api-key:",
        "api-key=",
        "apikey:",
        "apikey=",
        "access_token:",
        "access_token=",
        "refresh_token:",
        "refresh_token=",
        "password",
        "secret=",
        "secret:",
    ]
    .iter()
    .filter_map(|marker| lower.find(marker))
    .min();
    if let Some(index) = sensitive {
        raw.truncate(index);
        raw.push_str("[redacted credential]");
    }
    let detail = raw
        .split_whitespace()
        .map(|word| {
            if word.contains("://") {
                "[redacted URL]"
            } else if word.contains("sk-") || word.contains("sk_") || word.contains("eyJ") {
                "[redacted credential]"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    raw.zeroize();
    let mut chars = detail.chars().filter(|ch| !ch.is_control());
    let mut limited: String = chars.by_ref().take(DETAIL_LIMIT).collect();
    if chars.next().is_some() || truncated {
        limited.push_str(" … [truncated]");
    }
    if limited.is_empty() {
        "upstream error response contains no message".into()
    } else {
        limited
    }
}

impl UsageStore {
    pub(crate) fn log_upstream_error(
        &self,
        request_id: Uuid,
        route_id: Uuid,
        target: &ProxyTargetConfig,
        status: StatusCode,
        transport: &'static str,
        detail: &str,
    ) {
        self.log_diagnostic("error", format!(
            "event=proxy.upstream.rejected request_id={request_id} route_id={route_id} target_id={} provider_id={} transport={transport} status={} error={detail:?}",
            target.id, target.provider_entry_id, status.as_u16(),
        ));
    }
}

/// Observe errors before either prefetch or forwarding consumes the stream.
/// Keep at most one small event; oversized events are skipped through their
/// boundary so their tail can never be mistaken for a new error event.
#[derive(Default)]
pub(crate) struct ErrorEvents {
    buffer: Vec<u8>,
    newline: bool,
    oversized: bool,
    reported: bool,
}

impl ErrorEvents {
    pub(crate) fn observe(&mut self, chunk: &[u8]) -> Option<Value> {
        if self.reported {
            return None;
        }
        for &byte in chunk {
            if self.buffer.len() < BODY_LIMIT {
                self.buffer.push(byte);
            } else {
                self.oversized = true;
            }
            if byte == b'\r' {
                continue;
            }
            let boundary = byte == b'\n' && self.newline;
            self.newline = byte == b'\n';
            if boundary {
                let error = if !self.oversized && sse_event_reports_error(&self.buffer) {
                    sse_event_data(&self.buffer).and_then(|mut data| {
                        let value = serde_json::from_slice(&data).ok();
                        data.zeroize();
                        value
                    })
                } else {
                    None
                };
                self.buffer.zeroize();
                self.buffer.clear();
                self.oversized = false;
                self.newline = false;
                if error.is_some() {
                    self.reported = true;
                    return error;
                }
            }
        }
        None
    }
}

impl Drop for ErrorEvents {
    fn drop(&mut self) {
        self.buffer.zeroize();
    }
}

/// Resolve the live credential only while sanitizing an error; stream trackers
/// deliberately keep their own credential copies zeroized.
pub(crate) fn log_wire_error(
    state: &RuntimeState,
    request_id: Uuid,
    route_id: Uuid,
    target_id: Uuid,
    status: StatusCode,
    transport: &'static str,
    value: &Value,
) {
    let Ok(config) = state.config.read() else {
        return;
    };
    let Some(route) = config
        .routes
        .iter()
        .find(|route| route.config.id == route_id)
    else {
        return;
    };
    let Some(target) = route
        .targets
        .iter()
        .find(|target| target.config.id == target_id)
    else {
        return;
    };
    let detail = sanitize(error_fields(value), target, &[&route.local_token], false);
    state.usage.log_upstream_error(
        request_id,
        route_id,
        &target.config,
        status,
        transport,
        &detail,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sse_errors_survive_chunk_boundaries_and_skip_oversized_events() {
        let mut observer = ErrorEvents::default();
        assert!(observer
            .observe(format!("data: {}", "x".repeat(BODY_LIMIT * 2)).as_bytes())
            .is_none());
        assert!(observer
            .observe(b"\r\n\r\nevent: error\r\ndata: {\"error\":{\"message\":\"balance")
            .is_none());
        let value = observer.observe(b" exhausted\"}}\r\n\r\n").unwrap();
        assert_eq!(value["error"]["message"], "balance exhausted");
        assert!(observer
            .observe(b"data: {\"error\":\"duplicate\"}\n\n")
            .is_none());
    }

    #[test]
    fn error_details_keep_explanations_but_redact_secrets_and_unrelated_payloads() {
        let mut target = crate::tests::test_target("https://example.test".into(), 0);
        target.api_key = "private-key-value".into();
        target
            .config
            .headers
            .push(("x-private".into(), "private-header-value".into()));
        let body = br#"{"error":{"code":"quota_exhausted","message":"Balance too low: private-key-value private-header-value local-secret"},"input":"private prompt","access_token":"unrelated-secret"}"#;
        let detail = error_detail(body, &target, &["local-secret"]);
        assert!(detail.contains("quota_exhausted"));
        assert!(detail.contains("Balance too low"));
        for secret in [
            "private-key-value",
            "private-header-value",
            "local-secret",
            "private prompt",
            "unrelated-secret",
        ] {
            assert!(!detail.contains(secret));
        }
        assert!(
            !error_detail(b"Rejected bearer unexpected-secret", &target, &[])
                .contains("unexpected-secret")
        );
        assert!(
            !error_detail(b"Failure https://private:password@host/path", &target, &[])
                .contains("password@host")
        );
        assert!(
            !error_detail(b"Invalid key sk-unknown-secret", &target, &[]).contains("sk-unknown")
        );
        assert!(error_detail(
            br#"{"error":{"code":"invalid_api_key","message":"Key expired"}}"#,
            &target,
            &[]
        )
        .contains("invalid_api_key"));
        assert!(
            !error_detail(br#"{"input":"private prompt","error":"#, &target, &[])
                .contains("private prompt")
        );
        let long = "额度".repeat(5000);
        let bounded = error_detail(long.as_bytes(), &target, &[]);
        assert!(bounded.chars().count() < 2100);
        assert!(bounded.ends_with("[truncated]"));
    }
}
