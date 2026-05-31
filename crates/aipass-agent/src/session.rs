use crate::device_secrets;
use aipass_agent_protocol::{
    AgentErrorCode, LockReason, SensitiveString, SessionPolicy, SessionStatus, SyncMode,
    SyncSettings, SyncSettingsUpdate,
};
use aipass_crypto::{decrypt_bytes, encrypt_bytes, Ciphertext, SecretString};
use aipass_storage::atomic_write_bytes;
use aipass_vault::{RecoveryKit, Vault, VaultError};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
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
    #[serde(default)]
    pub webdav_password_device: bool,
}

#[derive(Clone, Debug)]
pub enum StoredSyncSecret {
    Plaintext(SensitiveString),
    Encrypted(Ciphertext),
    Device,
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
    password.zeroize();
    let vault = vault?;
    let now = OffsetDateTime::now_utc();
    *state
        .session
        .lock()
        .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "session lock poisoned"))? =
        SessionState::Unlocked(Box::new(SessionInfo {
            vault,
            unlocked_at: now,
            last_activity_at: now,
        }));
    *state
        .last_lock_reason
        .lock()
        .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "lock reason poisoned"))? = None;
    session_status(state)
}

pub fn create_vault(
    state: &Arc<AgentState>,
    mut password: String,
) -> ServiceResult<(RecoveryKit, SessionStatus)> {
    let creation =
        Vault::create(&state.vault_dir, &SecretString::new(&password)).map_err(map_vault_error);
    password.zeroize();
    let creation = creation?;
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
    new_password.zeroize();
    let creation = creation?;
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
    device_secrets::delete_webdav_password(root).ok();

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
    if let Ok(mut last_reason) = state.last_lock_reason.lock() {
        *last_reason = Some(reason);
    }
}

pub fn lock_if_idle(state: &Arc<AgentState>) -> ServiceResult<bool> {
    let policy = current_policy(state)?;
    if policy.idle_lock_minutes == 0 {
        return Ok(false);
    }
    let should_lock = {
        let session = state
            .session
            .lock()
            .map_err(|_| ServiceError::new(AgentErrorCode::Internal, "session lock poisoned"))?;
        match &*session {
            SessionState::Locked => false,
            SessionState::Unlocked(info) => {
                let idle_for = OffsetDateTime::now_utc() - info.last_activity_at;
                idle_for >= time::Duration::minutes(policy.idle_lock_minutes.into())
            }
        }
    };
    if should_lock {
        lock_session(state, LockReason::IdleTimeout);
    }
    Ok(should_lock)
}

pub fn clamp_policy(policy: SessionPolicy) -> SessionPolicy {
    SessionPolicy {
        idle_lock_minutes: policy.idle_lock_minutes.min(240),
        lock_on_sleep: policy.lock_on_sleep,
        lock_on_screen_lock: policy.lock_on_screen_lock,
    }
}

