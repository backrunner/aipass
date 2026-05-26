use anyhow::{Context, Result};
use directories::ProjectDirs;
use sha2::{Digest, Sha256};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub fn default_vault_dir() -> Result<PathBuf> {
    let dirs =
        ProjectDirs::from("dev", "aipass", "AIPass").context("cannot determine project dir")?;
    Ok(dirs.data_dir().join("vault"))
}

pub fn canonical_vault_dir(path: impl AsRef<Path>) -> Result<PathBuf> {
    let path = path.as_ref();
    if path.exists() {
        return fs::canonicalize(path).map_err(Into::into);
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let canonical_parent = fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
    Ok(canonical_parent.join(
        path.file_name()
            .map(|value| value.to_os_string())
            .unwrap_or_default(),
    ))
}

pub fn namespace_for_vault_dir(path: impl AsRef<Path>) -> Result<String> {
    let canonical = canonical_vault_dir(path)?;
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    let hash = hasher.finalize();
    Ok(hash[..16]
        .iter()
        .map(|value| format!("{value:02x}"))
        .collect())
}

pub fn agent_service_name(path: impl AsRef<Path>) -> Result<String> {
    Ok(format!(
        "dev.aipass.agent.{}",
        namespace_for_vault_dir(path)?
    ))
}

pub fn agent_runtime_dir() -> Result<PathBuf> {
    let dirs =
        ProjectDirs::from("dev", "aipass", "AIPass").context("cannot determine project dir")?;
    let dir = if let Some(explicit) = std::env::var_os("AIPASS_AGENT_RUNTIME_DIR") {
        PathBuf::from(explicit)
    } else if cfg!(target_os = "windows") {
        dirs.data_local_dir().join("agent")
    } else {
        let root = if cfg!(target_os = "macos") {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .context("HOME is not set")?
                .join(".aipass")
                .join("run")
        } else {
            std::env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| dirs.data_local_dir().join("runtime"))
        };
        root.join("aipass-agent")
    };
    fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    Ok(dir)
}

pub fn agent_socket_path(path: impl AsRef<Path>) -> Result<PathBuf> {
    let namespace = namespace_for_vault_dir(path)?;
    if cfg!(target_os = "windows") {
        Ok(PathBuf::from(format!(
            r"\\.\pipe\dev.aipass.agent.{namespace}"
        )))
    } else {
        Ok(agent_runtime_dir()?.join(format!("{}.sock", namespace)))
    }
}
