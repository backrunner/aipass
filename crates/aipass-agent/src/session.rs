use aipass_agent_protocol::{
    AgentErrorCode, LockReason, SensitiveString, SessionPolicy, SessionStatus, SyncMode,
    SyncSettings, SyncSettingsUpdate,
};
use aipass_crypto::{decrypt_bytes, encrypt_bytes, Ciphertext, SecretString};
use aipass_storage::atomic_write_bytes;
use aipass_vault::{RecoveryKit, Vault, VaultError};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Condvar, Mutex,
};
use std::time::Duration as StdDuration;
use time::OffsetDateTime;
use zeroize::Zeroize;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeHostSettings {
    #[serde(default)]
    pub ignored_origins: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersistedSyncSettings {
    #[serde(default)]
    pub mode: SyncMode,
    #[serde(default)]
    pub sync_folder: Option<PathBuf>,
    #[serde(default)]
    pub webdav_url: Option<String>,
    #[serde(default)]
    pub webdav_username: Option<String>,
    #[serde(default)]
    pub webdav_password: Option<Ciphertext>,
}

#[derive(Clone, Debug)]
pub enum StoredSyncSecret {
    Plaintext(SensitiveString),
    Encrypted(Ciphertext),
}

#[derive(Clone, Debug, Default)]
pub struct StoredSyncSettings {
    pub mode: SyncMode,
    pub sync_folder: Option<PathBuf>,
    pub webdav_url: Option<String>,
    pub webdav_username: Option<String>,
    pub webdav_password: Option<StoredSyncSecret>,
}

pub struct SessionInfo {
    pub vault: Vault,
    #[allow(dead_code)]
    pub unlocked_at: OffsetDateTime,
    pub last_activity_at: OffsetDateTime,
}

pub enum SessionState {
    Locked,
    Unlocked(Box<SessionInfo>),
}

pub struct AgentState {
    pub vault_dir: PathBuf,
    pub namespace: String,
    pub auth_token: SensitiveString,
    pub policy: Mutex<SessionPolicy>,
    pub session: Mutex<SessionState>,
    pub session_changed: Condvar,
    pub last_lock_reason: Mutex<Option<LockReason>>,
    pub shutdown: AtomicBool,
}

#[derive(Debug)]
pub struct ServiceError {
    pub code: AgentErrorCode,
    pub message: String,
}

pub type ServiceResult<T> = std::result::Result<T, ServiceError>;

impl ServiceError {
    pub fn new(code: AgentErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn internal(err: impl Into<anyhow::Error>) -> Self {
        Self::new(AgentErrorCode::Internal, err.into().to_string())
    }

    pub fn response(self) -> aipass_agent_protocol::AgentResponse {
        aipass_agent_protocol::AgentResponse::error(self.code, self.message)
    }
}

impl From<anyhow::Error> for ServiceError {
    fn from(value: anyhow::Error) -> Self {
        Self::internal(value)
    }
}

pub fn current_policy(state: &Arc<AgentState>) -> ServiceResult<SessionPolicy> {
    state
        .policy
        .lock()
        .map(|policy| policy.clone())
        .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "policy lock poisoned"))
}

pub fn session_status(state: &Arc<AgentState>) -> ServiceResult<SessionStatus> {
    let locked = matches!(
        *state
            .session
            .lock()
            .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "session lock poisoned"))?,
        SessionState::Locked
    );
    Ok(SessionStatus {
        exists: manifest_path(&state.vault_dir).exists(),
        locked,
        policy: current_policy(state)?,
        last_lock_reason: state
            .last_lock_reason
            .lock()
            .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "lock reason poisoned"))?
            .clone(),
        vault_namespace: Some(state.namespace.clone()),
    })
}

pub fn unlock_with_password(
    state: &Arc<AgentState>,
    mut password: String,
) -> ServiceResult<SessionStatus> {
    if !manifest_path(&state.vault_dir).exists() {
        password.zeroize();
        return Err(ServiceError::new(
            AgentErrorCode::NotFound,
            "vault not initialized",
        ));
    }
    let vault =
        Vault::open(&state.vault_dir, &SecretString::new(&password)).map_err(map_vault_error);
    let vault = match vault {
        Ok(vault) => vault,
        Err(err) => {
            password.zeroize();
            return Err(err);
        }
    };
    password.zeroize();
    set_session_vault(state, vault);
    session_status(state)
}

