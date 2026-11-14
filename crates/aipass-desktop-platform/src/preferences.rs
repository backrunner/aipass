//! Shared desktop preference schema and storage. No credentials are persisted.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPreferences {
    pub auto_lock_minutes: u16,
    pub clipboard_clear_seconds: u16,
    #[serde(default)]
    pub credential_click_action: CredentialClickAction,
    #[serde(default = "default_confirm_secret_copy")]
    pub confirm_secret_copy: bool,
    pub lock_on_sleep: bool,
    pub lock_on_screen_lock: bool,
    #[serde(default)]
    pub theme: ThemePreference,
    #[serde(default)]
    pub locale: LocalePreference,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_locale: Option<aipass_agent_protocol::UiLocale>,
    #[serde(default)]
    pub official_accounts_import: bool,
}

fn default_confirm_secret_copy() -> bool {
    true
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CredentialClickAction {
    #[default]
    Reveal,
    Copy,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

pub use aipass_agent_protocol::LocalePreference;

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            auto_lock_minutes: 60,
            clipboard_clear_seconds: 45,
            credential_click_action: CredentialClickAction::Reveal,
            confirm_secret_copy: true,
            lock_on_sleep: true,
            lock_on_screen_lock: true,
            theme: ThemePreference::System,
            locale: LocalePreference::System,
            resolved_locale: None,
            official_accounts_import: false,
        }
    }
}

/// Same per-app configuration path used by the existing desktop shell.
pub fn default_path() -> Result<PathBuf, String> {
    directories::BaseDirs::new()
        .map(|dirs| {
            dirs.config_dir()
                .join("com.alkinum.aipass/preferences.json")
        })
        .ok_or_else(|| "Desktop configuration directory is unavailable.".into())
}

pub fn load(path: &Path) -> Result<AppPreferences, String> {
    read(path).map(|bytes| serde_json::from_slice(&bytes).unwrap_or_default())
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.to_string()),
    }
}

pub fn load_and_migrate(path: &Path) -> Result<AppPreferences, String> {
    let bytes = read(path)?;
    let preferences = serde_json::from_slice(&bytes).unwrap_or_default();
    if serde_json::from_slice::<serde_json::Value>(&bytes)
        .is_ok_and(|value| value.get("persistUnlock").is_some())
    {
        save(path, &preferences)?;
    }
    Ok(preferences)
}

pub fn save(path: &Path, preferences: &AppPreferences) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(preferences).map_err(|error| error.to_string())?;
    aipass_storage::atomic_write_bytes(path, &bytes).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_read_preserves_old_file_and_explicit_migration_removes_unlock_flag() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preferences.json");
        let bytes = br#"{"autoLockMinutes":17,"clipboardClearSeconds":60,"lockOnSleep":true,"lockOnScreenLock":false,"persistUnlock":true,"credentialClickAction":"copy","confirmSecretCopy":false}"#;
        std::fs::write(&path, bytes).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.credential_click_action, CredentialClickAction::Copy);
        assert!(!loaded.confirm_secret_copy);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let migrated = load_and_migrate(&path).unwrap();
        assert_eq!(migrated.auto_lock_minutes, 17);
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(value.get("persistUnlock").is_none());
        assert_eq!(value["clipboardClearSeconds"], 60);
        assert_eq!(value["lockOnScreenLock"], false);
    }
    #[test]
    fn missing_and_malformed_files_default_without_overwriting_and_io_errors_surface() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preferences.json");
        assert!(load(&path).unwrap().confirm_secret_copy);
        assert!(!path.exists());
        std::fs::write(&path, b"invalid").unwrap();
        assert!(load_and_migrate(&path).unwrap().confirm_secret_copy);
        assert_eq!(std::fs::read(&path).unwrap(), b"invalid");
        assert!(load(dir.path()).is_err());
    }
}
