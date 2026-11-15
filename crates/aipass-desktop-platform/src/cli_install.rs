use std::fs;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Get the path where the CLI binary should be installed
#[cfg(target_os = "macos")]
fn cli_install_path() -> Option<PathBuf> {
    // Try user's local bin first, then system-wide
    let home = std::env::var("HOME").ok()?;
    let user_bin = PathBuf::from(home).join(".local/bin/aipass");

    // Check if .local/bin exists and is in PATH
    if user_bin.parent().map(|p| p.exists()).unwrap_or(false) {
        return Some(user_bin);
    }

    // Fallback to /usr/local/bin
    Some(PathBuf::from("/usr/local/bin/aipass"))
}

#[cfg(target_os = "windows")]
fn cli_install_path() -> Option<PathBuf> {
    // On Windows, install to %LOCALAPPDATA%\Programs\AIPass
    let local_app_data = std::env::var("LOCALAPPDATA").ok()?;
    let install_dir = PathBuf::from(local_app_data)
        .join("Programs")
        .join("AIPass");

    fs::create_dir_all(&install_dir).ok()?;
    Some(install_dir.join("aipass.exe"))
}

#[cfg(target_os = "linux")]
fn cli_install_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".local/bin/aipass"))
}

/// Get the path to the bundled CLI binary
fn bundled_cli_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        // Bundled binaries ship as desktop resources under Contents/Resources;
        // fall back to Contents/MacOS for sidecar-style layouts.
        let exe_path = std::env::current_exe().ok()?;
        let macos_dir = exe_path.parent()?;
        let resources_dir = macos_dir.parent()?.join("Resources");
        for dir in [resources_dir, macos_dir.to_path_buf()] {
            let candidate = dir.join("aipass");
            if candidate.exists() {
                return Some(candidate);
            }
        }
        Some(macos_dir.join("aipass"))
    }

    #[cfg(target_os = "windows")]
    {
        // In Windows: next to the .exe
        let exe_path = std::env::current_exe().ok()?;
        let app_dir = exe_path.parent()?;
        Some(app_dir.join("aipass.exe"))
    }

    #[cfg(target_os = "linux")]
    {
        let exe_path = std::env::current_exe().ok()?;
        aipass_agent::bundled::helper_candidates(&exe_path, "aipass")
            .into_iter()
            .find(|candidate| candidate.is_file())
    }
}

/// Install or update the CLI binary to the system PATH
pub fn install_cli() -> Result<PathBuf, String> {
    let bundled_path =
        bundled_cli_path().ok_or_else(|| "Could not locate bundled CLI binary".to_string())?;

    if !bundled_path.exists() {
        return Err(format!(
            "Bundled CLI binary not found at {}",
            bundled_path.display()
        ));
    }

    let install_path = cli_install_path()
        .ok_or_else(|| "Could not determine CLI installation path".to_string())?;

    // Create parent directory if needed
    if let Some(parent) = install_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create installation directory: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    install_linux_helpers(&bundled_path, &install_path)?;
    #[cfg(windows)]
    install_windows_helpers(&bundled_path, &install_path)?;

    // A distro package may already install the CLI at this exact path.
    if fs::canonicalize(&bundled_path).ok() == fs::canonicalize(&install_path).ok()
        && install_path.exists()
    {
        return Ok(install_path);
    }

    // Copy the binary
    copy_executable(&bundled_path, &install_path)?;

    Ok(install_path)
}

fn copy_executable(source: &Path, destination: &Path) -> Result<(), String> {
    let bytes = fs::read(source).map_err(|err| err.to_string())?;
    // Rename a new inode into place: overwriting a running ELF fails with
    // ETXTBSY on Linux and can leave a partial executable after an IO error.
    if !fs::read(destination).is_ok_and(|installed| installed == bytes) {
        aipass_storage::atomic_write_bytes(destination, &bytes).map_err(|err| err.to_string())?;
    }

    // Set executable permissions on Unix
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(destination)
            .map_err(|e| format!("Failed to read file permissions: {}", e))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(destination, perms)
            .map_err(|e| format!("Failed to set executable permissions: {}", e))?;
    }

    Ok(())
}

