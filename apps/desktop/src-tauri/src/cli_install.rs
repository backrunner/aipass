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
    let home = std::env::var("HOME").ok()?;
    let user_bin = PathBuf::from(home).join(".local/bin/aipass");

    if user_bin.parent().map(|p| p.exists()).unwrap_or(false) {
        return Some(user_bin);
    }

    Some(PathBuf::from("/usr/local/bin/aipass"))
}

/// Get the path to the bundled CLI binary
fn bundled_cli_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        // Bundled binaries ship as Tauri resources under Contents/Resources;
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
        let app_dir = exe_path.parent()?;
        Some(app_dir.join("aipass"))
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

    // Copy the binary
    fs::copy(&bundled_path, &install_path)
        .map_err(|e| format!("Failed to copy CLI binary: {}", e))?;

    // Set executable permissions on Unix
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&install_path)
            .map_err(|e| format!("Failed to read file permissions: {}", e))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&install_path, perms)
            .map_err(|e| format!("Failed to set executable permissions: {}", e))?;
    }

    Ok(install_path)
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