pub fn create_vault(
    state: &Arc<AgentState>,
    mut password: String,
) -> ServiceResult<(RecoveryKit, SessionStatus)> {
    let creation =
        Vault::create(&state.vault_dir, &SecretString::new(&password)).map_err(map_vault_error);
    let creation = match creation {
        Ok(creation) => creation,
        Err(err) => {
            password.zeroize();
            return Err(err);
        }
    };
    password.zeroize();
    let recovery_kit = creation.recovery_kit.clone();
    set_session_vault(state, creation.vault);
    Ok((recovery_kit, session_status(state)?))
}

pub fn recover_vault(
    state: &Arc<AgentState>,
    mut recovery_key: String,
    mut new_password: String,
) -> ServiceResult<(RecoveryKit, SessionStatus)> {
    let creation = Vault::recover_master_password(
        &state.vault_dir,
        &SecretString::new(&recovery_key),
        &SecretString::new(&new_password),
    )
    .map_err(map_vault_error);
    recovery_key.zeroize();
    let creation = match creation {
        Ok(creation) => creation,
        Err(err) => {
            new_password.zeroize();
            return Err(err);
        }
    };
    new_password.zeroize();
    let recovery_kit = creation.recovery_kit.clone();
    set_session_vault(state, creation.vault);
    Ok((recovery_kit, session_status(state)?))
}

pub fn reset_vault(state: &Arc<AgentState>) -> ServiceResult<SessionStatus> {
    let root = &state.vault_dir;
    let remove_err = |err: std::io::Error| {
        ServiceError::new(
            AgentErrorCode::Internal,
            format!("failed to remove vault: {err}"),
        )
    };

    *state
        .session
        .lock()
        .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "session lock poisoned"))? =
        SessionState::Locked;
    state.session_changed.notify_all();
    *state
        .last_lock_reason
        .lock()
        .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "lock reason poisoned"))? = None;

    for file in ["manifest.aipmanifest", "sync-checkpoint.aipcheckpoint"] {
        remove_file_if_exists(&root.join(file)).map_err(remove_err)?;
    }
    for dir in ["objects", "audit", "devices", "grants", "index"] {
        remove_dir_if_exists(&root.join(dir)).map_err(remove_err)?;
    }
    remove_file_if_exists(&sync_settings_path(root)).map_err(remove_err)?;

    session_status(state)
}

pub fn set_session_vault(state: &Arc<AgentState>, vault: Vault) {
    let now = OffsetDateTime::now_utc();
    if let Ok(mut session) = state.session.lock() {
        *session = SessionState::Unlocked(Box::new(SessionInfo {
            vault,
            unlocked_at: now,
            last_activity_at: now,
        }));
    }
    if let Ok(mut reason) = state.last_lock_reason.lock() {
        *reason = None;
    }
    state.session_changed.notify_all();
}

pub fn touch_session(state: &Arc<AgentState>) {
    if let Ok(mut session) = state.session.lock() {
        if let SessionState::Unlocked(info) = &mut *session {
            info.last_activity_at = OffsetDateTime::now_utc();
        }
    }
}

pub fn lock_session(state: &Arc<AgentState>, reason: LockReason) {
    if let Ok(mut session) = state.session.lock() {
        *session = SessionState::Locked;
    }
    state.session_changed.notify_all();
    if let Ok(mut last_reason) = state.last_lock_reason.lock() {
        *last_reason = Some(reason);
    }
}

pub fn wait_for_unlock(
    state: &Arc<AgentState>,
    timeout: StdDuration,
) -> ServiceResult<SessionStatus> {
    let deadline = std::time::Instant::now() + timeout;
    let mut session = state
        .session
        .lock()
        .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "session lock poisoned"))?;
    loop {
        if matches!(*session, SessionState::Unlocked(_)) {
            drop(session);
            return session_status(state);
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            drop(session);
            return session_status(state);
        }
        let wait_for = deadline.saturating_duration_since(now);
        let (next_session, result) = state
            .session_changed
            .wait_timeout(session, wait_for)
            .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "session lock poisoned"))?;
        session = next_session;
        if result.timed_out() && std::time::Instant::now() >= deadline {
            drop(session);
            return session_status(state);
        }
    }
}

