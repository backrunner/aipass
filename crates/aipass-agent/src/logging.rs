use anyhow::{Context, Result};
use directories::ProjectDirs;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use time::{format_description::well_known::Rfc3339, macros::format_description, OffsetDateTime};

const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;
const RETAINED_LOG_FILES: usize = 5;
const REPEAT_SUPPRESSION_WINDOW: Duration = Duration::from_secs(60);
const MAX_TRACKED_REPEATED_LOGS: usize = 1024;

static LOG_LOCK: Mutex<()> = Mutex::new(());
static REPEATED_LOGS: LazyLock<Mutex<HashMap<RepeatLogKey, RepeatLogState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub const AGENT_LOG: &str = "agent";
pub const NATIVE_HOST_LOG: &str = "native-host";

pub fn init_component_logging(component: &str) -> Result<PathBuf> {
    let path = component_log_path(component)?;
    prune_component_logs(component);
    write_component_log(component, "INFO", "logging initialized");
    Ok(path)
}

pub fn install_panic_logger(component: &'static str) {
    std::panic::set_hook(Box::new(move |info| {
        write_component_log(component, "ERROR", &format!("panic: {info}"));
    }));
}

pub fn write_component_log(component: &str, level: &str, message: &str) {
    let Ok(_guard) = LOG_LOCK.lock() else {
        return;
    };
    let Ok(path) = component_log_path(component) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
        #[cfg(unix)]
        let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
    }
    prune_component_logs_locked(component);

    let Some(message) = prepare_log_message(component, level, message) else {
        return;
    };
    let line = format!(
        "{} [{level}] {}\n",
        OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "unknown-time".to_string()),
        message
    );
    let line_len = line.len() as u64;
    let current_len = fs::metadata(&path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if current_len.saturating_add(line_len) > MAX_LOG_BYTES {
        return;
    }

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = file.write_all(line.as_bytes());
        #[cfg(unix)]
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct RepeatLogKey {
    component: String,
    level: String,
    message: String,
}

#[derive(Debug)]
struct RepeatLogState {
    last_written: Instant,
    suppressed: u64,
}

fn prepare_log_message(component: &str, level: &str, message: &str) -> Option<String> {
    prepare_log_message_at(component, level, message, Instant::now())
}

fn prepare_log_message_at(
    component: &str,
    level: &str,
    message: &str,
    now: Instant,
) -> Option<String> {
    let message = sanitize_log_message(message);
    let key = RepeatLogKey {
        component: component.to_string(),
        level: level.to_string(),
        message: message.clone(),
    };
    let Ok(mut repeated_logs) = REPEATED_LOGS.lock() else {
        return Some(message);
    };
    let Some(state) = repeated_logs.get_mut(&key) else {
        if repeated_logs.len() >= MAX_TRACKED_REPEATED_LOGS {
            return Some(message);
        }
        repeated_logs.insert(
            key,
            RepeatLogState {
                last_written: now,
                suppressed: 0,
            },
        );
        return Some(message);
    };
    if now.duration_since(state.last_written) < REPEAT_SUPPRESSION_WINDOW {
        state.suppressed = state.suppressed.saturating_add(1);
        return None;
    }
    let suppressed = state.suppressed;
    state.last_written = now;
    state.suppressed = 0;
    if suppressed == 0 {
        Some(message)
    } else {
        Some(format!("{message} (suppressed {suppressed} repeats)"))
    }
}

pub fn component_log_path(component: &str) -> Result<PathBuf> {
    Ok(log_dir()?.join(format!(
        "{}-{}.log",
        sanitize_component(component),
        current_date()
    )))
}

pub fn log_dir() -> Result<PathBuf> {
    let dirs =
        ProjectDirs::from("dev", "aipass", "desktop").context("cannot determine project dir")?;
    Ok(dirs.data_local_dir().join("logs"))
}

fn prune_component_logs(component: &str) {
    let Ok(_guard) = LOG_LOCK.lock() else {
        return;
    };
    prune_component_logs_locked(component);
}

fn prune_component_logs_locked(component: &str) {
    let Ok(dir) = log_dir() else {
        return;
    };
    let prefix = format!("{}-", sanitize_component(component));
    let mut files = fs::read_dir(&dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(|entry| entry.ok()))
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.starts_with(&prefix) && name.ends_with(".log"))
        })
        .collect::<Vec<_>>();
    files.sort();
    let remove_count = files.len().saturating_sub(RETAINED_LOG_FILES);
    for path in files.into_iter().take(remove_count) {
        let _ = fs::remove_file(path);
    }
}

fn current_date() -> String {
    OffsetDateTime::now_utc()
        .format(format_description!("[year]-[month]-[day]"))
        .unwrap_or_else(|_| "unknown-date".to_string())
}

fn sanitize_component(component: &str) -> String {
    component
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn sanitize_log_message(message: &str) -> String {
    message
        .chars()
        .map(|ch| match ch {
            '\n' | '\r' | '\t' => ' ',
            _ => ch,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_messages_are_suppressed_and_reported() {
        let start = Instant::now();
        assert_eq!(
            prepare_log_message_at(
                "native-host-repeat-test",
                "ERROR",
                "extension id is not allowed",
                start
            ),
            Some("extension id is not allowed".to_string())
        );
        assert_eq!(
            prepare_log_message_at(
                "native-host-repeat-test",
                "ERROR",
                "extension id is not allowed",
                start + Duration::from_secs(15)
            ),
            None
        );
        assert_eq!(
            prepare_log_message_at(
                "native-host-repeat-test",
                "ERROR",
                "extension id is not allowed",
                start + REPEAT_SUPPRESSION_WINDOW
            ),
            Some("extension id is not allowed (suppressed 1 repeats)".to_string())
        );
    }

    #[test]
    fn different_messages_are_not_suppressed() {
        let now = Instant::now();
        assert!(
            prepare_log_message_at("native-host-different-test", "ERROR", "first", now).is_some()
        );
        assert!(
            prepare_log_message_at("native-host-different-test", "ERROR", "second", now).is_some()
        );
    }
}
