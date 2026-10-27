//! Rust-owned update operations for CLI installations and the desktop companion.
pub mod desktop;
mod install;
mod release;

use anyhow::{bail, Context, Result};
pub use release::{check_cli, CliRelease};
use std::path::{Path, PathBuf};

pub fn standalone_directory(executable: &Path) -> Result<PathBuf> {
    let executable = executable.canonicalize()?;
    if executable
        .components()
        .any(|part| part.as_os_str().to_string_lossy().ends_with(".app"))
    {
        bail!("use --target desktop to update a desktop application bundle");
    }
    let directory = executable.parent().context("CLI has no parent directory")?;
    for name in binary_names() {
        if !directory.join(name).is_file() {
            bail!("standalone update requires aipass, aipass-agent and aipass-native-host together; use --target desktop for a desktop installation");
        }
    }
    let desktop_names: &[&str] = if cfg!(windows) {
        &["aipass-desktop.exe", "AIPass.exe"]
    } else {
        &["aipass-desktop"]
    };
    if desktop_names
        .iter()
        .any(|name| directory.join(name).exists())
    {
        bail!("use --target desktop to update this managed installation");
    }
    Ok(directory.to_path_buf())
}

pub fn binary_names() -> [&'static str; 3] {
    if cfg!(windows) {
        ["aipass.exe", "aipass-agent.exe", "aipass-native-host.exe"]
    } else {
        ["aipass", "aipass-agent", "aipass-native-host"]
    }
}

pub fn install_cli(
    release: &CliRelease,
    executable: &Path,
    vault: &Path,
) -> Result<aipass_agent_protocol::UpdateOutcome> {
    install::run(release, &standalone_directory(executable)?, vault)
}
