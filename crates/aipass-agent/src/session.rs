use crate::paths::{canonical_vault_dir, default_vault_dir};
use aipass_agent_protocol::{
    AgentErrorCode, LockReason, SensitiveString, SessionPolicy, SessionStatus,
};
use aipass_crypto::SecretString;
use aipass_storage::atomic_write_bytes;
use aipass_vault::{RecoveryKit, Vault, VaultError};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use time::OffsetDateTime;
use zeroize::Zeroize;

#[derive(Clone, Debug, Deserialize)]
pub struct LegacyDesktopPreferences {
    #[serde(default)]
    pub auto_lock_minutes: Option<u16>,
    #[serde(default)]
    pub lock_on_sleep: Option<bool>,
    #[serde(default)]
    pub lock_on_screen_lock: Option<bool>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeHostSettings {
    #[serde(default)]
    pub ignored_origins: Vec<String>,
}

pub struct SessionInfo {
    pub vault: Vault,
    #[allow(dead_code)]
    pub unlocked_at: OffsetDateTime,
    pub last_activity_at: OffsetDateTime,
}

pub enum SessionState {
    Locked,
    Unlocked(SessionInfo),
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
        SessionState::Unlocked(SessionInfo {
            vault,
            unlocked_at: now,
            last_activity_at: now,
        });
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

pub fn set_session_vault(state: &Arc<AgentState>, vault: Vault) {
    let now = OffsetDateTime::now_utc();
    if let Ok(mut session) = state.session.lock() {
        *session = SessionState::Unlocked(SessionInfo {
            vault,
            unlocked_at: now,
            last_activity_at: now,
        });
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
    let mut policy = SessionPolicy::default();
    if canonical_vault_dir(vault_dir)? == canonical_vault_dir(default_vault_dir()?)? {
        let legacy_path = legacy_desktop_preferences_path()?;
        if legacy_path.exists() {
            let legacy: LegacyDesktopPreferences = serde_json::from_slice(&fs::read(legacy_path)?)?;
            if let Some(value) = legacy.auto_lock_minutes {
                policy.idle_lock_minutes = value.min(240);
            }
            if let Some(value) = legacy.lock_on_sleep {
                policy.lock_on_sleep = value;
            }
            if let Some(value) = legacy.lock_on_screen_lock {
                policy.lock_on_screen_lock = value;
            }
        }
    }
    Ok(policy)
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

pub fn legacy_desktop_preferences_path() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("dev", "aipass", "AIPass")
        .context("cannot determine project dir")?;
    Ok(dirs.config_dir().join("preferences.json"))
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
