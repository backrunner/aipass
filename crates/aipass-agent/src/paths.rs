use aipass_agent_protocol::CloudSyncProvider;
use anyhow::{bail, Context, Result};
use directories::ProjectDirs;
use sha2::{Digest, Sha256};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const CLOUD_SYNC_SUBDIR: &str = "AIPass";

pub fn project_dirs() -> Result<ProjectDirs> {
    let application = if cfg!(target_os = "linux") {
        "aipass"
    } else {
        "desktop"
    };
    ProjectDirs::from("dev", "aipass", application).context("cannot determine project dir")
}

pub fn default_vault_dir() -> Result<PathBuf> {
    let preferred = project_dirs()?.data_dir().join("vault");
    // Do not move a possibly open vault or silently present existing users with
    // an empty vault. Only the legacy vault is retained, not the generic data root.
    if cfg!(target_os = "linux") {
        let legacy = ProjectDirs::from("dev", "aipass", "desktop")
            .context("cannot determine legacy project dir")?
            .data_dir()
            .join("vault");
        return Ok(select_vault_dir(preferred, legacy));
    }
    Ok(preferred)
}

fn select_vault_dir(preferred: PathBuf, legacy: PathBuf) -> PathBuf {
    if !preferred.exists() && legacy.is_dir() {
        legacy
    } else {
        preferred
    }
}

pub fn canonical_vault_dir(path: impl AsRef<Path>) -> Result<PathBuf> {
    #[cfg(windows)]
    return canonical_windows_vault(path.as_ref());
    #[cfg(not(windows))]
    canonical_unix_vault(path.as_ref())
}

#[cfg(not(windows))]
fn canonical_unix_vault(path: &Path) -> Result<PathBuf> {
    use std::path::Component;
    let absolute = std::path::absolute(path)?;
    // Resolve the longest existing ancestor, not just the immediate parent.
    // Otherwise a new nested vault under a symlink changes IPC namespace as
    // soon as the Agent creates its directories (including /var -> /private/var).
    for ancestor in absolute.ancestors() {
        let mut resolved = match fs::canonicalize(ancestor) {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        for component in absolute.strip_prefix(ancestor)?.components() {
            match component {
                Component::Normal(value) => resolved.push(value),
                Component::ParentDir => {
                    resolved.pop();
                }
                Component::CurDir => {}
                _ => bail!("invalid vault path"),
            }
        }
        return Ok(resolved);
    }
    bail!("vault has no existing ancestor")
}

#[cfg(windows)]
fn canonical_windows_vault(path: &Path) -> Result<PathBuf> {
    use std::path::{Component, Prefix};
    // Canonicalize the longest existing ancestor. Otherwise a new nested
    // vault hashes as C:\... before creation and \\?\C:\... afterwards, so
    // every later client connects to a different pipe than the first Agent.
    let absolute = std::path::absolute(path)?;
    let mut ancestor = absolute.as_path();
    let mut missing = Vec::new();
    while !ancestor.exists() {
        missing.push(
            ancestor
                .file_name()
                .context("invalid vault path")?
                .to_owned(),
        );
        ancestor = ancestor
            .parent()
            .context("vault has no existing ancestor")?;
    }
    let canonical = fs::canonicalize(ancestor)?;
    let mut components = canonical.components();
    let mut result = match components.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::VerbatimDisk(drive) => PathBuf::from(format!("{}:\\", drive as char)),
            Prefix::VerbatimUNC(server, share) => {
                let mut unc = std::ffi::OsString::from(r"\\");
                unc.push(server);
                unc.push(r"\");
                unc.push(share);
                PathBuf::from(unc)
            }
            _ => PathBuf::from(prefix.as_os_str()),
        },
        _ => anyhow::bail!("vault does not have an absolute Windows prefix"),
    };
    for component in components {
        result.push(component);
    }
    for component in missing.into_iter().rev() {
        result.push(component);
    }
    Ok(result)
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
        "com.aipass.agent.{}",
        namespace_for_vault_dir(path)?
    ))
}

/// XDG configuration roots must be absolute; ignore invalid relative overrides.
pub fn linux_config_home(home: &Path) -> PathBuf {
    linux_config_home_from(home, std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
}

fn linux_config_home_from(home: &Path, configured: Option<PathBuf>) -> PathBuf {
    configured
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".config"))
}

/// Ignore empty and relative XDG runtime roots, as required by the base directory spec.
pub fn linux_runtime_root(fallback: &Path) -> PathBuf {
    linux_runtime_root_from(
        fallback,
        std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from),
    )
}

fn linux_runtime_root_from(fallback: &Path, configured: Option<PathBuf>) -> PathBuf {
    configured
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| fallback.to_path_buf())
}