pub fn lock_if_idle(state: &Arc<AgentState>) -> ServiceResult<bool> {
    let policy = current_policy(state)?;
    if policy.idle_lock_minutes == 0 {
        return Ok(false);
    }
    let idle_threshold = time::Duration::minutes(policy.idle_lock_minutes.into());
    let should_lock = {
        let session = state
            .session
            .lock()
            .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "session lock poisoned"))?;
        match &*session {
            SessionState::Locked => false,
            SessionState::Unlocked(info) => {
                let request_idle = OffsetDateTime::now_utc() - info.last_activity_at;
                should_lock_for_idle(request_idle, system_idle_seconds(), idle_threshold)
            }
        }
    };
    if should_lock {
        lock_session(state, LockReason::IdleTimeout);
    }
    Ok(should_lock)
}

fn should_lock_for_idle(
    request_idle: time::Duration,
    system_idle_seconds: Option<f64>,
    threshold: time::Duration,
) -> bool {
    if request_idle < threshold {
        return false;
    }
    // Match 1Password's system-idle behavior: activity elsewhere on the
    // computer keeps the vault open even when AIPass itself is not in use.
    system_idle_seconds
        .map(|seconds| time::Duration::seconds_f64(seconds) >= threshold)
        .unwrap_or(true)
}

/// Seconds since the last system-wide input event (keyboard/mouse), or `None` when
/// the platform exposes no such signal.
#[cfg(target_os = "macos")]
pub fn system_idle_seconds() -> Option<f64> {
    use objc2_core_graphics::{CGEventSource, CGEventSourceStateID, CGEventType};
    const ANY_INPUT_EVENT_TYPE: CGEventType = CGEventType(0xFFFF_FFFF);
    let seconds = CGEventSource::seconds_since_last_event_type(
        CGEventSourceStateID::CombinedSessionState,
        ANY_INPUT_EVENT_TYPE,
    );
    (seconds.is_finite() && seconds >= 0.0).then_some(seconds)
}

#[cfg(not(target_os = "macos"))]
pub fn system_idle_seconds() -> Option<f64> {
    None
}

/// Lock the session when the OS sleeps or the screen locks, honoring the policy
/// flags. macOS only; other platforms wire this elsewhere (Windows service) or not
/// at all.
#[cfg(target_os = "macos")]
pub fn spawn_power_watcher(state: Arc<AgentState>) {
    use block2::RcBlock;
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSDistributedNotificationCenter, NSNotification, NSString};
    use std::ptr::NonNull;

    std::thread::spawn(move || {
        // NSWorkspace/distributed notifications are delivered on the thread's run loop.
        let workspace = NSWorkspace::sharedWorkspace();
        let workspace_center = workspace.notificationCenter();
        let distributed = NSDistributedNotificationCenter::defaultCenter();

        let make_handler = |reason: LockReason| {
            let state = state.clone();
            RcBlock::new(move |_note: NonNull<NSNotification>| {
                let lock = match reason {
                    LockReason::SystemSleep => current_policy(&state)
                        .map(|p| p.lock_on_sleep)
                        .unwrap_or(true),
                    LockReason::ScreenLock => current_policy(&state)
                        .map(|p| p.lock_on_screen_lock)
                        .unwrap_or(true),
                    _ => true,
                };
                if lock {
                    lock_session(&state, reason.clone());
                }
            })
        };

        let sleep_name = unsafe { objc2_app_kit::NSWorkspaceWillSleepNotification };
        let _sleep_obs = unsafe {
            workspace_center.addObserverForName_object_queue_usingBlock(
                Some(sleep_name),
                None,
                None,
                &make_handler(LockReason::SystemSleep),
            )
        };

        let lock_name = NSString::from_str("com.apple.screenIsLocked");
        let _lock_obs = unsafe {
            distributed.addObserverForName_object_queue_usingBlock(
                Some(&lock_name),
                None,
                None,
                &make_handler(LockReason::ScreenLock),
            )
        };

        // Keep the observers alive and pump the run loop for the agent's lifetime.
        loop {
            if state.shutdown.load(Ordering::Relaxed) {
                break;
            }
            objc2_core_foundation::CFRunLoop::run_in_mode(
                unsafe { objc2_core_foundation::kCFRunLoopDefaultMode },
                1.0,
                false,
            );
            // A run loop without a registered source returns immediately. Keep
            // that state from turning this watcher into a busy loop.
            std::thread::sleep(StdDuration::from_millis(50));
        }
        let _ = (_sleep_obs, _lock_obs);
    });
}

