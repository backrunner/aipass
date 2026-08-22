use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
const RETAINED_LOG_FILES: usize = 5;

static LOG_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn init() {
    let _ = log_event("desktop.startup.logging_initialized", &[]);
}

pub(crate) fn log_event(event: &str, fields: &[(&str, &str)]) -> Result<(), String> {
    let _guard = LOG_LOCK
        .lock()
        .map_err(|_| "desktop log lock is poisoned".to_string())?;
    let path = log_path()?;
    let parent = path
        .parent()
        .ok_or_else(|| "desktop log path has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    rotate_if_needed(&path).map_err(|err| err.to_string())?;

    let mut line = format!(
        "{} pid={} event={}",
        OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "unknown-time".to_string()),
        std::process::id(),
        sanitize(event),
    );
    for (key, value) in fields {
        line.push(' ');
        line.push_str(&sanitize(key));
        line.push('=');
        line.push_str(&sanitize(value));
    }
    line.push('\n');

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| err.to_string())?;
    file.write_all(line.as_bytes())
        .map_err(|err| err.to_string())?;
    Ok(())
}

fn log_path() -> Result<PathBuf, String> {
    if let Some(explicit) = std::env::var_os("AIPASS_LOG_DIR") {
        return Ok(PathBuf::from(explicit).join("desktop.log"));
    }

    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| "HOME is not set".to_string())?;
        Ok(home
            .join("Library")
            .join("Logs")
            .join("AIPass")
            .join("desktop.log"))
    }

    #[cfg(not(target_os = "macos"))]
    directories::ProjectDirs::from("dev", "aipass", "desktop")
        .map(|dirs| dirs.data_local_dir().join("logs").join("desktop.log"))
        .ok_or_else(|| "cannot determine desktop log directory".to_string())
}

fn rotate_if_needed(path: &std::path::Path) -> std::io::Result<()> {
    let oversized = fs::metadata(path)
        .map(|metadata| metadata.len() >= MAX_LOG_BYTES)
        .unwrap_or(false);
    if !oversized {
        return Ok(());
    }

    for index in (1..RETAINED_LOG_FILES).rev() {
        let source = path.with_extension(format!("log.{index}"));
        let destination = path.with_extension(format!("log.{}", index + 1));
        if destination.exists() {
            let _ = fs::remove_file(&destination);
        }
        if source.exists() {
            fs::rename(source, destination)?;
        }
    }
    let first = path.with_extension("log.1");
    if first.exists() {
        let _ = fs::remove_file(&first);
    }
    fs::rename(path, first)
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '\n' | '\r' | '\t' | ' ' => '_',
            _ => ch,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{log_event, rotate_if_needed, MAX_LOG_BYTES};
    use std::fs;

    #[test]
    fn rotates_oversized_log() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop.log");
        fs::write(&path, vec![b'x'; MAX_LOG_BYTES as usize]).unwrap();
        rotate_if_needed(&path).unwrap();
        assert!(!path.exists());
        assert!(path.with_extension("log.1").exists());
    }

    #[test]
    fn writes_sanitized_event() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("AIPASS_LOG_DIR", dir.path());
        log_event("startup test", &[("reason", "line\nbreak")]).unwrap();
        let output = fs::read_to_string(dir.path().join("desktop.log")).unwrap();
        assert!(output.contains("event=startup_test"));
        assert!(output.contains("reason=line_break"));
        std::env::remove_var("AIPASS_LOG_DIR");
    }
}