pub fn agent_runtime_dir() -> Result<PathBuf> {
    let dirs = project_dirs()?;
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
            linux_runtime_root(&dirs.data_local_dir().join("runtime"))
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
    aipass_config_writers::user_home_dir().context("home directory is not set")
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

#[cfg(test)]
mod tests {
    #[test]
    fn legacy_vault_is_preserved_without_overriding_new_vault() {
        let temp = tempfile::tempdir().expect("tempdir");
        let preferred = temp.path().join("aipass/vault");
        let legacy = temp.path().join("desktop/vault");
        assert_eq!(
            super::select_vault_dir(preferred.clone(), legacy.clone()),
            preferred
        );
        std::fs::create_dir_all(&legacy).expect("legacy");
        assert_eq!(
            super::select_vault_dir(preferred.clone(), legacy.clone()),
            legacy
        );
        std::fs::create_dir_all(&preferred).expect("preferred");
        assert_eq!(
            super::select_vault_dir(preferred.clone(), legacy),
            preferred
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_project_directory_is_application_specific() {
        assert!(super::project_dirs()
            .expect("dirs")
            .data_dir()
            .ends_with("aipass"));
    }

    use super::*;
    use std::sync::{Mutex, OnceLock};
    use tempfile::tempdir;

    #[cfg(unix)]
    #[test]
    fn nested_new_unix_vault_keeps_its_namespace_through_a_symlink() {
        let root = tempdir().unwrap();
        let real = root.path().join("real");
        fs::create_dir(&real).unwrap();
        let alias = root.path().join("alias");
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let vault = alias.join("new/nested/vault");
        let before = namespace_for_vault_dir(&vault).unwrap();
        let expected = fs::canonicalize(&real).unwrap().join("new/nested/vault");
        assert_eq!(canonical_vault_dir(&vault).unwrap(), expected);
        fs::create_dir_all(&vault).unwrap();
        assert_eq!(namespace_for_vault_dir(&vault).unwrap(), before);
        assert_eq!(namespace_for_vault_dir(&expected).unwrap(), before);
    }

    #[cfg(unix)]
    #[test]
    fn unix_vault_resolves_missing_suffix_dots_without_reinterpreting_existing_symlinks() {
        let root = tempdir().unwrap();
        let real = root.path().join("real/child");
        fs::create_dir_all(&real).unwrap();
        let alias = root.path().join("alias");
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let expected = fs::canonicalize(real.parent().unwrap())
            .unwrap()
            .join("vault");
        assert_eq!(
            canonical_vault_dir(alias.join("../vault")).unwrap(),
            expected
        );
        let missing = alias.join("new/../nested/vault");
        let expected = fs::canonicalize(&real).unwrap().join("nested/vault");
        assert_eq!(canonical_vault_dir(&missing).unwrap(), expected);
        fs::create_dir_all(&expected).unwrap();
        assert_eq!(canonical_vault_dir(&expected).unwrap(), expected);
        assert_eq!(canonical_vault_dir(&missing).unwrap(), expected);
    }

    #[cfg(windows)]
    #[test]
    fn nested_new_windows_vault_keeps_its_pipe_namespace_after_creation() {
        let root = tempdir().unwrap();
        let vault = root.path().join("中文 空格/parent/vault");
        let before = namespace_for_vault_dir(&vault).unwrap();
        let path = canonical_vault_dir(&vault).unwrap();
        fs::create_dir_all(&vault).unwrap();
        assert_eq!(canonical_vault_dir(&vault).unwrap(), path);
        assert_eq!(namespace_for_vault_dir(&vault).unwrap(), before);
        assert_eq!(
            namespace_for_vault_dir(fs::canonicalize(&vault).unwrap()).unwrap(),
            before
        );
    }

    #[test]
    fn linux_configuration_uses_only_absolute_xdg_overrides() {
        let root = tempdir().unwrap();
        let home = root.path();
        let custom = root.path().join("custom/config");
        assert_eq!(linux_config_home_from(home, Some(custom.clone())), custom);
        assert_eq!(
            linux_config_home_from(home, Some("relative".into())),
            home.join(".config")
        );
        assert_eq!(linux_config_home_from(home, None), home.join(".config"));
    }

    #[test]
    fn linux_runtime_ignores_empty_and_relative_xdg_roots() {
        let root = tempdir().unwrap();
        let fallback = root.path().join("fallback/runtime");
        let configured = root.path().join("custom/runtime");
        for configured in [None, Some("".into()), Some("relative".into())] {
            assert_eq!(linux_runtime_root_from(&fallback, configured), fallback);
        }
        assert_eq!(
            linux_runtime_root_from(&fallback, Some(configured.clone())),
            configured
        );
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvRestore {
        name: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvRestore {
        fn capture(name: &'static str) -> Self {
            Self {
                name,
                previous: std::env::var_os(name),
            }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }

    #[test]
    fn icloud_override_resolves_aipass_subdirectory() {
        let _guard = env_lock().lock().unwrap();
        let _restore = EnvRestore::capture("AIPASS_ICLOUD_ROOT");
        let root = tempdir().unwrap();
        std::env::set_var("AIPASS_ICLOUD_ROOT", root.path());
        let vault = tempdir().unwrap();
        let sync_dir = cloud_sync_dir(CloudSyncProvider::ICloud).unwrap();

        aipass_sync::sync_local_folder(vault.path(), &sync_dir).unwrap();

        assert_eq!(sync_dir, root.path().join(CLOUD_SYNC_SUBDIR));
        assert!(sync_dir.join("objects").is_dir());
    }

    #[test]
    fn icloud_override_requires_an_existing_directory() {
        let _guard = env_lock().lock().unwrap();
        let _restore = EnvRestore::capture("AIPASS_ICLOUD_ROOT");
        let missing = tempdir().unwrap().path().join("missing");
        std::env::set_var("AIPASS_ICLOUD_ROOT", &missing);

        let error = cloud_sync_dir(CloudSyncProvider::ICloud).expect_err("missing root");
        assert!(error
            .to_string()
            .contains("AIPASS_ICLOUD_ROOT directory not found"));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn icloud_without_override_reports_platform_error() {
        let _guard = env_lock().lock().unwrap();
        let _restore = EnvRestore::capture("AIPASS_ICLOUD_ROOT");
        std::env::remove_var("AIPASS_ICLOUD_ROOT");

        let error = cloud_sync_dir(CloudSyncProvider::ICloud).expect_err("non-macOS iCloud");
        assert!(error
            .to_string()
            .contains("iCloud Drive sync is only available on macOS"));
    }
}
