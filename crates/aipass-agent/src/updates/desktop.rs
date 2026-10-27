use crate::{desktop as companion, ipc, paths};
use aipass_agent_protocol::{
    AuthenticatedDesktopUpdate, DesktopUpdateOperation, UpdateOutcome, UPDATE_CONTROL_VERSION,
};
use anyhow::{Context, Result};
use interprocess::local_socket::{prelude::*, Stream};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

/// Kind is chosen by the owning desktop build, or explicitly by CLI fixtures.
pub fn socket_path(kind: &str) -> Result<PathBuf> {
    let root = if let Some(value) = std::env::var_os("AIPASS_DESKTOP_RUNTIME_DIR") {
        PathBuf::from(value)
    } else if cfg!(target_os = "macos") {
        PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?)
            .join(".aipass/run/aipass-desktop")
    } else {
        let dirs = paths::project_dirs()?;
        paths::linux_runtime_root(&dirs.data_local_dir().join("runtime")).join("aipass-desktop")
    };
    Ok(root.join(match kind {
        "development" => "desktop-dev-server.sock",
        "packaged-development" => "desktop-dev-bundle.sock",
        _ => "desktop-tray.sock",
    }))
}

pub fn request(
    vault: &Path,
    kind: &str,
    request: DesktopUpdateOperation,
    timeout: Duration,
) -> Result<UpdateOutcome> {
    #[cfg(windows)]
    let mut stream = {
        use interprocess::local_socket::GenericNamespaced;
        let prefix = match kind {
            "development" => "dev.aipass.desktop.dev-server",
            "packaged-development" => "dev.aipass.desktop.dev-bundle",
            _ => "dev.aipass.desktop.tray",
        };
        let name = companion::windows_singleton_name(prefix)?;
        let stream = Stream::connect(name.as_str().to_ns_name::<GenericNamespaced>()?)?;
        ipc::verify_pipe_owner(&stream)?;
        stream
    };
    #[cfg(not(windows))]
    let mut stream = {
        use interprocess::local_socket::GenericFilePath;
        Stream::connect(socket_path(kind)?.to_fs_name::<GenericFilePath>()?)?
    };
    #[derive(Serialize)]
    struct Envelope {
        version: &'static str,
        target: &'static str,
        command: &'static str,
        update: AuthenticatedDesktopUpdate,
    }
    let message = Envelope {
        version: env!("CARGO_PKG_VERSION"),
        target: "tray",
        command: "update",
        update: AuthenticatedDesktopUpdate {
            protocol_version: UPDATE_CONTROL_VERSION,
            vault_dir: paths::canonical_vault_dir(vault)?,
            auth_token: ipc::read_auth_token(vault)
                .context("desktop Agent is unavailable; start AIPass before updating")?,
            request,
        },
    };
    ipc::framing::write_frame_with_deadline(&mut stream, &message, Duration::from_secs(5))?;
    ipc::framing::read_frame_with_deadline(&mut stream, timeout)
        .context("desktop update control unavailable; install a version with CLI update support")
}

pub fn installed_binary() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let parent = exe.parent()?;
    let names = if cfg!(windows) {
        &["aipass-desktop.exe", "AIPass.exe"][..]
    } else {
        &["aipass-desktop"][..]
    };
    for name in names {
        let candidate = parent.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    #[cfg(target_os = "macos")]
    {
        let mut candidates = vec![
            parent.join("../MacOS/aipass-desktop"),
            PathBuf::from("/Applications/AIPass.app/Contents/MacOS/aipass-desktop"),
        ];
        if let Some(home) = std::env::var_os("HOME") {
            candidates.push(
                PathBuf::from(home).join("Applications/AIPass.app/Contents/MacOS/aipass-desktop"),
            );
        }
        candidates.into_iter().find(|path| path.is_file())
    }
    #[cfg(target_os = "linux")]
    for path in ["/usr/bin/aipass-desktop", "/usr/lib/AIPass/aipass-desktop"] {
        if Path::new(path).is_file() {
            return Some(PathBuf::from(path));
        }
    }
    #[cfg(not(target_os = "macos"))]
    None
}

pub fn start(binary: &Path, vault: &Path) -> Result<()> {
    use std::process::{Command, Stdio};
    let mut command = Command::new(binary);
    command
        .env(companion::WINDOW_TARGET_ENV, "tray")
        .env(companion::VAULT_DIR_ENV, vault)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    crate::windows_process::spawn(&command)?;
    #[cfg(not(windows))]
    command.spawn()?;
    Ok(())
}
