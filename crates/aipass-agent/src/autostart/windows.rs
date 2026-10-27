use super::windows_registry::{Key, RUN_KEY};
use super::*;
use interprocess::local_socket::{prelude::*, GenericNamespaced, Stream};
use std::time::{Duration, Instant};

pub(super) fn install(agent_binary: &Path, vault_dir: &Path) -> Result<AgentAutostartStatus> {
    use crate::{client::AgentClientConfig, AgentClient};
    let binary = std::fs::canonicalize(agent_binary)?;
    anyhow::ensure!(binary.is_file(), "Agent executable not found");
    let vault = crate::canonical_vault_dir(vault_dir)?;
    let command = super::windows_command::command(&display_path(&binary)?, &display_path(&vault)?)?;
    super::windows_registry::ensure_login(&crate::agent_service_name(&vault)?, &command)?;
    let client = AgentClient::new(AgentClientConfig::for_vault(vault.clone())?);
    // Reuse serialized launch/protocol replacement rather than spawning a
    // second candidate while the first one is still binding its pipe.
    client.ensure_running_for_desktop_companion()?;
    let mut status = query(&vault)?;
    status.agent_binary = Some(binary);
    Ok(status)
}

pub(super) fn uninstall(vault_dir: &Path) -> Result<AgentAutostartStatus> {
    if let Some(key) = Key::open(RUN_KEY, false)? {
        key.remove(&crate::agent_service_name(vault_dir)?)?;
    }
    stop(vault_dir)
}

pub(super) fn stop(vault_dir: &Path) -> Result<AgentAutostartStatus> {
    stop_agent(vault_dir)?;
    query(vault_dir)
}

fn stop_agent(vault_dir: &Path) -> Result<()> {
    use aipass_agent_protocol::{AgentRequest, SessionStatus};
    let client = crate::AgentClient::for_vault(vault_dir.to_path_buf())?;
    // A protocol mismatch is a live resident, not evidence that it stopped.
    // Retire only an older Agent through the existing authenticated handshake.
    if let Ok(response) = client.request_raw(&AgentRequest::SessionStatus) {
        if response.protocol_version < aipass_agent_protocol::AGENT_PROTOCOL_VERSION {
            client.shutdown_older_agent(response.protocol_version)?;
        } else {
            client.shutdown()?;
        }
        let deadline = Instant::now() + Duration::from_secs(8);
        while client
            .request::<SessionStatus>(&AgentRequest::SessionStatus)
            .is_ok()
        {
            anyhow::ensure!(Instant::now() < deadline, "Agent did not stop");
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    Ok(())
}

pub(super) fn query(vault_dir: &Path) -> Result<AgentAutostartStatus> {
    use aipass_agent_protocol::{AgentRequest, SessionStatus};
    let name = crate::agent_service_name(vault_dir)?;
    let registered = match Key::read_only(RUN_KEY)? {
        Some(key) => key.read(&name)?.is_some(),
        None => false,
    };
    Ok(AgentAutostartStatus {
        service_name: name,
        registered,
        running: registered
            && crate::AgentClient::for_vault(vault_dir.to_path_buf())?
                .request::<SessionStatus>(&AgentRequest::SessionStatus)
                .is_ok(),
        install_path: Some(PathBuf::from(format!(r"HKCU\{RUN_KEY}"))),
        supervisor_path: None,
        agent_binary: None,
    })
}

pub(super) fn install_tray(desktop_binary: &Path, vault_dir: &Path) -> Result<TrayAutostartStatus> {
    let binary = std::fs::canonicalize(desktop_binary)?;
    anyhow::ensure!(binary.is_file(), "desktop executable not found");
    let vault = crate::canonical_vault_dir(vault_dir)?;
    let command =
        super::windows_command::desktop_command(&display_path(&binary)?, &display_path(&vault)?)?;
    super::windows_registry::ensure_login(&tray_service_name(&vault)?, &command)?;
    let mut status = query_tray(&vault)?;
    status.desktop_binary = Some(binary);
    Ok(status)
}

pub(super) fn uninstall_tray(vault_dir: &Path) -> Result<TrayAutostartStatus> {
    if let Some(key) = Key::open(RUN_KEY, false)? {
        key.remove(&tray_service_name(vault_dir)?)?;
    }
    stop_tray(vault_dir)
}

pub(super) fn stop_tray(vault_dir: &Path) -> Result<TrayAutostartStatus> {
    if let Ok(mut stream) = connect_tray() {
        crate::ipc::verify_pipe_owner(&stream)?;
        let process = crate::windows_process::peer_process(&stream)?;
        crate::ipc::framing::write_frame_with_deadline(
            &mut stream,
            &serde_json::json!({
                "version": "0.0.0", "target": "tray", "command": "quit"
            }),
            Duration::from_secs(2),
        )?;
        let _: serde_json::Value =
            crate::ipc::framing::read_frame_with_deadline(&mut stream, Duration::from_secs(2))?;
        drop(stream);
        if let Some(process) = process {
            crate::windows_process::wait_for_exit(&process)?;
        }
    }
    query_tray(vault_dir)
}

pub(super) fn query_tray(vault_dir: &Path) -> Result<TrayAutostartStatus> {
    let name = tray_service_name(vault_dir)?;
    let registered = match Key::read_only(RUN_KEY)? {
        Some(key) => key.read(&name)?.is_some(),
        None => false,
    };
    let running = connect_tray()
        .and_then(|mut stream| status(&mut stream))
        .is_ok();
    Ok(TrayAutostartStatus {
        service_name: name,
        registered,
        running,
        install_path: Some(PathBuf::from(format!(r"HKCU\{RUN_KEY}"))),
        supervisor_path: None,
        desktop_binary: None,
    })
}

fn connect_tray() -> Result<Stream> {
    let name = crate::desktop::windows_singleton_name("dev.aipass.desktop.tray")?;
    Ok(Stream::connect(name.to_ns_name::<GenericNamespaced>()?)?)
}

fn status(stream: &mut Stream) -> Result<()> {
    crate::ipc::verify_pipe_owner(stream)?;
    crate::ipc::framing::write_frame_with_deadline(
        stream,
        &serde_json::json!({
            "version":"0.0.0", "target":"tray", "command":"status"
        }),
        Duration::from_secs(2),
    )?;
    let response: serde_json::Value =
        crate::ipc::framing::read_frame_with_deadline(stream, Duration::from_secs(2))?;
    anyhow::ensure!(
        response["action"] == "useExisting" && response["version"].is_string(),
        "invalid desktop status response"
    );
    Ok(())
}

fn display_path(path: &Path) -> Result<String> {
    // Installers can launch via 8.3 paths while Run entries contain the long
    // canonical path. Resolve even a missing sibling through its parent.
    let path = crate::canonical_vault_dir(path)?;
    let path = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("login path is not valid Unicode"))?;
    // canonicalize adds a verbatim prefix; Explorer's startup launcher expects
    // normal drive/UNC paths. Do not lossy-convert Chinese or other Unicode.
    Ok(if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(path).to_string()
    })
}

