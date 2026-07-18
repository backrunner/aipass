use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const WINDOW_TARGET_ENV: &str = "AIPASS_WINDOW_TARGET";
pub const VAULT_DIR_ENV: &str = "AIPASS_VAULT_DIR";
pub const SUPPRESS_TRAY_ENV: &str = "AIPASS_AGENT_SUPPRESS_TRAY";
pub const TRAY_WINDOW_TARGET: &str = "tray";
pub const RELEASE_DEEP_LINK_SCHEME: &str = "aipass";
pub const DEVELOPMENT_DEEP_LINK_SCHEME: &str = "aipass-dev";

pub fn open_desktop_window(target: &str, vault_dir: &Path) -> Result<()> {
    if should_open_desktop_url(target, vault_dir) && open_desktop_url(target).is_ok() {
        return Ok(());
    }

    // This path preserves the launch target and custom vault environment.
    Command::new(desktop_binary())
        .env(WINDOW_TARGET_ENV, target)
        .env(VAULT_DIR_ENV, vault_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to open desktop companion")?;
    Ok(())
}

fn should_open_desktop_url(target: &str, vault_dir: &Path) -> bool {
    if target == TRAY_WINDOW_TARGET {
        return false;
    }
    let Ok(default_vault_dir) = crate::paths::default_vault_dir() else {
        return false;
    };
    match (
        crate::paths::canonical_vault_dir(default_vault_dir),
        crate::paths::canonical_vault_dir(vault_dir),
    ) {
        (Ok(default_vault_dir), Ok(vault_dir)) => default_vault_dir == vault_dir,
        _ => false,
    }
}

fn open_desktop_url(target: &str) -> Result<()> {
    let target = match target {
        "main" | "unlock" | "quick-access" | "tray" => target,
        _ => "main",
    };
    let url = format!("{}://launch/{target}", desktop_deep_link_scheme());
    let mut command = if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg(&url);
        command
    } else if cfg!(target_os = "windows") {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", &url]);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(&url);
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
        .spawn()
        .map(|_| ())
        .context("failed to open AIPass URL")
}

fn desktop_deep_link_scheme() -> &'static str {
    if cfg!(debug_assertions) {
        DEVELOPMENT_DEEP_LINK_SCHEME
    } else {
        RELEASE_DEEP_LINK_SCHEME
    }
}

pub fn tray_launch_suppressed() -> bool {
    std::env::var_os(SUPPRESS_TRAY_ENV).is_some_and(|value| value != "0")
}

fn desktop_binary() -> PathBuf {
    desktop_binary_candidates()
        .into_iter()
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| PathBuf::from(desktop_binary_names()[0]))
}

fn desktop_binary_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        for name in desktop_binary_names() {
            push_unique(&mut candidates, exe.with_file_name(name));
        }
        #[cfg(target_os = "macos")]
        if let Some(resources_dir) = exe.parent() {
            if resources_dir
                .file_name()
                .is_some_and(|name| name == "Resources")
            {
                if let Some(contents_dir) = resources_dir.parent() {
                    for name in desktop_binary_names() {
                        push_unique(&mut candidates, contents_dir.join("MacOS").join(name));
                    }
                }
            }
        }
    }
    for name in desktop_binary_names() {
        push_unique(&mut candidates, PathBuf::from(name));
    }
    candidates
}

fn desktop_binary_names() -> &'static [&'static str] {
    if cfg!(target_os = "windows") {
        &["aipass-desktop.exe", "AIPass.exe"]
    } else if cfg!(target_os = "macos") {
        &["aipass-desktop", "AIPass"]
    } else {
        &["aipass-desktop"]
    }
}

fn push_unique(candidates: &mut Vec<PathBuf>, path: PathBuf) {
    if !candidates.iter().any(|candidate| candidate == &path) {
        candidates.push(path);
    }
}

#[cfg(test)]
mod tests {
    use super::{desktop_deep_link_scheme, DEVELOPMENT_DEEP_LINK_SCHEME};

    #[test]
    fn debug_builds_use_the_development_deep_link_scheme() {
        assert_eq!(desktop_deep_link_scheme(), DEVELOPMENT_DEEP_LINK_SCHEME);
    }
}