#[cfg(not(target_os = "macos"))]
pub fn spawn_power_watcher(_state: Arc<AgentState>) {}

pub fn clamp_policy(policy: SessionPolicy) -> SessionPolicy {
    SessionPolicy {
        idle_lock_minutes: policy.idle_lock_minutes.min(1_440),
        lock_on_sleep: policy.lock_on_sleep,
        lock_on_screen_lock: policy.lock_on_screen_lock,
    }
}

pub fn load_policy(vault_dir: &Path) -> Result<SessionPolicy> {
    let path = policy_path(vault_dir);
    if path.exists() {
        let value: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
        let had_persist_unlock = value.get("persistUnlock").is_some();
        let decoded: SessionPolicy = serde_json::from_value(value)?;
        let policy = clamp_policy(decoded.clone());
        if had_persist_unlock || policy != decoded {
            save_policy(vault_dir, &policy)?;
        }
        return Ok(policy);
    }
    Ok(SessionPolicy::default())
}

pub fn save_policy(vault_dir: &Path, policy: &SessionPolicy) -> Result<()> {
    let path = policy_path(vault_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    atomic_write_bytes(&path, &serde_json::to_vec_pretty(policy)?)?;
    Ok(())
}

pub fn policy_path(vault_dir: &Path) -> PathBuf {
    vault_dir
        .parent()
        .unwrap_or(vault_dir)
        .join("agent")
        .join("session-policy.json")
}

pub fn load_sync_settings(vault_dir: &Path) -> Result<StoredSyncSettings> {
    let path = sync_settings_path(vault_dir);
    if path.exists() {
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
        let had_device_marker = value.get("webdavPasswordDevice").is_some();
        let persisted: PersistedSyncSettings = serde_json::from_value(value)?;
        if had_device_marker {
            atomic_write_bytes(&path, &serde_json::to_vec_pretty(&persisted)?)?;
        }
        return Ok(StoredSyncSettings {
            mode: persisted.mode,
            sync_folder: persisted.sync_folder,
            webdav_url: persisted.webdav_url,
            webdav_username: persisted.webdav_username,
            webdav_password: persisted.webdav_password.map(StoredSyncSecret::Encrypted),
        });
    }
    Ok(StoredSyncSettings::default())
}

pub fn sync_settings_view(settings: &StoredSyncSettings) -> SyncSettings {
    SyncSettings {
        mode: settings.mode,
        sync_folder: settings.sync_folder.clone(),
        webdav_url: settings.webdav_url.clone(),
        webdav_username: settings.webdav_username.clone(),
        has_webdav_password: settings.webdav_password.is_some(),
    }
}

pub fn save_sync_settings(
    vault_dir: &Path,
    vault: &Vault,
    settings: &StoredSyncSettings,
) -> Result<StoredSyncSettings> {
    let normalized_secret = settings
        .webdav_password
        .clone()
        .map(|secret| persist_sync_secret(vault, secret))
        .transpose()?;
    let persisted = PersistedSyncSettings {
        mode: settings.mode,
        sync_folder: settings.sync_folder.clone(),
        webdav_url: settings.webdav_url.clone(),
        webdav_username: settings.webdav_username.clone(),
        webdav_password: match &normalized_secret {
            Some(StoredSyncSecret::Encrypted(ciphertext)) => Some(ciphertext.clone()),
            _ => None,
        },
    };
    let path = sync_settings_path(vault_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    atomic_write_bytes(&path, &serde_json::to_vec_pretty(&persisted)?)?;
    Ok(StoredSyncSettings {
        mode: settings.mode,
        sync_folder: settings.sync_folder.clone(),
        webdav_url: settings.webdav_url.clone(),
        webdav_username: settings.webdav_username.clone(),
        webdav_password: normalized_secret,
    })
}

pub fn apply_sync_settings_update(
    current: StoredSyncSettings,
    update: SyncSettingsUpdate,
) -> StoredSyncSettings {
    StoredSyncSettings {
        mode: update.mode,
        sync_folder: update.sync_folder,
        webdav_url: update.webdav_url,
        webdav_username: update.webdav_username,
        webdav_password: if update.clear_webdav_password {
            None
        } else if let Some(password) = update.webdav_password {
            Some(StoredSyncSecret::Plaintext(password))
        } else {
            current.webdav_password
        },
    }
}

pub fn sync_settings_password(
    settings: &StoredSyncSettings,
    vault: &Vault,
) -> Result<Option<SensitiveString>> {
    settings
        .webdav_password
        .as_ref()
        .map(|secret| decrypt_sync_secret(vault, secret))
        .transpose()
}

pub fn sync_settings_path(vault_dir: &Path) -> PathBuf {
    vault_dir
        .parent()
        .unwrap_or(vault_dir)
        .join("agent")
        .join("sync-settings.json")
}

pub fn native_host_settings_path(vault_dir: &Path) -> PathBuf {
    vault_dir
        .parent()
        .unwrap_or(vault_dir)
        .join("native-host")
        .join("preferences.json")
}

pub fn manifest_path(vault_dir: &Path) -> PathBuf {
    vault_dir.join("manifest.aipmanifest")
}

fn persist_sync_secret(vault: &Vault, secret: StoredSyncSecret) -> Result<StoredSyncSecret> {
    match secret {
        StoredSyncSecret::Plaintext(secret) => {
            let mut plaintext = secret.into_inner();
            let encrypted = encrypt_sync_plaintext(vault, &plaintext);
            plaintext.zeroize();
            encrypted.map(StoredSyncSecret::Encrypted)
        }
        StoredSyncSecret::Encrypted(ciphertext) => Ok(StoredSyncSecret::Encrypted(ciphertext)),
    }
}

fn encrypt_sync_plaintext(vault: &Vault, plaintext: &str) -> Result<Ciphertext> {
    Ok(encrypt_bytes(
        &vault.config_backup_key(),
        b"aipass sync settings webdav password v1",
        plaintext.as_bytes(),
    )?)
}

fn decrypt_sync_secret(vault: &Vault, secret: &StoredSyncSecret) -> Result<SensitiveString> {
    match secret {
        StoredSyncSecret::Plaintext(secret) => Ok(secret.clone()),
        StoredSyncSecret::Encrypted(ciphertext) => Ok(SensitiveString::new(
            decrypt_sync_ciphertext(vault, ciphertext)?,
        )),
    }
}

fn decrypt_sync_ciphertext(vault: &Vault, ciphertext: &Ciphertext) -> Result<String> {
    let mut bytes = decrypt_bytes(
        &vault.config_backup_key(),
        b"aipass sync settings webdav password v1",
        ciphertext,
    )?;
    match String::from_utf8(std::mem::take(&mut bytes)) {
        Ok(plaintext) => Ok(plaintext),
        Err(err) => {
            let mut bytes = err.into_bytes();
            bytes.zeroize();
            bail!("stored webdav password is not valid utf-8")
        }
    }
}

fn remove_file_if_exists(path: &Path) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

fn remove_dir_if_exists(path: &Path) -> std::io::Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

pub fn with_vault<T>(
    state: &Arc<AgentState>,
    touch: bool,
    f: impl FnOnce(&Vault) -> ServiceResult<T>,
) -> ServiceResult<T> {
    let result = {
        let session = state
            .session
            .lock()
            .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "session lock poisoned"))?;
        let vault = match &*session {
            SessionState::Locked => {
                return Err(ServiceError::new(AgentErrorCode::Locked, "vault is locked"))
            }
            SessionState::Unlocked(info) => &info.vault,
        };
        f(vault)?
    };
    if touch {
        touch_session(state);
    }
    Ok(result)
}

