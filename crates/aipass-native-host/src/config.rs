use aipass_storage::atomic_write_bytes;
use anyhow::Result;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct NativeHostConfig {
    pub vault_dir: PathBuf,
    pub allowed_extension_ids: Vec<String>,
}

impl NativeHostConfig {
    pub fn from_env() -> Result<Self> {
        let vault_dir = std::env::var("AIPASS_VAULT_DIR")
            .map(PathBuf::from)
            .or_else(|_| default_vault_dir())?;
        let allowed_extension_ids = allowed_extension_ids_from_env()
            .filter(|ids| !ids.is_empty())
            .or_else(|| load_allowed_extension_ids().ok())
            .unwrap_or_default();
        Ok(Self {
            vault_dir,
            allowed_extension_ids,
        })
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeHostSettings {
    #[serde(default)]
    pub allowed_extension_ids: Vec<String>,
}

pub fn native_host_settings_path() -> Result<PathBuf> {
    let dirs = project_dirs()?;
    Ok(dirs.config_dir().join("native-host.json"))
}

pub fn load_allowed_extension_ids() -> Result<Vec<String>> {
    let path = native_host_settings_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let settings: NativeHostSettings = serde_json::from_slice(&fs::read(path)?)?;
    Ok(clean_extension_ids(settings.allowed_extension_ids))
}

pub fn save_allowed_extension_ids(ids: &[String]) -> Result<PathBuf> {
    let path = native_host_settings_path()?;
    let settings = NativeHostSettings {
        allowed_extension_ids: clean_extension_ids(ids.iter().cloned()),
    };
    atomic_write_bytes(&path, &serde_json::to_vec_pretty(&settings)?)?;
    Ok(path)
}

fn allowed_extension_ids_from_env() -> Option<Vec<String>> {
    std::env::var("AIPASS_ALLOWED_EXTENSION_IDS")
        .ok()
        .map(|value| clean_extension_ids(value.split(',').map(ToString::to_string)))
}

fn clean_extension_ids(values: impl IntoIterator<Item = String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| normalize_extension_id(&value))
        .filter(|value| !value.is_empty())
        .collect()
}

fn normalize_extension_id(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("chrome-extension://")
        .trim_start_matches("chrome://")
        .trim_end_matches('/')
        .to_lowercase()
}

fn default_vault_dir() -> Result<PathBuf, std::env::VarError> {
    if let Ok(dirs) = project_dirs() {
        Ok(dirs.data_dir().join("vault"))
    } else {
        Err(std::env::VarError::NotPresent)
    }
}

fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("dev", "aipass", "AIPass")
        .ok_or_else(|| anyhow::anyhow!("cannot determine AIPass project directory"))
}
