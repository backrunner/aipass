//! Official binary discovery and bounded capability probes.
use super::*;

pub(crate) fn executable(provider: &str) -> Result<PathBuf> {
    let name = match provider {
        "gemini-cli" => "gemini",
        v => v,
    };
    let mut directories =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect::<Vec<_>>();
    directories.extend([
        home()?.join(".local/bin"),
        home()?.join(".grok/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ]);
    if let Some(dir) = std::env::var_os("GROK_BIN_DIR").filter(|_| provider == "grok") {
        directories.insert(0, dir.into());
    }
    if let Ok(entries) = std::fs::read_dir(home()?.join(".nvm/versions/node")) {
        let mut nodes = entries
            .flatten()
            .map(|e| e.path().join("bin"))
            .collect::<Vec<_>>();
        nodes.sort();
        nodes.reverse();
        directories.extend(nodes);
    }
    #[cfg(windows)]
    if let Some(dir) = std::env::var_os("APPDATA") {
        directories.push(PathBuf::from(dir).join("npm"));
    }
    for dir in directories {
        #[cfg(windows)]
        let names = [format!("{name}.exe"), format!("{name}.cmd")];
        #[cfg(not(windows))]
        let names = [name.to_owned()];
        for name in names {
            let path = dir.join(name);
            if !path.is_file() {
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if path
                    .metadata()
                    .map_or(true, |m| m.permissions().mode() & 0o111 == 0)
                {
                    continue;
                }
            }
            // The community npm package named grok is a different product.
            if provider == "grok"
                && std::env::var_os("GROK_BIN_DIR").is_none()
                && !std::fs::canonicalize(&path)
                    .is_ok_and(|p| p.to_string_lossy().replace('\\', "/").contains("/.grok/"))
            {
                continue;
            }
            return Ok(path);
        }
    }
    Err(format!(
        "Install or update the official {name} CLI, then retry in AIPass"
    ))
}

pub(crate) fn status(provider: &str) -> aipass_agent_protocol::ClaudeCliStatus {
    let unavailable = |reason: &str| aipass_agent_protocol::ClaudeCliStatus {
        available: false,
        reason: Some(reason.into()),
        version: None,
    };
    if !matches!(provider, "codex" | "grok" | "copilot" | "gemini-cli") {
        return unavailable("unsupported");
    }
    let Ok(exe) = executable(provider) else {
        return unavailable("missing");
    };
    let probe = |args: &[&str]| -> Option<String> {
        let file = tempfile::tempfile().ok()?;
        let mut cmd = std::process::Command::new(&exe);
        let mut paths = vec![exe.parent()?.to_owned()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        cmd.env("PATH", std::env::join_paths(paths).ok()?)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(file.try_clone().ok()?)
            .stderr(std::process::Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        fn stop(child: &mut std::process::Child) {
            #[cfg(unix)]
            if let Some(pid) = rustix::process::Pid::from_raw(child.id() as i32) {
                let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
            }
            let _ = child.kill();
            let _ = child.wait();
        }
        let mut child = cmd.spawn().ok()?;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match child.try_wait() {
                Err(_) => {
                    stop(&mut child);
                    return None;
                }
                Ok(Some(status)) => {
                    if !status.success() {
                        return None;
                    }
                    break;
                }
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Ok(None) => {
                    stop(&mut child);
                    return None;
                }
            }
            if file.metadata().map_or(true, |m| m.len() > 128 * 1024) {
                stop(&mut child);
                return None;
            }
        }
        use std::io::{Read, Seek, SeekFrom};
        let mut file = file;
        file.seek(SeekFrom::Start(0)).ok()?;
        let mut value = String::new();
        file.take(128 * 1024).read_to_string(&mut value).ok()?;
        Some(value)
    };
    let Some(version) = probe(&["--version"]) else {
        return unavailable("unusable");
    };
    let args = if provider == "gemini-cli" {
        vec!["--help"]
    } else {
        vec!["login", "--help"]
    };
    let Some(help) = probe(&args) else {
        return unavailable("unsupported");
    };
    let supported = match provider {
        "codex" => version.contains("codex") && help.contains("Manage login"),
        "grok" => help.contains("Sign in to Grok") && help.contains("--device-auth"),
        "copilot" => help.contains("--web-flow"),
        "gemini-cli" => help.contains("Gemini CLI") && help.contains("--acp"),
        _ => false,
    };
    if !supported {
        return unavailable("unsupported");
    }
    aipass_agent_protocol::ClaudeCliStatus {
        available: true,
        reason: None,
        version: Some(version.trim().chars().take(100).collect()),
    }
}
