use aipass_agent_protocol::CloudSyncProvider;
use anyhow::{bail, Context, Result};
use directories::ProjectDirs;
use sha2::{Digest, Sha256};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const CLOUD_SYNC_SUBDIR: &str = "AIPass";

pub fn default_vault_dir() -> Result<PathBuf> {
    let dirs =
        ProjectDirs::from("dev", "aipass", "desktop").context("cannot determine project dir")?;
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
        ProjectDirs::from("dev", "aipass", "desktop").context("cannot determine project dir")?;
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

pub fn cloud_sync_dir(provider: CloudSyncProvider) -> Result<PathBuf> {
    let root = match provider {
        CloudSyncProvider::ICloud => icloud_root_dir()?,
        CloudSyncProvider::OneDrive => onedrive_root_dir()?,
    };
    Ok(root.join(CLOUD_SYNC_SUBDIR))
}

fn icloud_root_dir() -> Result<PathBuf> {
    if let Some(explicit) = std::env::var_os("AIPASS_ICLOUD_ROOT") {
        return existing_dir(PathBuf::from(explicit), "AIPASS_ICLOUD_ROOT");
    }

    #[cfg(target_os = "macos")]
    {
        existing_dir(
            home_dir()?.join("Library/Mobile Documents/com~apple~CloudDocs"),
            "iCloud Drive",
        )
    }

    #[cfg(not(target_os = "macos"))]
    {
        bail!("iCloud Drive sync is only available on macOS")
    }
}

fn onedrive_root_dir() -> Result<PathBuf> {
    if let Some(explicit) = std::env::var_os("AIPASS_ONEDRIVE_ROOT") {
        return existing_dir(PathBuf::from(explicit), "AIPASS_ONEDRIVE_ROOT");
    }

    #[cfg(target_os = "windows")]
    {
        for key in ["OneDriveConsumer", "OneDriveCommercial", "OneDrive"] {
            if let Some(path) = std::env::var_os(key) {
                let candidate = PathBuf::from(path);
                if candidate.is_dir() {
                    return Ok(candidate);
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let cloud_storage = home_dir()?.join("Library/CloudStorage");
        if cloud_storage.is_dir() {
            let mut candidates = fs::read_dir(&cloud_storage)?
                .filter_map(|entry| entry.ok().map(|item| item.path()))
                .filter(|path| {
                    path.is_dir()
                        && path
                            .file_name()
                            .and_then(|value| value.to_str())
                            .is_some_and(|value| value.starts_with("OneDrive"))
                })
                .collect::<Vec<_>>();
            candidates.sort_by(|left, right| {
                rank_onedrive_candidate(left)
                    .cmp(&rank_onedrive_candidate(right))
                    .then(left.cmp(right))
            });
            if let Some(path) = candidates.into_iter().next() {
                return Ok(path);
            }
        }
    }

    let home = home_dir()?;
    for candidate in one_drive_home_candidates(&home) {
        if candidate.is_dir() {
            return Ok(candidate);
        }
    }

    bail!("OneDrive sync folder not found on this device")
}

fn existing_dir(path: PathBuf, label: &str) -> Result<PathBuf> {
    if path.is_dir() {
        Ok(path)
    } else {
        bail!("{label} directory not found: {}", path.display())
    }
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .context("home directory is not set")
}

fn one_drive_home_candidates(home: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![
        home.join("OneDrive"),
        home.join("OneDrive - Personal"),
        home.join("OneDrive-Personal"),
    ];
    if let Ok(entries) = fs::read_dir(home) {
        let mut discovered = entries
            .filter_map(|entry| entry.ok().map(|item| item.path()))
            .filter(|path| {
                path.is_dir()
                    && path
                        .file_name()
                        .and_then(|value| value.to_str())
                        .is_some_and(|value| value.starts_with("OneDrive"))
            })
            .collect::<Vec<_>>();
        discovered.sort();
        candidates.extend(discovered);
    }
    candidates
}

#[cfg(target_os = "macos")]
fn rank_onedrive_candidate(path: &Path) -> u8 {
    match path.file_name().and_then(|value| value.to_str()) {
        Some("OneDrive-Personal") => 0,
        Some("OneDrive") => 1,
        Some(_) => 2,
        None => 3,
    }
}
