use anyhow::{Context, Result};
use directories::ProjectDirs;
use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Mutex;
use time::{format_description::well_known::Rfc3339, macros::format_description, OffsetDateTime};

const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;
const RETAINED_LOG_FILES: usize = 5;

static LOG_LOCK: Mutex<()> = Mutex::new(());

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

    let line = format!(
        "{} [{level}] {}\n",
        OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "unknown-time".to_string()),
        sanitize_log_message(message)
    );
    let line_len = line.as_bytes().len() as u64;
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
