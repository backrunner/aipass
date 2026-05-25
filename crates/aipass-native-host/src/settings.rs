use aipass_storage::atomic_write_bytes;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeHostSettings {
    #[serde(default)]
    ignored_origins: Vec<String>,
}

pub fn is_origin_ignored(vault_dir: &Path, origin: &str) -> Result<bool> {
    let origin = normalize_origin(origin)?;
    Ok(load_settings(vault_dir)?
        .ignored_origins
        .iter()
        .any(|value| value == &origin))
}

pub fn ignore_origin(vault_dir: &Path, origin: &str) -> Result<Vec<String>> {
    let origin = normalize_origin(origin)?;
    let mut settings = load_settings(vault_dir)?;
    if !settings
        .ignored_origins
        .iter()
        .any(|value| value == &origin)
    {
        settings.ignored_origins.push(origin);
        settings.ignored_origins.sort();
        settings.ignored_origins.dedup();
        save_settings(vault_dir, &settings)?;
    }
    Ok(settings.ignored_origins)
}

fn load_settings(vault_dir: &Path) -> Result<NativeHostSettings> {
    let path = settings_path(vault_dir);
    if !path.exists() {
        return Ok(NativeHostSettings::default());
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn save_settings(vault_dir: &Path, settings: &NativeHostSettings) -> Result<()> {
    let path = settings_path(vault_dir);
    atomic_write_bytes(&path, &serde_json::to_vec_pretty(settings)?)?;
    Ok(())
}

fn settings_path(vault_dir: &Path) -> PathBuf {
    vault_dir
        .parent()
        .unwrap_or(vault_dir)
        .join("native-host")
        .join("preferences.json")
}

fn normalize_origin(origin: &str) -> Result<String> {
    let normalized = origin.trim().trim_end_matches('/').to_lowercase();
    if normalized.is_empty() {
        bail!("origin is required");
    }
    Ok(normalized)
}
