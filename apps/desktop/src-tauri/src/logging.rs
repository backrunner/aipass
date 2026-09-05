use aipass_agent::logging::{install_panic_logger, try_write_component_log, DESKTOP_LOG};

pub(crate) fn init() {
    install_panic_logger(DESKTOP_LOG);
    let _ = log_event("desktop.startup.logging_initialized", &[]);
}

pub(crate) fn log_event(event: &str, fields: &[(&str, &str)]) -> Result<(), String> {
    try_write_component_log(DESKTOP_LOG, "INFO", &format_event(event, fields))
        .map_err(|_| "failed to write desktop log".to_string())
}

pub(crate) struct DesktopOperation {
    event: &'static str,
    id: uuid::Uuid,
    started: std::time::Instant,
    finished: bool,
}

impl DesktopOperation {
    pub(crate) fn start(event: &'static str) -> Self {
        let operation = Self {
            event,
            id: uuid::Uuid::new_v4(),
            started: std::time::Instant::now(),
            finished: false,
        };
        operation.write("started");
        operation
    }

    pub(crate) fn finish(mut self, success: bool) {
        self.finished = true;
        self.write(if success { "completed" } else { "failed" });
    }

    fn write(&self, outcome: &str) {
        let _ = log_event(
            self.event,
            &[
                ("operation_id", &self.id.to_string()),
                ("outcome", outcome),
                (
                    "elapsed_ms",
                    &self.started.elapsed().as_millis().to_string(),
                ),
            ],
        );
    }
}

impl Drop for DesktopOperation {
    fn drop(&mut self) {
        if !self.finished {
            self.write("interrupted");
        }
    }
}

pub(crate) async fn log_task<T, E>(
    event: &'static str,
    task: impl std::future::Future<Output = Result<T, E>>,
) -> Result<T, E> {
    let operation = DesktopOperation::start(event);
    let result = task.await;
    operation.finish(result.is_ok());
    result
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