pub fn with_vault_mut<T>(
    state: &Arc<AgentState>,
    touch: bool,
    f: impl FnOnce(&mut Vault) -> ServiceResult<T>,
) -> ServiceResult<T> {
    let result = {
        let mut session = state
            .session
            .lock()
            .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "session lock poisoned"))?;
        let vault = match &mut *session {
            SessionState::Locked => {
                return Err(ServiceError::new(AgentErrorCode::Locked, "vault is locked"))
            }
            SessionState::Unlocked(info) => &mut info.vault,
        };
        f(vault)?
    };
    if touch {
        touch_session(state);
    }
    Ok(result)
}

pub fn map_vault_error(err: VaultError) -> ServiceError {
    match err {
        VaultError::NotFound => ServiceError::new(AgentErrorCode::NotFound, err.to_string()),
        VaultError::UnlockFailed | VaultError::RecoveryFailed => {
            ServiceError::new(AgentErrorCode::InvalidPassword, err.to_string())
        }
        VaultError::Locked => ServiceError::new(AgentErrorCode::Locked, err.to_string()),
        VaultError::GrantExpired | VaultError::GrantNotFound => {
            ServiceError::new(AgentErrorCode::GrantExpired, err.to_string())
        }
        VaultError::RecordNotFound | VaultError::DeviceNotFound => {
            ServiceError::new(AgentErrorCode::NotFound, err.to_string())
        }
        VaultError::DuplicateSecretLabel | VaultError::LastSecret | VaultError::AlreadyExists => {
            ServiceError::new(AgentErrorCode::Conflict, err.to_string())
        }
        VaultError::UnsupportedVersion | VaultError::InvalidExport => {
            ServiceError::new(AgentErrorCode::ValidationFailed, err.to_string())
        }
        VaultError::Io(err) => ServiceError::internal(err),
        VaultError::Json(err) => ServiceError::internal(err),
        VaultError::Crypto(err) => ServiceError::internal(err),
    }
}

