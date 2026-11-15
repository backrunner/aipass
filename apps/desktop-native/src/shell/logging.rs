use aipass_agent::logging::{install_panic_logger, try_write_component_log, DESKTOP_LOG};

pub fn init() {
    install_panic_logger(DESKTOP_LOG);
    let _ = log_event("desktop.startup.logging_initialized", &[]);
}

pub fn log_event(event: &str, fields: &[(&str, &str)]) -> Result<(), String> {
    try_write_component_log(DESKTOP_LOG, "INFO", &format_event(event, fields))
        .map_err(|_| "failed to write desktop log".to_string())
}

fn format_event(event: &str, fields: &[(&str, &str)]) -> String {
    let mut line = format!("event={}", sanitize(event));
    for (key, value) in fields {
        line.push(' ');
        line.push_str(&sanitize(key));
        line.push('=');
        // OS/network errors can contain URLs, credentials and file contents.
        line.push_str(&sanitize(if *key == "error" { "redacted" } else { value }));
    }
    line
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_whitespace() || ch.is_control() {
                '_'
            } else {
                ch
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_sanitized_event_without_raw_errors() {
        let line = format_event(
            "startup test",
            &[("reason", "line\nbreak"), ("error", "fake-secret")],
        );
        assert!(line.contains("event=startup_test"));
        assert!(line.contains("reason=line_break"));
        assert!(!line.contains("fake-secret"));
    }
}