pub(super) fn uninstall_entries(desktop: &Path) -> Result<()> {
    stop_owned_tray(desktop)?;
    let desktop = display_path(desktop)?;
    let agent = display_path(&Path::new(&desktop).with_file_name("aipass-agent.exe"))?;
    if let Some(key) = Key::open(RUN_KEY, false)? {
        for name in key.names()? {
            let binary = if name.starts_with("com.aipass.tray.") {
                &desktop
            } else if name.starts_with("com.aipass.agent.") {
                &agent
            } else {
                continue;
            };
            let Some(value) = key.read(&name)? else {
                continue;
            };
            let Some(vault) = super::windows_command::vault_for_command(binary, &value) else {
                continue;
            };
            key.remove(&name)?;
            stop_agent(Path::new(&vault))?;
        }
    }
    Ok(())
}

pub(super) fn owned_vaults(agent: &Path) -> Result<Vec<PathBuf>> {
    let agent = display_path(agent)?;
    let desktop = display_path(&Path::new(&agent).with_file_name("aipass-desktop.exe"))?;
    let mut result = Vec::new();
    if let Some(key) = Key::read_only(RUN_KEY)? {
        for name in key.names()? {
            let binary = if name.starts_with("com.aipass.tray.") {
                &desktop
            } else if name.starts_with("com.aipass.agent.") {
                &agent
            } else {
                continue;
            };
            if let Some(command) = key.read(&name)? {
                if let Some(vault) = super::windows_command::vault_for_command(binary, &command) {
                    result.push(crate::canonical_vault_dir(vault)?);
                }
            }
        }
    }
    Ok(result)
}