pub fn shutdown_requested(state: &Arc<AgentState>) -> bool {
    state.shutdown.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use tempfile::tempdir;

    fn test_state(vault_dir: PathBuf) -> Arc<AgentState> {
        Arc::new(AgentState {
            policy: Mutex::new(SessionPolicy::default()),
            vault_dir,
            namespace: "test".to_string(),
            auth_token: SensitiveString::from("token"),
            session: Mutex::new(SessionState::Locked),
            session_changed: Condvar::new(),
            last_lock_reason: Mutex::new(None),
            shutdown: AtomicBool::new(false),
        })
    }

    #[test]
    fn wait_for_unlock_returns_when_session_changes() {
        let temp = tempdir().expect("tempdir");
        let vault_dir = temp.path().join("vault");
        let state = test_state(vault_dir);
        let wait_state = state.clone();
        let handle = thread::spawn(move || {
            wait_for_unlock(&wait_state, StdDuration::from_secs(15)).expect("wait for unlock")
        });

        thread::sleep(StdDuration::from_millis(50));
        create_vault(&state, "correct horse battery staple".to_string()).expect("create vault");

        let status = handle.join().expect("wait thread");
        assert!(status.exists);
        assert!(!status.locked);
    }

    #[test]
    fn wait_for_unlock_returns_locked_on_timeout() {
        let temp = tempdir().expect("tempdir");
        let vault_dir = temp.path().join("vault");
        let state = test_state(vault_dir);

        let status =
            wait_for_unlock(&state, StdDuration::from_millis(10)).expect("wait timeout status");
        assert!(!status.exists);
        assert!(status.locked);
    }

    #[test]
    fn save_sync_settings_encrypts_webdav_password_with_vault_key() {
        let temp = tempdir().expect("tempdir");
        let vault_dir = temp.path().join("vault");
        let creation = Vault::create(
            &vault_dir,
            &SecretString::new("correct horse battery staple"),
        )
        .expect("create vault");
        let settings = StoredSyncSettings {
            mode: SyncMode::WebDav,
            sync_folder: None,
            webdav_url: Some("https://dav.example".to_string()),
            webdav_username: Some("alice".to_string()),
            webdav_password: Some(StoredSyncSecret::Plaintext(SensitiveString::new(
                "webdav secret",
            ))),
        };

        let saved =
            save_sync_settings(&vault_dir, &creation.vault, &settings).expect("save settings");
        assert!(matches!(
            saved.webdav_password,
            Some(StoredSyncSecret::Encrypted(_))
        ));

        let bytes = fs::read(sync_settings_path(&vault_dir)).expect("read settings");
        assert!(!String::from_utf8_lossy(&bytes).contains("webdav secret"));
        assert!(!String::from_utf8_lossy(&bytes).contains("webdavPasswordDevice"));
        let persisted: PersistedSyncSettings =
            serde_json::from_slice(&bytes).expect("decode settings");
        assert!(persisted.webdav_password.is_some());
        let password = sync_settings_password(&saved, &creation.vault)
            .expect("decrypt settings")
            .expect("stored password");
        assert_eq!(password.expose(), "webdav secret");
    }

    #[test]
    fn legacy_device_password_marker_requires_password_reentry() {
        let temp = tempdir().expect("tempdir");
        let vault_dir = temp.path().join("vault");
        let path = sync_settings_path(&vault_dir);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        let persisted = serde_json::json!({
            "mode": "web_dav",
            "webdavUrl": "https://dav.example",
            "webdavUsername": "alice",
            "webdavPasswordDevice": true
        });
        atomic_write_bytes(
            &path,
            &serde_json::to_vec(&persisted).expect("encode settings"),
        )
        .expect("write settings");

        let loaded = load_sync_settings(&vault_dir).expect("load settings");
        assert!(loaded.webdav_password.is_none());
        let rewritten = fs::read_to_string(path).expect("read migrated settings");
        assert!(!rewritten.contains("webdavPasswordDevice"));
    }

    #[test]
    fn reset_vault_removes_vault_and_locks_session() {
        let temp = tempdir().expect("tempdir");
        let vault_dir = temp.path().join("vault");
        let state = Arc::new(AgentState {
            policy: Mutex::new(SessionPolicy::default()),
            vault_dir: vault_dir.clone(),
            namespace: "test".to_string(),
            auth_token: SensitiveString::from("token"),
            session: Mutex::new(SessionState::Locked),
            session_changed: Condvar::new(),
            last_lock_reason: Mutex::new(None),
            shutdown: AtomicBool::new(false),
        });

        create_vault(&state, "correct horse battery staple".to_string()).expect("create");
        let sync_settings = PersistedSyncSettings {
            mode: SyncMode::WebDav,
            sync_folder: Some(temp.path().join("sync")),
            webdav_url: Some("https://dav.example".to_string()),
            webdav_username: Some("alice".to_string()),
            webdav_password: None,
        };
        atomic_write_bytes(
            sync_settings_path(&vault_dir),
            &serde_json::to_vec(&sync_settings).expect("encode settings"),
        )
        .expect("write sync settings");
        assert!(manifest_path(&vault_dir).exists());
        assert!(sync_settings_path(&vault_dir).exists());

        let status = reset_vault(&state).expect("reset");
        assert!(!status.exists);
        assert!(status.locked);
        assert!(!manifest_path(&vault_dir).exists());
        assert!(!vault_dir.join("objects").exists());
        assert!(!sync_settings_path(&vault_dir).exists());
    }

    #[test]
    fn policy_defaults_to_relaxed_one_hour_idle_lock() {
        let policy = SessionPolicy::default();
        assert_eq!(policy.idle_lock_minutes, 60);
        assert!(policy.lock_on_sleep);
        assert!(policy.lock_on_screen_lock);

        let clamped = clamp_policy(SessionPolicy {
            idle_lock_minutes: u16::MAX,
            lock_on_sleep: false,
            lock_on_screen_lock: false,
        });
        assert_eq!(clamped.idle_lock_minutes, 1_440);
    }

    #[test]
    fn legacy_policy_is_clamped_and_drops_persist_unlock() {
        let temp = tempdir().expect("tempdir");
        let vault_dir = temp.path().join("vault");
        let path = policy_path(&vault_dir);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create policy parent");
        }
        atomic_write_bytes(
            &path,
            br#"{
                "idleLockMinutes": 65535,
                "lockOnSleep": true,
                "lockOnScreenLock": false,
                "persistUnlock": true
            }"#,
        )
        .expect("write legacy policy");

        let policy = load_policy(&vault_dir).expect("load policy");
        assert_eq!(policy.idle_lock_minutes, 1_440);
        assert!(!policy.lock_on_screen_lock);
        let rewritten = fs::read_to_string(path).expect("read migrated policy");
        assert!(!rewritten.contains("persistUnlock"));
    }

    #[test]
    fn idle_lock_requires_both_app_and_system_inactivity() {
        let threshold = time::Duration::minutes(60);

        assert!(!should_lock_for_idle(
            time::Duration::minutes(30),
            Some(7_200.0),
            threshold
        ));
        assert!(!should_lock_for_idle(
            time::Duration::minutes(120),
            Some(30.0),
            threshold
        ));
        assert!(should_lock_for_idle(
            time::Duration::minutes(120),
            Some(7_200.0),
            threshold
        ));
        assert!(should_lock_for_idle(
            time::Duration::minutes(120),
            None,
            threshold
        ));
    }
}
