use crate::logging::{write_component_log, AGENT_LOG};
use crate::paths::cloud_sync_dir;
use crate::session::{shutdown_requested, AgentState, StoredSyncSettings};
use aipass_agent_protocol::{CloudSyncProvider, SyncMode};
use notify::{RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

const SYNC_WATCH_DEBOUNCE: Duration = Duration::from_secs(2);
const SYNC_WATCH_POLL: Duration = Duration::from_millis(250);

/// The folder a sync configuration syncs against, when the backend is a
/// local filesystem folder (explicit local folder, iCloud Drive, OneDrive).
/// WebDAV is not folder-based and never resolves here.
pub(crate) fn folder_sync_dir(settings: &StoredSyncSettings) -> Option<PathBuf> {
    match settings.mode {
        SyncMode::Local => settings.sync_folder.clone(),
        SyncMode::ICloud => cloud_sync_dir(CloudSyncProvider::ICloud).ok(),
        SyncMode::OneDrive => cloud_sync_dir(CloudSyncProvider::OneDrive).ok(),
        SyncMode::WebDav => None,
    }
}

/// Filesystem events arrive in bursts (one sync writes many objects), so a
/// change only becomes actionable once the directory has been quiet for the
/// debounce window.
pub(crate) struct Debounce {
    window: Duration,
    last_event: Option<Instant>,
}

impl Debounce {
    pub(crate) fn new(window: Duration) -> Self {
        Self {
            window,
            last_event: None,
        }
    }

    pub(crate) fn record_event(&mut self, now: Instant) {
        self.last_event = Some(now);
    }

    /// Fires once the window has elapsed since the latest event, then disarms
    /// until another event arrives.
    pub(crate) fn take_due(&mut self, now: Instant) -> bool {
        match self.last_event {
            Some(last) if now.duration_since(last) >= self.window => {
                self.last_event = None;
                true
            }
            _ => false,
        }
    }
}

pub struct SyncWatcher {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Drop for SyncWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Do not join: a sync triggered by the watcher may still be running
        // and the thread also observes the agent shutdown flag on its own.
        if let Some(handle) = self.handle.take() {
            drop(handle);
        }
    }
}

/// Start watching the sync folder of the current settings, replacing any
/// watcher from a previous configuration. Settings without a folder backend
/// (or an unresolvable cloud folder) leave no watcher behind.
pub(crate) fn restart_sync_watcher(state: &Arc<AgentState>, settings: &StoredSyncSettings) {
    let dir = folder_sync_dir(settings);
    let watcher = dir.and_then(|dir| spawn_sync_watcher(state.clone(), dir));
    match state.sync_watcher.lock() {
        Ok(mut slot) => *slot = watcher,
        Err(poisoned) => *poisoned.into_inner() = watcher,
    }
}

pub(crate) fn start_sync_watcher_for_current_settings(state: &Arc<AgentState>) {
    match crate::session::load_sync_settings(&state.vault_dir) {
        Ok(settings) => restart_sync_watcher(state, &settings),
        Err(err) => write_component_log(
            AGENT_LOG,
            "WARN",
            &format!("sync watcher not started: failed to load sync settings: {err}"),
        ),
    }
}

fn spawn_sync_watcher(state: Arc<AgentState>, dir: PathBuf) -> Option<SyncWatcher> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let watch_dir = dir.clone();
    let handle = thread::spawn(move || watch_loop(state, dir, stop_thread));
    write_component_log(
        AGENT_LOG,
        "INFO",
        &format!("watching sync folder {}", watch_dir.display()),
    );
    Some(SyncWatcher {
        stop,
        handle: Some(handle),
    })
}

fn watch_loop(state: Arc<AgentState>, dir: PathBuf, stop: Arc<AtomicBool>) {
    let (tx, rx) = mpsc::channel::<()>();
    let mut watcher = match notify::recommended_watcher(
        move |_event: notify::Result<notify::Event>| {
            let _ = tx.send(());
        },
    ) {
        Ok(watcher) => watcher,
        Err(err) => {
            write_component_log(
                AGENT_LOG,
                "WARN",
                &format!("sync watcher unavailable for {}: {err}", dir.display()),
            );
            return;
        }
    };
    if let Err(err) = watcher.watch(&dir, RecursiveMode::Recursive) {
        write_component_log(
            AGENT_LOG,
            "WARN",
            &format!("cannot watch sync folder {}: {err}", dir.display()),
        );
        return;
    }
    let mut debounce = Debounce::new(SYNC_WATCH_DEBOUNCE);
    loop {
        if stop.load(Ordering::Relaxed) || shutdown_requested(&state) {
            break;
        }
        match rx.recv_timeout(SYNC_WATCH_POLL) {
            Ok(()) => {
                debounce.record_event(Instant::now());
                while rx.try_recv().is_ok() {}
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if debounce.take_due(Instant::now()) {
            match crate::server::run_sync_local(&state, &dir) {
                Ok(report) => write_component_log(
                    AGENT_LOG,
                    "INFO",
                    &format!(
                        "sync watcher ran sync on {}: uploaded={} downloaded={} conflicts={}",
                        dir.display(),
                        report.uploaded,
                        report.downloaded,
                        report.conflicts
                    ),
                ),
                Err(err) => write_component_log(
                    AGENT_LOG,
                    "WARN",
                    &format!("sync watcher sync failed for {}: {}", dir.display(), err.message),
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounce_waits_for_quiet_window_and_fires_once() {
        let start = Instant::now();
        let mut debounce = Debounce::new(Duration::from_secs(2));

        assert!(!debounce.take_due(start), "no events, nothing due");

        debounce.record_event(start);
        assert!(!debounce.take_due(start + Duration::from_millis(500)));

        // A second event re-arms the window.
        debounce.record_event(start + Duration::from_secs(1));
        assert!(!debounce.take_due(start + Duration::from_millis(2500)));
        assert!(debounce.take_due(start + Duration::from_secs(3)));

        // After firing, the debounce stays disarmed until a new event.
        assert!(!debounce.take_due(start + Duration::from_secs(10)));
        debounce.record_event(start + Duration::from_secs(10));
        assert!(debounce.take_due(start + Duration::from_secs(12)));
    }

    #[test]
    fn folder_sync_dir_only_resolves_folder_backends() {
        let local = StoredSyncSettings {
            mode: SyncMode::Local,
            sync_folder: Some(PathBuf::from("/tmp/aipass-sync-watch-test")),
            ..StoredSyncSettings::default()
        };
        assert_eq!(
            folder_sync_dir(&local),
            Some(PathBuf::from("/tmp/aipass-sync-watch-test"))
        );

        let local_without_folder = StoredSyncSettings {
            mode: SyncMode::Local,
            sync_folder: None,
            ..StoredSyncSettings::default()
        };
        assert_eq!(folder_sync_dir(&local_without_folder), None);

        let webdav = StoredSyncSettings {
            mode: SyncMode::WebDav,
            webdav_url: Some("https://dav.example".to_string()),
            ..StoredSyncSettings::default()
        };
        assert_eq!(folder_sync_dir(&webdav), None);
    }
}