fn stop_owned_tray(desktop: &Path) -> Result<()> {
    use interprocess::local_socket::traits::StreamCommon as _;
    use std::os::windows::{
        ffi::OsStringExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    };
    use windows_sys::Win32::{
        Foundation::WAIT_OBJECT_0,
        System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, WaitForSingleObject,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        },
    };
    let Ok(mut stream) = connect_tray() else {
        return Ok(());
    };
    crate::ipc::verify_pipe_owner(&stream)?;
    let pid = stream
        .peer_creds()?
        .pid()
        .ok_or_else(|| anyhow::anyhow!("desktop peer PID is missing"))?;
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    anyhow::ensure!(!handle.is_null(), "cannot inspect the running desktop");
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    let mut path = vec![0_u16; 32_768];
    let mut length = path.len() as u32;
    anyhow::ensure!(
        unsafe {
            QueryFullProcessImageNameW(handle.as_raw_handle(), 0, path.as_mut_ptr(), &mut length)
        } != 0,
        "cannot resolve the running desktop path"
    );
    let running = PathBuf::from(std::ffi::OsString::from_wide(&path[..length as usize]));
    if std::fs::canonicalize(running)? != std::fs::canonicalize(desktop)? {
        // Complete a harmless transaction even when this is another copy;
        // never leave a connect-and-discard probe in the listener's queue.
        status(&mut stream)?;
        return Ok(());
    }
    crate::ipc::framing::write_frame_with_deadline(
        &mut stream,
        &serde_json::json!({
            "version":"0.0.0", "target":"tray", "command":"quit"
        }),
        Duration::from_secs(2),
    )?;
    let _: serde_json::Value =
        crate::ipc::framing::read_frame_with_deadline(&mut stream, Duration::from_secs(2))?;
    anyhow::ensure!(
        unsafe { WaitForSingleObject(handle.as_raw_handle(), 8000) } == WAIT_OBJECT_0,
        "desktop did not stop before uninstall"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aipass_agent_protocol::{
        AgentErrorCode, AgentRequest, AgentResponse, AuthenticatedAgentRequest,
        AGENT_PROTOCOL_VERSION,
    };

    #[test]
    fn executable_ownership_uses_canonical_paths_and_missing_siblings() {
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join("aipass-agent.exe");
        fs::write(&binary, "fixture").unwrap();
        let canonical = fs::canonicalize(&binary).unwrap();
        let alias = root.path().join(".").join("aipass-agent.exe");
        assert_eq!(
            display_path(&alias).unwrap(),
            display_path(&canonical).unwrap()
        );
        let value = super::super::windows_command::command(
            &display_path(&canonical).unwrap(),
            "C:\\fixture-vault",
        )
        .unwrap();
        assert_eq!(
            super::super::windows_command::vault_for_command(
                &display_path(&alias).unwrap(),
                &value
            )
            .as_deref(),
            Some("C:\\fixture-vault")
        );
        let missing = root.path().join(".").join("aipass-desktop.exe");
        let expected = canonical.with_file_name("aipass-desktop.exe");
        assert_eq!(
            display_path(&missing).unwrap(),
            display_path(&expected).unwrap()
        );
    }

    #[test]
    fn uninstall_stops_an_older_authenticated_agent_instead_of_ignoring_the_mismatch() {
        let root = tempfile::tempdir().unwrap();
        let vault = root.path().join("vault");
        let token = crate::ipc::load_or_create_auth_token(&vault).unwrap();
        let listener = crate::ipc::listen(&vault).unwrap();
        let version = AGENT_PROTOCOL_VERSION - 1;
        let server = std::thread::spawn(move || {
            for shutdown in [false, true] {
                let mut stream = listener.accept().unwrap();
                let payload: AuthenticatedAgentRequest =
                    crate::ipc::framing::read_frame_with_deadline(
                        &mut stream,
                        Duration::from_secs(5),
                    )
                    .unwrap();
                assert_eq!(payload.auth_token.expose(), token.expose());
                assert_eq!(
                    payload.protocol_version,
                    if shutdown {
                        version
                    } else {
                        AGENT_PROTOCOL_VERSION
                    }
                );
                assert!(if shutdown {
                    matches!(payload.request, AgentRequest::AgentShutdown)
                } else {
                    matches!(payload.request, AgentRequest::SessionStatus)
                });
                let mut response = if shutdown {
                    AgentResponse::empty()
                } else {
                    AgentResponse::error(
                        AgentErrorCode::ValidationFailed,
                        "unsupported agent protocol version",
                    )
                };
                response.protocol_version = version;
                crate::ipc::framing::write_frame_with_deadline(
                    &mut stream,
                    &response,
                    Duration::from_secs(5),
                )
                .unwrap();
            }
        });
        stop_agent(&vault).unwrap();
        server.join().unwrap();
        crate::ipc::clear_auth_token(&vault).unwrap();
    }
}
