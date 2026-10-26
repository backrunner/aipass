//! Freedesktop login startup for sessions without a systemd user manager.
use super::*;

fn paths(vault: &Path) -> Result<(PathBuf, PathBuf)> {
    let home = home_dir()?;
    let name = agent_service_name(vault)?;
    let config = crate::paths::linux_config_home(&home);
    Ok((
        config.join("autostart").join(format!("{name}.desktop")),
        config.join("aipass/autostart").join(format!("{name}.sh")),
    ))
}

pub(super) fn registered(vault: &Path) -> Result<bool> {
    Ok(paths(vault)?.0.is_file())
}

fn script(binary: &Path, vault: &Path) -> String {
    format!(
        "#!/bin/sh\nexec {} --vault {}\n",
        shell_quote(&binary.to_string_lossy()),
        shell_quote(&vault.to_string_lossy()),
    )
}

pub(super) fn install(binary: &Path, vault: &Path) -> Result<AgentAutostartStatus> {
    let binary = absolute_path(binary);
    anyhow::ensure!(
        binary.is_file(),
        "agent binary not found: {}",
        binary.display()
    );
    let vault = absolute_path(vault);
    let (desktop, supervisor) = paths(&vault)?;
    fs::create_dir_all(desktop.parent().context("invalid desktop entry path")?)?;
    fs::create_dir_all(supervisor.parent().context("invalid supervisor path")?)?;
    write_supervisor(&supervisor, &script(&binary, &vault))?;
    atomic_write_bytes(
        &desktop,
        super::super::linux_tray::desktop_entry(&supervisor, "AIPass Agent").as_bytes(),
    )?;
    if running(&vault) {
        let client = crate::client::AgentClient::for_vault(vault.clone())?;
        let response = client.request_raw(&aipass_agent_protocol::AgentRequest::AgentShutdown)?;
        anyhow::ensure!(response.ok, "Agent rejected shutdown during reinstall");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while running(&vault) {
            anyhow::ensure!(
                std::time::Instant::now() < deadline,
                "Agent did not stop during reinstall"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    {
        let mut child = Command::new(&supervisor)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("failed to start login-session Agent")?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
    let mut status = query(&vault)?;
    status.agent_binary = Some(binary);
    Ok(status)
}

fn running(vault: &Path) -> bool {
    crate::client::AgentClient::for_vault(vault.to_path_buf())
        .and_then(|client| {
            client
                .request_raw(&aipass_agent_protocol::AgentRequest::SessionStatus)
                .map(|response| response.ok)
                .map_err(Into::into)
        })
        .unwrap_or(false)
}

pub(super) fn query(vault: &Path) -> Result<AgentAutostartStatus> {
    let (desktop, supervisor) = paths(vault)?;
    Ok(AgentAutostartStatus {
        service_name: agent_service_name(vault)?,
        registered: desktop.is_file() && supervisor.is_file(),
        running: running(vault),
        install_path: Some(desktop),
        supervisor_path: Some(supervisor),
        agent_binary: None,
    })
}

pub(super) fn uninstall(vault: &Path) -> Result<AgentAutostartStatus> {
    let (desktop, supervisor) = paths(vault)?;
    for path in [desktop, supervisor] {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    shutdown_agent(vault);
    query(vault)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_script_passes_literal_binary_and_vault_paths() {
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join("agent '$HOME% name");
        let output = root.path().join("output");
        fs::write(
            &binary,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\n",
                shell_quote(&output.to_string_lossy())
            ),
        )
        .unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        let vault = root.path().join("vault '$HOME% name");
        let supervisor = root.path().join("startup.sh");
        write_supervisor(&supervisor, &script(&binary, &vault)).unwrap();
        assert!(Command::new(&supervisor).status().unwrap().success());
        assert_eq!(
            fs::read_to_string(output).unwrap(),
            format!("--vault\n{}\n", vault.display())
        );
    }
}
