use crate::logging::{write_component_log, AGENT_LOG};
use crate::paths::cloud_sync_dir;
use crate::session::{shutdown_requested, AgentState, StoredSyncSettings};
use aipass_agent_protocol::{AgentErrorCode, CloudSyncProvider, SyncMode};
use aipass_sync::SyncStatus;
use notify::{RecursiveMode, Watcher};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

const SYNC_WATCH_DEBOUNCE: Duration = Duration::from_millis(350);
const SYNC_WATCH_POLL: Duration = Duration::from_millis(250);

/// The folder a sync configuration syncs against, when the backend is a
/// local filesystem folder (explicit folder or OneDrive).
/// CloudKit and WebDAV have independent remote-change detection.
pub(crate) fn folder_sync_dir(settings: &StoredSyncSettings) -> Option<PathBuf> {
    match settings.mode {
        SyncMode::Local => settings.sync_folder.clone(),
        SyncMode::ICloud => None,
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
    first_event: Option<Instant>,
}

impl Debounce {
    pub(crate) fn new(window: Duration) -> Self {
        Self {
            window,
            last_event: None,
            first_event: None,
        }
    }

    pub(crate) fn record_event(&mut self, now: Instant) {
        self.first_event.get_or_insert(now);
        self.last_event = Some(now);
    }

    /// Fires once the window has elapsed since the latest event, then disarms
    /// until another event arrives.
    pub(crate) fn take_due(&mut self, now: Instant) -> bool {
        match self.last_event {
            Some(last)
                if now.duration_since(last) >= self.window
                    || self.first_event.is_some_and(|first| {
                        now.duration_since(first) >= (self.window * 2).max(Duration::from_secs(2))
                    }) =>
            {
                self.last_event = None;
                self.first_event = None;
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

/// Watch local writes for every backend. Polling also catches WebDAV changes,
/// missed filesystem notifications, and cloud folders mounted after startup.
pub(crate) fn restart_sync_watcher(state: &Arc<AgentState>, settings: &StoredSyncSettings) {
    if let Ok(mut cached) = state.webdav_transport.lock() {
        *cached = None;
    }
    let enabled = settings.mode != SyncMode::Local || settings.sync_folder.is_some();
    let watcher = enabled.then(|| {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        let state = state.clone();
        let handle = thread::spawn(move || watch_loop(state, stop_thread));
        SyncWatcher {
            stop,
            handle: Some(handle),
        }
    });
    match state.sync_watcher.lock() {
        Ok(mut slot) => *slot = watcher,
        Err(poisoned) => *poisoned.into_inner() = watcher,
    }
}

pub(crate) fn start_sync_watcher_for_current_settings(state: &Arc<AgentState>) {
    if let Ok(settings) = crate::session::load_sync_settings(&state.vault_dir) {
        restart_sync_watcher(state, &settings);
    }
}

fn relevant_event(event: &notify::Event) -> bool {
    if !matches!(
        event.kind,
        notify::EventKind::Create(_) | notify::EventKind::Modify(_) | notify::EventKind::Remove(_)
    ) {
        return false;
    }
    event.paths.iter().any(|path| {
        (matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some(
                "aipmanifest"
                    | "aipobj"
                    | "aipdevice"
                    | "aipaudit"
                    | "aipgrant"
                    | "aipstate"
                    | "aipsnapshot"
            )
        ) || matches!(
            path.file_name().and_then(|name| name.to_str()),
            Some("objects" | "audit" | "devices" | "grants" | "snapshots")
        )) && !path.components().any(|part| {
            matches!(
                part.as_os_str().to_str(),
                Some("sync-cache" | "sync-state" | "sync-outbox")
            )
        })
    })
}

fn local_write_stamp(root: &std::path::Path) -> u64 {
    let mut paths = vec![
        root.join("manifest.aipmanifest"),
        root.join("server-config.aipstate"),
    ];
    for dir in ["objects", "audit", "devices", "grants"] {
        if let Ok(entries) = std::fs::read_dir(root.join(dir)) {
            paths.extend(entries.filter_map(|entry| entry.ok().map(|entry| entry.path())));
        }
    }
    paths.sort();
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    for path in paths {
        if let Ok(metadata) = path.metadata() {
            path.hash(&mut hash);
            metadata.len().hash(&mut hash);
            metadata.modified().ok().hash(&mut hash);
        }
    }
    hash.finish()
}

fn watch_loop(state: Arc<AgentState>, stop: Arc<AtomicBool>) {
    let (tx, rx) = mpsc::channel::<()>();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.as_ref().map(relevant_event).unwrap_or(true) {
            let _ = tx.send(());
        }
    })
    .ok();
    if let Some(watcher) = &mut watcher {
        let _ = watcher.watch(&state.vault_dir, RecursiveMode::Recursive);
    }
    let mut watched_remote = None;
    let mut debounce = Debounce::new(SYNC_WATCH_DEBOUNCE);
    let mut next_sync = Instant::now();
    let mut retry_after = Instant::now();
    let mut failures = 0u32;
    let mut mode = SyncMode::Local;
    let mut wake = state.sync_wake.load(Ordering::Relaxed);
    let mut last_local_check = Instant::now();
    let mut local_stamp = local_write_stamp(&state.vault_dir);
    loop {
        if stop.load(Ordering::Relaxed) || shutdown_requested(&state) {
            break;
        }
        if last_local_check.elapsed() >= Duration::from_secs(1) {
            let stamp = local_write_stamp(&state.vault_dir);
            if stamp != local_stamp {
                debounce.record_event(Instant::now());
                local_stamp = stamp;
            }
            last_local_check = Instant::now();
        }
        let next_wake = state.sync_wake.load(Ordering::Relaxed);
        if wake != next_wake {
            debounce.record_event(Instant::now());
            wake = next_wake;
        }
        if let Ok(settings) = crate::session::load_sync_settings(&state.vault_dir) {
            mode = settings.mode;
            if let Some(dir) = folder_sync_dir(&settings) {
                if watched_remote.as_ref() != Some(&dir) && dir.exists() {
                    if let Some(watcher) = &mut watcher {
                        if watcher.watch(&dir, RecursiveMode::Recursive).is_ok() {
                            watched_remote = Some(dir);
                        }
                    }
                }
            }
        }
        match rx.recv_timeout(SYNC_WATCH_POLL) {
            Ok(()) => {
                debounce.record_event(Instant::now());
                while rx.try_recv().is_ok() {}
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if stop.load(Ordering::Relaxed) || shutdown_requested(&state) {
            break;
        }
        let now = Instant::now();
        if now >= retry_after && (debounce.take_due(now) || now >= next_sync) {
            let status = match crate::server::run_sync_configured(&state) {
                Ok(report) => report.status,
                Err(err) => {
                    write_component_log(
                        AGENT_LOG,
                        "WARN",
                        &format!("sync watcher failed code={:?}", err.code),
                    );
                    match err.code {
                        AgentErrorCode::PermissionDenied | AgentErrorCode::Locked => {
                            SyncStatus::AuthFailed
                        }
                        _ => SyncStatus::Offline,
                    }
                }
            };
            let failed = matches!(
                status,
                SyncStatus::AuthFailed | SyncStatus::Offline | SyncStatus::ServerError
            );
            failures = if failed {
                failures.saturating_add(1)
            } else {
                0
            };
            let delay = next_delay(&mode, &status, failures);
            next_sync = Instant::now() + delay;
            retry_after = if failed { next_sync } else { Instant::now() };
        }
    }
}

// CloudKit pushes wake the same scheduler; periodic enumeration recovers
// dropped APNs notifications. WebDAV has no universal push facility.
fn next_delay(mode: &SyncMode, status: &SyncStatus, failures: u32) -> Duration {
    if matches!(status, SyncStatus::AuthFailed) {
        return Duration::from_secs(300);
    }
    if failures > 0 {
        return Duration::from_secs((5u64 << failures.min(6)).min(300));
    }
    Duration::from_secs(match mode {
        SyncMode::WebDav => 5,
        SyncMode::ICloud => 60,
        _ => 30,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuous_writes_cannot_starve_sync_and_failures_back_off() {
        let now = Instant::now();
        let mut debounce = Debounce::new(SYNC_WATCH_DEBOUNCE);
        for step in 0..8 {
            debounce.record_event(now + Duration::from_millis(step * 250));
            assert!(!debounce.take_due(now + Duration::from_millis(step * 250)));
        }
        debounce.record_event(now + Duration::from_secs(2));
        assert!(debounce.take_due(now + Duration::from_secs(2)));
        assert_eq!(
            next_delay(&SyncMode::WebDav, &SyncStatus::Idle, 0),
            Duration::from_secs(5)
        );
        assert!(
            next_delay(&SyncMode::WebDav, &SyncStatus::Offline, 3)
                > next_delay(&SyncMode::WebDav, &SyncStatus::Offline, 1)
        );
        assert_eq!(
            next_delay(&SyncMode::ICloud, &SyncStatus::Offline, 100),
            Duration::from_secs(300)
        );
        assert_eq!(
            next_delay(&SyncMode::WebDav, &SyncStatus::AuthFailed, 1),
            Duration::from_secs(300)
        );
    }

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
