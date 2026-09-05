use anyhow::{Context, Result};
use std::cell::Cell;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;
const RETAINED_LOG_FILES: usize = 10;
const MAX_MESSAGE_CHARS: usize = 16_384;
const REPEAT_SUPPRESSION_WINDOW: Duration = Duration::from_secs(60);
const MAX_TRACKED_REPEATED_LOGS: usize = 1024;

static LOG_LOCK: Mutex<()> = Mutex::new(());
static REPEATED_LOGS: LazyLock<Mutex<HashMap<RepeatLogKey, RepeatLogState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub const AGENT_LOG: &str = "agent";
pub const NATIVE_HOST_LOG: &str = "native-host";
pub const DESKTOP_LOG: &str = "desktop";
pub const CLIENT_LOG: &str = "client";

thread_local! {
    static REQUEST_ID: Cell<Option<uuid::Uuid>> = const { Cell::new(None) };
    #[cfg(test)]
    static TEST_LOG_DIR: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn with_test_log_dir<T>(dir: &Path, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<PathBuf>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_LOG_DIR.replace(self.0.take());
        }
    }
    let _restore = Restore(TEST_LOG_DIR.replace(Some(dir.to_path_buf())));
    run()
}

/// Synchronous request scope; the marker prevents moving it to another thread.
pub struct RequestScope {
    previous: Option<uuid::Uuid>,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl RequestScope {
    pub fn new(id: uuid::Uuid) -> Self {
        Self {
            previous: REQUEST_ID.replace(Some(id)),
            _thread: std::marker::PhantomData,
        }
    }
}

pub(crate) fn current_request_id() -> Option<uuid::Uuid> {
    REQUEST_ID.get()
}

impl Drop for RequestScope {
    fn drop(&mut self) {
        REQUEST_ID.set(self.previous);
    }
}

pub fn init_component_logging(component: &str) -> Result<PathBuf> {
    let path = component_log_path(component)?;
    try_write_component_log(component, "INFO", "logging initialized")?;
    Ok(path)
}

pub fn install_panic_logger(component: &'static str) {
    std::panic::set_hook(Box::new(move |info| {
        // Panic payloads can contain request bodies or decrypted configuration.
        let location = info
            .location()
            .map(|at| format!("{}:{}", at.file(), at.line()));
        write_component_log(
            component,
            "ERROR",
            &format!(
                "panic location={}",
                location.as_deref().unwrap_or("unknown")
            ),
        );
    }));
}

pub fn write_component_log(component: &str, level: &str, message: &str) {
    if try_write_component_log(component, level, message).is_err() {
        // Do not echo the original message or an OS error containing user paths.
        if prepare_log_message("logging", "ERROR", "local log write failed").is_some() {
            eprintln!("AIPass: local log write failed");
        }
    }
}

pub fn try_write_component_log(component: &str, level: &str, message: &str) -> Result<()> {
    let _guard = LOG_LOCK.lock().unwrap_or_else(|err| err.into_inner());
    let path = component_log_path(component)?;
    let original_message = message;
    // Operation scopes are never deduplicated: identical actions still represent
    // separate user operations, and start/completion records must remain paired.
    let message = if current_request_id().is_some() {
        Some(sanitize_log_message(message))
    } else {
        prepare_log_message(component, level, message)
    };
    let Some(message) = message else {
        return Ok(());
    };
    let context = REQUEST_ID
        .get()
        .map(|id| format!("request_id={id} "))
        .unwrap_or_default();
    let line = format!(
        "{} [{}] pid={} {}\n",
        OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "unknown-time".to_string()),
        sanitize_component(level),
        std::process::id(),
        format_args!("{context}{message}")
    );
    if let Err(err) = append_rotating(&path, line.as_bytes(), MAX_LOG_BYTES, RETAINED_LOG_FILES) {
        // A failed write must not suppress its retry or make initialization
        // appear successful while the log directory is still unavailable.
        REPEATED_LOGS
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .remove(&RepeatLogKey {
                component: component.to_string(),
                level: level.to_string(),
                message: sanitize_log_message(original_message),
            });
        return Err(err.into());
    }
    Ok(())
}

fn private_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true).read(true);
    #[cfg(unix)]
    options.mode(0o600);
    let file = options.open(path)?;
    #[cfg(unix)]
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

