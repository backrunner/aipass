use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const WINDOW_TARGET_ENV: &str = "AIPASS_WINDOW_TARGET";
pub const VAULT_DIR_ENV: &str = "AIPASS_VAULT_DIR";
pub const SUPPRESS_TRAY_ENV: &str = "AIPASS_AGENT_SUPPRESS_TRAY";
pub const TRAY_WINDOW_TARGET: &str = "tray";

pub fn open_desktop_window(target: &str, vault_dir: &Path) -> Result<()> {
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