pub fn load_policy(vault_dir: &Path) -> Result<SessionPolicy> {
    let path = policy_path(vault_dir);
    if path.exists() {
        return Ok(clamp_policy(serde_json::from_slice(&fs::read(path)?)?));
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
        let persisted: PersistedSyncSettings = serde_json::from_slice(&fs::read(path)?)?;
        return Ok(StoredSyncSettings {
            mode: persisted.mode,
            sync_folder: persisted.sync_folder,
            webdav_url: persisted.webdav_url,
            webdav_username: persisted.webdav_username,
            webdav_password: if persisted.webdav_password_device {
                Some(StoredSyncSecret::Device)
            } else {
                persisted.webdav_password.map(StoredSyncSecret::Encrypted)
            },
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
    vault: Option<&Vault>,
    settings: &StoredSyncSettings,
) -> Result<StoredSyncSettings> {
    let normalized_secret = settings
        .webdav_password
        .clone()
        .map(|secret| persist_sync_secret(vault_dir, vault, secret))
        .transpose()?;
    if !matches!(normalized_secret, Some(StoredSyncSecret::Device)) {
        device_secrets::delete_webdav_password(vault_dir).ok();
    }
    let persisted = PersistedSyncSettings {
        mode: settings.mode,
        sync_folder: settings.sync_folder.clone(),
        webdav_url: settings.webdav_url.clone(),
        webdav_username: settings.webdav_username.clone(),
        webdav_password: match &normalized_secret {
            Some(StoredSyncSecret::Encrypted(ciphertext)) => Some(ciphertext.clone()),
            _ => None,
        },
        webdav_password_device: matches!(normalized_secret, Some(StoredSyncSecret::Device)),
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
    vault_dir: &Path,
    settings: &StoredSyncSettings,
    vault: &Vault,
) -> Result<Option<SensitiveString>> {
    settings
        .webdav_password
        .as_ref()
        .map(|secret| decrypt_sync_secret(vault_dir, vault, secret))
        .transpose()
}

pub fn sync_settings_password_without_vault(
    vault_dir: &Path,
    settings: &StoredSyncSettings,
) -> Result<Option<SensitiveString>> {
    match settings.webdav_password.as_ref() {
        Some(StoredSyncSecret::Plaintext(secret)) => Ok(Some(secret.clone())),
        Some(StoredSyncSecret::Device) => Ok(Some(
            device_secrets::get_webdav_password(vault_dir)?
                .map(SensitiveString::new)
                .context("webdav password is missing from device secret storage")?,
        )),
        _ => Ok(None),
    }
}

pub fn sync_settings_password_requires_vault(settings: &StoredSyncSettings) -> bool {
    matches!(
        settings.webdav_password,
        Some(StoredSyncSecret::Encrypted(_))
    )
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

fn persist_sync_secret(
    vault_dir: &Path,
    vault: Option<&Vault>,
    secret: StoredSyncSecret,
) -> Result<StoredSyncSecret> {
    match secret {
        StoredSyncSecret::Plaintext(secret) => {
            let mut plaintext = secret.into_inner();
            let stored_in_device =
                device_secrets::set_webdav_password(vault_dir, &plaintext).unwrap_or(false);
            if stored_in_device {
                plaintext.zeroize();
                return Ok(StoredSyncSecret::Device);
            }
            let Some(vault) = vault else {
                plaintext.zeroize();
                bail!("vault is locked and device secret storage is unavailable");
            };
            let ciphertext = encrypt_sync_plaintext(vault, &plaintext)?;
            plaintext.zeroize();
            Ok(StoredSyncSecret::Encrypted(ciphertext))
        }
        StoredSyncSecret::Encrypted(ciphertext) => {
            let Some(vault) = vault else {
                return Ok(StoredSyncSecret::Encrypted(ciphertext));
            };
            let mut plaintext = decrypt_sync_ciphertext(vault, &ciphertext)?;
            let stored_in_device =
                device_secrets::set_webdav_password(vault_dir, &plaintext).unwrap_or(false);
            plaintext.zeroize();
            if stored_in_device {
                Ok(StoredSyncSecret::Device)
            } else {
                Ok(StoredSyncSecret::Encrypted(ciphertext))
            }
        }
        StoredSyncSecret::Device => Ok(StoredSyncSecret::Device),
    }
}

fn encrypt_sync_plaintext(vault: &Vault, plaintext: &str) -> Result<Ciphertext> {
    Ok(encrypt_bytes(
        &vault.config_backup_key(),
        b"aipass sync settings webdav password v1",
        plaintext.as_bytes(),
    )?)
}

fn decrypt_sync_secret(
    vault_dir: &Path,
    vault: &Vault,
    secret: &StoredSyncSecret,
) -> Result<SensitiveString> {
    match secret {
        StoredSyncSecret::Plaintext(secret) => Ok(secret.clone()),
        StoredSyncSecret::Device => device_secrets::get_webdav_password(vault_dir)?
            .map(SensitiveString::new)
            .context("webdav password is missing from device secret storage"),
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
    use tempfile::tempdir;

    #[test]
    fn save_sync_settings_persists_device_marker() {
        let temp = tempdir().expect("tempdir");
        let vault_dir = temp.path().join("vault");
        let settings = StoredSyncSettings {
            mode: SyncMode::WebDav,
            sync_folder: None,
            webdav_url: Some("https://dav.example".to_string()),
            webdav_username: Some("alice".to_string()),
            webdav_password: Some(StoredSyncSecret::Device),
        };

        let saved = save_sync_settings(&vault_dir, None, &settings).expect("save settings");
        assert!(matches!(
            saved.webdav_password,
            Some(StoredSyncSecret::Device)
        ));

        let persisted: PersistedSyncSettings = serde_json::from_slice(
            &fs::read(sync_settings_path(&vault_dir)).expect("read settings"),
        )
        .expect("decode settings");
        assert!(persisted.webdav_password.is_none());
        assert!(persisted.webdav_password_device);
    }

    #[test]
    fn load_sync_settings_prefers_device_marker_over_ciphertext() {
        let temp = tempdir().expect("tempdir");
        let vault_dir = temp.path().join("vault");
        let path = sync_settings_path(&vault_dir);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        let persisted = PersistedSyncSettings {
            mode: SyncMode::WebDav,
            sync_folder: Some(PathBuf::from("/tmp/aipass-sync")),
            webdav_url: Some("https://dav.example".to_string()),
            webdav_username: Some("alice".to_string()),
            webdav_password: Some(Ciphertext {
                aead: "xchacha20poly1305".to_string(),
                nonce_b64: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_string(),
                ciphertext_b64: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_string(),
            }),
            webdav_password_device: true,
        };
        atomic_write_bytes(
            &path,
            &serde_json::to_vec(&persisted).expect("encode settings"),
        )
        .expect("write settings");

        let loaded = load_sync_settings(&vault_dir).expect("load settings");
        assert!(matches!(
            loaded.webdav_password,
            Some(StoredSyncSecret::Device)
        ));
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
            webdav_password_device: false,
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
}