fn append_rotating(
    path: &Path,
    line: &[u8],
    max_bytes: u64,
    retained: usize,
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }
    // Native hosts and agent clients can write from multiple processes.
    // Lock a separate inode so rotation cannot invalidate the lock.
    let lock = private_file(&path.with_extension("lock"))?;
    lock.lock()?;
    let mut file = private_file(path)?;
    if file.metadata()?.len().saturating_add(line.len() as u64) > max_bytes {
        drop(file);
        for index in (1..=retained).rev() {
            let source = if index == 1 {
                path.to_path_buf()
            } else {
                path.with_extension(format!("log.{}", index - 1))
            };
            let destination = path.with_extension(format!("log.{index}"));
            if destination.exists() {
                fs::remove_file(&destination)?;
            }
            if source.exists() {
                fs::rename(source, destination)?;
            }
        }
        file = private_file(path)?;
    }
    file.write_all(line)?;
    Ok(())
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
            repeated_logs.retain(|_, state| {
                now.saturating_duration_since(state.last_written) < REPEAT_SUPPRESSION_WINDOW
            });
            if repeated_logs.len() >= MAX_TRACKED_REPEATED_LOGS {
                return Some(message);
            }
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
    Ok(log_dir()?.join(format!("{}.log", sanitize_component(component))))
}

pub fn log_dir() -> Result<PathBuf> {
    #[cfg(test)]
    if let Some(dir) = TEST_LOG_DIR.with(|dir| dir.borrow().clone()) {
        return Ok(dir);
    }
    if let Some(explicit) = std::env::var_os("AIPASS_LOG_DIR") {
        return Ok(PathBuf::from(explicit));
    }
    #[cfg(target_os = "macos")]
    {
        let home = directories::BaseDirs::new().context("cannot determine home directory")?;
        Ok(home.home_dir().join("Library/Logs/AIPass"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let dirs = directories::ProjectDirs::from("dev", "aipass", "desktop")
            .context("cannot determine project dir")?;
        Ok(dirs.data_local_dir().join("logs"))
    }
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
    let sanitized = message
        .chars()
        .take(MAX_MESSAGE_CHARS)
        .map(|ch| match ch {
            ch if ch.is_control() => ' ',
            _ => ch,
        })
        .collect::<String>();
    redact_sensitive_fields(&sanitized)
}

fn redact_sensitive_fields(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    // Once a sensitive field appears, its value may include whitespace or
    // punctuation. Drop the remainder rather than guessing where it ends.
    let sensitive = [
        "api_key=",
        "access_token=",
        "refresh_token=",
        "password=",
        "secret=",
        "authorization=",
        "authorization:",
        "bearer ",
        "x-api-key=",
    ]
    .iter()
    .filter_map(|prefix| lower.find(prefix).map(|index| (index, prefix)))
    .min_by_key(|(index, _)| *index);
    if let Some((index, prefix)) = sensitive {
        format!("{}{}<redacted>", &message[..index], prefix)
    } else {
        message.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_writes_remain_visible_and_resume_when_storage_recovers() {
        let dir = tempfile::tempdir().unwrap();
        let blocked = dir.path().join("blocked");
        fs::write(&blocked, "not a directory").unwrap();
        with_test_log_dir(&blocked, || {
            for _ in 0..2 {
                assert!(
                    try_write_component_log("recovery-test", "INFO", "event=test.recovery")
                        .is_err()
                );
            }
            fs::remove_file(&blocked).unwrap();
            try_write_component_log("recovery-test", "INFO", "event=test.recovery").unwrap();
            assert!(fs::read_to_string(blocked.join("recovery-test.log"))
                .unwrap()
                .contains("event=test.recovery"));
        });
    }

    #[test]
    fn rotation_keeps_writing_and_retains_only_bounded_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent.log");
        for index in 0..20 {
            append_rotating(&path, format!("{index:04}\n").as_bytes(), 10, 2).unwrap();
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), "0018\n0019\n");
        assert_eq!(
            fs::read_to_string(path.with_extension("log.2")).unwrap(),
            "0014\n0015\n"
        );
        assert!(!path.with_extension("log.3").exists());
        #[cfg(unix)]
        {
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
    }

    #[test]
    fn concurrent_writers_preserve_complete_lines_during_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shared.log");
        let workers = (0..4)
            .map(|worker| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for index in 0..100 {
                        append_rotating(
                            &path,
                            format!("{worker}:{index:03}\n").as_bytes(),
                            120,
                            30,
                        )
                        .unwrap();
                    }
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
        let mut lines = std::collections::HashSet::new();
        for entry in fs::read_dir(dir.path()).unwrap() {
            let path = entry.unwrap().path();
            if path
                .extension()
                .is_some_and(|extension| extension == "lock")
            {
                continue;
            }
            for line in fs::read_to_string(path).unwrap().lines() {
                assert!(lines.insert(line.to_string()));
            }
        }
        assert_eq!(lines.len(), 400);
    }

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

    #[test]
    fn sensitive_fields_are_redacted_before_persistence() {
        assert_eq!(
            sanitize_log_message("provider api_key=sk-test access_token=secret"),
            "provider api_key=<redacted>"
        );
        for message in [
            "request Authorization: Bearer private-token",
            "request bearer private-token",
            "password= private words,still-private",
        ] {
            assert!(!sanitize_log_message(message).contains("private"));
        }
    }
}
