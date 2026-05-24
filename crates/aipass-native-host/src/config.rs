use anyhow::Result;
use directories::ProjectDirs;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct NativeHostConfig {
    pub vault_dir: PathBuf,
    pub master_password: Option<String>,
    pub allowed_extension_ids: Vec<String>,
}

impl NativeHostConfig {
    pub fn from_env() -> Result<Self> {
        let vault_dir = std::env::var("AIPASS_VAULT_DIR")
            .map(PathBuf::from)
            .or_else(|_| default_vault_dir())?;
        let master_password = std::env::var("AIPASS_MASTER_PASSWORD").ok();
        let allowed_extension_ids = std::env::var("AIPASS_ALLOWED_EXTENSION_IDS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .collect();
        Ok(Self {
            vault_dir,
            master_password,
            allowed_extension_ids,
        })
    }
}

fn default_vault_dir() -> Result<PathBuf, std::env::VarError> {
    if let Some(dirs) = ProjectDirs::from("dev", "aipass", "AIPass") {
        Ok(dirs.data_dir().join("vault"))
    } else {
        Err(std::env::VarError::NotPresent)
    }
}