#[cfg(any(windows, test))]
fn install_windows_helpers(bundled: &Path, installed: &Path) -> Result<(), String> {
    let source = bundled.parent().ok_or("invalid bundled CLI path")?;
    let destination = installed.parent().ok_or("invalid CLI installation path")?;
    for name in ["aipass-agent.exe", "aipass-native-host.exe"] {
        let from = source.join(name);
        let to = destination.join(name);
        if fs::canonicalize(&from).ok() != fs::canonicalize(&to).ok() || !to.is_file() {
            copy_executable(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(any(target_os = "linux", all(test, unix)))]
fn install_linux_helpers(bundled: &Path, installed: &Path) -> Result<(), String> {
    let resources = bundled.parent().ok_or("invalid bundled CLI path")?;
    let prefix = installed
        .parent()
        .and_then(Path::parent)
        .ok_or("invalid CLI installation path")?;
    let destination = prefix.join("lib/AIPass");
    fs::create_dir_all(&destination).map_err(|err| err.to_string())?;
    for name in ["aipass-agent", "aipass-native-host"] {
        copy_executable(&resources.join(name), &destination.join(name))?;
    }
    Ok(())
}

/// Check if the CLI is already installed and matches the bundled binary
pub fn is_cli_installed() -> bool {
    let (Some(install_path), Some(bundled_path)) = (cli_install_path(), bundled_cli_path()) else {
        return false;
    };
    if !install_path.exists() || !bundled_path.exists() {
        return false;
    }
    let (Ok(installed_meta), Ok(bundled_meta)) =
        (fs::metadata(&install_path), fs::metadata(&bundled_path))
    else {
        return false;
    };
    if installed_meta.len() != bundled_meta.len() {
        return false;
    }
    match (fs::read(&install_path), fs::read(&bundled_path)) {
        (Ok(installed), Ok(bundled)) => installed == bundled,
        _ => false,
    }
}

/// Attempt to install CLI on first run
pub fn ensure_cli_installed() {
    #[cfg(windows)]
    if let (Some(bundled), Some(installed)) = (bundled_cli_path(), cli_install_path()) {
        if let Err(error) = install_windows_helpers(&bundled, &installed) {
            log::warn!("Failed to install CLI helpers: {error}");
            return;
        }
    }
    // Matching CLI bytes do not prove its companion binaries are current.
    #[cfg(target_os = "linux")]
    if let (Some(bundled), Some(installed)) = (bundled_cli_path(), cli_install_path()) {
        if let Err(error) = install_linux_helpers(&bundled, &installed) {
            log::warn!("Failed to install CLI helpers: {error}");
            return;
        }
    }
    if is_cli_installed() {
        return;
    }

    match install_cli() {
        Ok(path) => {
            log::info!("CLI installed to {}", path.display());

            #[cfg(unix)]
            if !is_in_path(&path) {
                log::warn!(
                    "CLI installed but {} may not be in your PATH. Add it to ~/.zshrc or ~/.bashrc",
                    path.parent().unwrap_or(&path).display()
                );
            }
        }
        Err(e) => {
            log::warn!("Failed to install CLI: {}", e);
        }
    }
}

#[cfg(unix)]
fn is_in_path(cli_path: &Path) -> bool {
    if let Some(parent) = cli_path.parent() {
        if let Ok(path_var) = std::env::var("PATH") {
            return path_var.split(':').any(|p| p == parent.to_string_lossy());
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_cli_copy_keeps_agent_and_native_host_beside_the_cli() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("安装目录");
        let installed = root.path().join("Programs/AIPass/aipass.exe");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(installed.parent().unwrap()).unwrap();
        for name in ["aipass-agent.exe", "aipass-native-host.exe"] {
            fs::write(source.join(name), name).unwrap();
        }
        install_windows_helpers(&source.join("aipass.exe"), &installed).unwrap();
        fs::remove_dir_all(source).unwrap();
        for name in ["aipass-agent.exe", "aipass-native-host.exe"] {
            assert_eq!(
                fs::read(installed.with_file_name(name)).unwrap(),
                name.as_bytes()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn linux_helpers_survive_removing_the_appimage_mount_and_refresh_atomically() {
        let root = tempfile::tempdir().unwrap();
        let mount = root.path().join("mount/usr/lib/AIPass");
        fs::create_dir_all(&mount).unwrap();
        for name in ["aipass-agent", "aipass-native-host"] {
            fs::write(mount.join(name), b"original helper").unwrap();
        }
        let installed = root.path().join("home/.local/bin/aipass");
        install_linux_helpers(&mount.join("aipass"), &installed).unwrap();
        let agent = root.path().join("home/.local/lib/AIPass/aipass-agent");
        let old_inode = fs::File::open(&agent).unwrap();
        fs::write(mount.join("aipass-agent"), b"updated helper").unwrap();
        install_linux_helpers(&mount.join("aipass"), &installed).unwrap();
        fs::remove_dir_all(root.path().join("mount")).unwrap();
        assert_eq!(fs::read(&agent).unwrap(), b"updated helper");
        assert_ne!(
            fs::metadata(&agent).unwrap().permissions().mode() & 0o111,
            0
        );
        use std::io::Read;
        let mut previous = String::new();
        (&old_inode).read_to_string(&mut previous).unwrap();
        assert_eq!(previous, "original helper");
    }
}
