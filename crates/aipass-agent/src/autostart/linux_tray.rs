//! XDG login startup. A desktop session supplies its display/DBus environment;
//! a systemd user service does not necessarily inherit either.
use super::*;

fn paths(vault: &Path) -> Result<(PathBuf, PathBuf)> {
    let home = home_dir()?;
    let name = tray_service_name(vault)?;
    Ok((
        crate::paths::linux_config_home(&home)
            .join("autostart")
            .join(format!("{name}.desktop")),
        home.join(".aipass/autostart").join(format!("{name}.sh")),
    ))
}

fn script(binary: &Path, vault: &Path) -> String {
    format!(
        "#!/bin/sh\nexport AIPASS_WINDOW_TARGET=tray\nexport AIPASS_VAULT_DIR={}\nexec {}\n",
        shell_quote(&vault.to_string_lossy()),
        shell_quote(&binary.to_string_lossy())
    )
}

fn entry(supervisor: &Path) -> String {
    desktop_entry(supervisor, "AIPass")
}

pub(super) fn desktop_entry(supervisor: &Path, name: &str) -> String {
    // Desktop entries have two escaping layers (value and Exec); field codes
    // must also be escaped, even inside a quoted argument.
    let argument = supervisor
        .to_string_lossy()
        .replace('\\', "\\\\\\\\")
        .replace('"', "\\\\\"")
        .replace('`', "\\\\`")
        .replace('$', "\\\\$")
        .replace('%', "%%")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    format!("[Desktop Entry]\nType=Application\nName={name}\nExec=\"{argument}\"\nTerminal=false\n")
}

fn socket_path() -> Result<PathBuf> {
    if let Some(root) = std::env::var_os("AIPASS_DESKTOP_RUNTIME_DIR") {
        return Ok(PathBuf::from(root).join("desktop-tray.sock"));
    }
    let dirs = crate::paths::project_dirs()?;
    let root = crate::paths::linux_runtime_root(&dirs.data_local_dir().join("runtime"));
    Ok(root.join("aipass-desktop/desktop-tray.sock"))
}

pub(crate) fn install_tray(binary: &Path, vault: &Path) -> Result<TrayAutostartStatus> {
    // The AppImage mount disappears when the process exits. Login must launch
    // the persistent archive, which will mount its own resources again.
    let binary = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_file())
        .unwrap_or_else(|| absolute_path(binary));
    anyhow::ensure!(
        binary.is_file(),
        "desktop binary not found: {}",
        binary.display()
    );
    let vault = absolute_path(vault);
    let (desktop, supervisor) = paths(&vault)?;
    fs::create_dir_all(supervisor.parent().context("invalid supervisor path")?)?;
    fs::create_dir_all(desktop.parent().context("invalid desktop entry path")?)?;
    write_supervisor(&supervisor, &script(&binary, &vault))?;
    atomic_write_bytes(&desktop, entry(&supervisor).as_bytes())?;
    let mut status = query_tray(&vault)?;
    status.desktop_binary = Some(binary);
    Ok(status)
}

pub(crate) fn query_tray(vault: &Path) -> Result<TrayAutostartStatus> {
    let (desktop, supervisor) = paths(vault)?;
    Ok(TrayAutostartStatus {
        service_name: tray_service_name(vault)?,
        registered: desktop.is_file() && supervisor.is_file(),
        running: std::os::unix::net::UnixStream::connect(socket_path()?).is_ok(),
        install_path: Some(desktop),
        supervisor_path: Some(supervisor),
        desktop_binary: None,
    })
}

pub(crate) fn stop_tray(vault: &Path) -> Result<TrayAutostartStatus> {
    use aipass_agent_protocol::{read_frame, write_frame};
    if let Ok(mut stream) = std::os::unix::net::UnixStream::connect(socket_path()?) {
        stream.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(std::time::Duration::from_secs(2)))?;
        write_frame(
            &mut stream,
            &serde_json::json!({
                "version": "0.0.0", "target": "tray", "command": "quit"
            }),
        )?;
        let _: serde_json::Value = read_frame(&mut stream)?;
    }
    query_tray(vault)
}

pub(crate) fn uninstall_tray(vault: &Path) -> Result<TrayAutostartStatus> {
    let (desktop, supervisor) = paths(vault)?;
    for path in [desktop, supervisor] {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
    }
    stop_tray(vault)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_script_preserves_paths_without_shell_expansion() {
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join("app '$HOME% name");
        let output = root.path().join("output");
        fs::write(&binary, format!("#!/bin/sh\nprintf '%s\\n%s' \"$AIPASS_WINDOW_TARGET\" \"$AIPASS_VAULT_DIR\" > {}\n", shell_quote(&output.to_string_lossy()))).unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        let vault = root.path().join("vault '$HOME% name");
        let supervisor = root.path().join("startup.sh");
        write_supervisor(&supervisor, &script(&binary, &vault)).unwrap();
        assert!(std::process::Command::new("sh")
            .arg(&supervisor)
            .status()
            .unwrap()
            .success());
        assert_eq!(
            fs::read_to_string(output).unwrap(),
            format!("tray\n{}", vault.display())
        );
    }

    #[test]
    fn desktop_exec_escapes_field_codes_and_both_value_layers() {
        let contents = entry(Path::new("/tmp/a b%$`\"\\.sh"));
        assert!(contents.contains("a b%%\\\\$\\\\`\\\\\"\\\\\\\\.sh"));
        assert!(!contents.contains("sh -c"));
    }
}
