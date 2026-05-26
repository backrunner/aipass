use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

pub fn open_desktop_window(target: &str) -> Result<()> {
    let exe = std::env::current_exe().context("cannot determine current executable")?;
    let desktop_name = if cfg!(target_os = "windows") {
        "aipass-desktop.exe"
    } else {
        "aipass-desktop"
    };
    let candidate = exe.with_file_name(desktop_name);
    let binary = if candidate.exists() {
        candidate
    } else {
        PathBuf::from(desktop_name)
    };
    Command::new(binary)
        .env("AIPASS_WINDOW_TARGET", target)
        .spawn()
        .context("failed to open desktop companion")?;
    Ok(())
}
