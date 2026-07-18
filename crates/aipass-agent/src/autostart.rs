use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use aipass_storage::atomic_write_bytes;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::process::Command;

#[derive(Clone, Debug)]
pub struct AgentAutostartStatus {
    pub service_name: String,
    pub registered: bool,
    pub running: bool,
    pub install_path: Option<PathBuf>,
    pub supervisor_path: Option<PathBuf>,
    pub agent_binary: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct TrayAutostartStatus {
    pub service_name: String,
    pub registered: bool,
    pub running: bool,
    pub install_path: Option<PathBuf>,
    pub supervisor_path: Option<PathBuf>,
    pub desktop_binary: Option<PathBuf>,
}

pub fn install_autostart(agent_binary: &Path, vault_dir: &Path) -> Result<AgentAutostartStatus> {
    imp::install(agent_binary, vault_dir)
}

pub fn uninstall_autostart(vault_dir: &Path) -> Result<AgentAutostartStatus> {
    imp::uninstall(vault_dir)
}

pub fn stop_autostart(vault_dir: &Path) -> Result<AgentAutostartStatus> {
    imp::stop(vault_dir)
}

pub fn query_autostart(vault_dir: &Path) -> Result<AgentAutostartStatus> {
    imp::query(vault_dir)
}

pub fn install_tray_autostart(
    desktop_binary: &Path,
    vault_dir: &Path,
) -> Result<TrayAutostartStatus> {
    imp::install_tray(desktop_binary, vault_dir)
}

pub fn uninstall_tray_autostart(vault_dir: &Path) -> Result<TrayAutostartStatus> {
    imp::uninstall_tray(vault_dir)
}

pub fn stop_tray_autostart(vault_dir: &Path) -> Result<TrayAutostartStatus> {
    imp::stop_tray(vault_dir)
}

pub fn query_tray_autostart(vault_dir: &Path) -> Result<TrayAutostartStatus> {
    imp::query_tray(vault_dir)
}

fn shutdown_agent(vault_dir: &Path) {
    if let Ok(client) = crate::client::AgentClient::for_vault(vault_dir.to_path_buf()) {
        let _ = client.shutdown();
    }
}

fn tray_service_name(vault_dir: &Path) -> Result<String> {
    Ok(format!(
        "dev.aipass.desktop.tray.{}",
        crate::paths::namespace_for_vault_dir(vault_dir)?
    ))
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use crate::paths::agent_service_name;
    use aipass_agent_protocol::{read_frame, write_frame};
    use interprocess::local_socket::{prelude::*, GenericFilePath, Stream};

    pub(super) fn install(agent_binary: &Path, vault_dir: &Path) -> Result<AgentAutostartStatus> {
        let service_name = agent_service_name(vault_dir)?;
        let paths = macos_paths(&service_name)?;
        let agent_binary = absolute_path(agent_binary);
        let vault_dir = absolute_path(vault_dir);

        fs::create_dir_all(
            paths
                .supervisor_path
                .parent()
                .context("invalid supervisor path")?,
        )?;
        fs::create_dir_all(paths.log_dir.as_path())?;
        write_supervisor(
            &paths.supervisor_path,
            &macos_supervisor_script(
                &service_name,
                &paths.plist_path,
                &agent_binary,
                &vault_dir,
                &paths.out_log,
                &paths.err_log,
            ),
        )?;

        fs::create_dir_all(
            paths
                .plist_path
                .parent()
                .context("invalid LaunchAgent path")?,
        )?;
        let plist = macos_plist(
            &service_name,
            &paths.supervisor_path,
            &paths.supervisor_log,
            &paths.supervisor_err_log,
        );
        atomic_write_bytes(&paths.plist_path, plist.as_bytes())?;

        let _ = unload_launch_agent(&service_name, &paths.plist_path);
        load_launch_agent(&service_name, &paths.plist_path)?;

        Ok(AgentAutostartStatus {
            service_name: service_name.clone(),
            registered: paths.plist_path.exists(),
            running: launch_agent_running(&service_name),
            install_path: Some(paths.plist_path),
            supervisor_path: Some(paths.supervisor_path),
            agent_binary: Some(agent_binary),
        })
    }

    pub(super) fn uninstall(vault_dir: &Path) -> Result<AgentAutostartStatus> {
        let service_name = agent_service_name(vault_dir)?;
        let paths = macos_paths(&service_name)?;
        let _ = unload_launch_agent(&service_name, &paths.plist_path);
        shutdown_agent(vault_dir);
        let _ = fs::remove_file(&paths.plist_path);
        let _ = fs::remove_file(&paths.supervisor_path);
        Ok(AgentAutostartStatus {
            service_name,
            registered: false,
            running: false,
            install_path: Some(paths.plist_path),
            supervisor_path: Some(paths.supervisor_path),
            agent_binary: None,
        })
    }

    pub(super) fn stop(vault_dir: &Path) -> Result<AgentAutostartStatus> {
        let service_name = agent_service_name(vault_dir)?;
        let paths = macos_paths(&service_name)?;
        let _ = unload_launch_agent(&service_name, &paths.plist_path);
        shutdown_agent(vault_dir);
        Ok(AgentAutostartStatus {
            service_name,
            registered: paths.plist_path.exists(),
            running: false,
            install_path: Some(paths.plist_path),
            supervisor_path: Some(paths.supervisor_path),
            agent_binary: None,
        })
    }

    pub(super) fn query(vault_dir: &Path) -> Result<AgentAutostartStatus> {
        let service_name = agent_service_name(vault_dir)?;
        let paths = macos_paths(&service_name)?;
        Ok(AgentAutostartStatus {
            service_name: service_name.clone(),
            registered: paths.plist_path.exists(),
            running: launch_agent_running(&service_name),
            install_path: Some(paths.plist_path),
            supervisor_path: Some(paths.supervisor_path),
            agent_binary: None,
        })
    }

    pub(super) fn install_tray(
        desktop_binary: &Path,
        vault_dir: &Path,
    ) -> Result<TrayAutostartStatus> {
        let service_name = tray_service_name(vault_dir)?;
        let paths = macos_paths(&service_name)?;
        let desktop_binary = absolute_path(desktop_binary);
        let vault_dir = absolute_path(vault_dir);

        fs::create_dir_all(
            paths
                .supervisor_path
                .parent()
                .context("invalid tray supervisor path")?,
        )?;
        fs::create_dir_all(paths.log_dir.as_path())?;
        write_supervisor(
            &paths.supervisor_path,
            &macos_tray_supervisor_script(
                &service_name,
                &paths.plist_path,
                &desktop_binary,
                &vault_dir,
                &paths.out_log,
                &paths.err_log,
                &paths.stop_child_path,
            )?,
        )?;

        fs::create_dir_all(
            paths
                .plist_path
                .parent()
                .context("invalid tray LaunchAgent path")?,
        )?;
        let plist = macos_plist(
            &service_name,
            &paths.supervisor_path,
            &paths.supervisor_log,
            &paths.supervisor_err_log,
        );
        atomic_write_bytes(&paths.plist_path, plist.as_bytes())?;

        let _ = fs::remove_file(&paths.stop_child_path);
        let _ = unload_launch_agent(&service_name, &paths.plist_path);
        load_launch_agent(&service_name, &paths.plist_path)?;

        Ok(TrayAutostartStatus {
            service_name: service_name.clone(),
            registered: paths.plist_path.exists(),
            running: launch_agent_running(&service_name),
            install_path: Some(paths.plist_path),
            supervisor_path: Some(paths.supervisor_path),
            desktop_binary: Some(desktop_binary),
        })
    }

    pub(super) fn uninstall_tray(vault_dir: &Path) -> Result<TrayAutostartStatus> {
        let service_name = tray_service_name(vault_dir)?;
        let paths = macos_paths(&service_name)?;
        let _ = write_stop_child_flag(&paths.stop_child_path);
        let _ = unload_launch_agent(&service_name, &paths.plist_path);
        request_tray_exit();
        let _ = fs::remove_file(&paths.plist_path);
        let _ = fs::remove_file(&paths.supervisor_path);
        Ok(TrayAutostartStatus {
            service_name,
            registered: false,
            running: false,
            install_path: Some(paths.plist_path),
            supervisor_path: Some(paths.supervisor_path),
            desktop_binary: None,
        })
    }

    pub(super) fn stop_tray(vault_dir: &Path) -> Result<TrayAutostartStatus> {
        let service_name = tray_service_name(vault_dir)?;
        let paths = macos_paths(&service_name)?;
        let _ = write_stop_child_flag(&paths.stop_child_path);
        let _ = unload_launch_agent(&service_name, &paths.plist_path);
        request_tray_exit();
        Ok(TrayAutostartStatus {
            service_name,
            registered: paths.plist_path.exists(),
            running: false,
            install_path: Some(paths.plist_path),
            supervisor_path: Some(paths.supervisor_path),
            desktop_binary: None,
        })
    }

    pub(super) fn query_tray(vault_dir: &Path) -> Result<TrayAutostartStatus> {
        let service_name = tray_service_name(vault_dir)?;
        let paths = macos_paths(&service_name)?;
        Ok(TrayAutostartStatus {
            service_name: service_name.clone(),
            registered: paths.plist_path.exists(),
            running: launch_agent_running(&service_name),
            install_path: Some(paths.plist_path),
            supervisor_path: Some(paths.supervisor_path),
            desktop_binary: None,
        })
    }

    struct MacosAutostartPaths {
        plist_path: PathBuf,
        supervisor_path: PathBuf,
        log_dir: PathBuf,
        out_log: PathBuf,
        err_log: PathBuf,
        supervisor_log: PathBuf,
        supervisor_err_log: PathBuf,
        stop_child_path: PathBuf,
    }

    fn macos_paths(service_name: &str) -> Result<MacosAutostartPaths> {
        let home = home_dir()?;
        let log_dir = home.join("Library").join("Logs").join("AIPass");
        Ok(MacosAutostartPaths {
            plist_path: home
                .join("Library")
                .join("LaunchAgents")
                .join(format!("{service_name}.plist")),
            supervisor_path: home
                .join(".aipass")
                .join("autostart")
                .join(format!("{service_name}.sh")),
            out_log: log_dir.join(format!("{service_name}.out.log")),
            err_log: log_dir.join(format!("{service_name}.err.log")),
            supervisor_log: log_dir.join(format!("{service_name}.supervisor.out.log")),
            supervisor_err_log: log_dir.join(format!("{service_name}.supervisor.err.log")),
            stop_child_path: home
                .join(".aipass")
                .join("autostart")
                .join(format!("{service_name}.stop-child")),
            log_dir,
        })
    }

    fn macos_plist(
        service_name: &str,
        supervisor_path: &Path,
        supervisor_log: &Path,
        supervisor_err_log: &Path,
    ) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{}</string>
  <key>StandardErrorPath</key>
  <string>{}</string>
</dict>
</plist>
"#,
            xml_escape(service_name),
            xml_escape(&supervisor_path.display().to_string()),
            xml_escape(&supervisor_log.display().to_string()),
            xml_escape(&supervisor_err_log.display().to_string()),
        )
    }

    fn macos_supervisor_script(
        service_name: &str,
        plist_path: &Path,
        agent_binary: &Path,
        vault_dir: &Path,
        out_log: &Path,
        err_log: &Path,
    ) -> String {
        format!(
            r#"#!/bin/sh
set -u

LABEL={}
PLIST={}
AGENT={}
VAULT={}
OUT_LOG={}
ERR_LOG={}
child=""

cleanup() {{
  rm -f "$PLIST" "$0"
  launchctl remove "$LABEL" >/dev/null 2>&1 || true
}}

terminate() {{
  if [ -n "$child" ]; then
    kill "$child" >/dev/null 2>&1 || true
    wait "$child" >/dev/null 2>&1 || true
  fi
  exit 0
}}

trap terminate TERM INT

if [ ! -x "$AGENT" ]; then
  cleanup
  exit 0
fi

agent_running() {{
  pgrep -f "aipass-agent --vault $VAULT" >/dev/null 2>&1
}}

wait_for_existing_agent() {{
  while agent_running; do
    sleep 2
  done
}}

while [ -x "$AGENT" ]; do
  wait_for_existing_agent
  AIPASS_AGENT_SUPPRESS_TRAY=1 "$AGENT" --vault "$VAULT" >>"$OUT_LOG" 2>>"$ERR_LOG" &
  child=$!
  while kill -0 "$child" >/dev/null 2>&1; do
    if [ ! -x "$AGENT" ]; then
      kill "$child" >/dev/null 2>&1 || true
      wait "$child" >/dev/null 2>&1 || true
      cleanup
      exit 0
    fi
    sleep 10
  done
  wait "$child" >/dev/null 2>&1 || true
  child=""
  sleep 2
done

cleanup
"#,
            shell_quote(service_name),
            shell_quote(&plist_path.display().to_string()),
            shell_quote(&agent_binary.display().to_string()),
            shell_quote(&vault_dir.display().to_string()),
            shell_quote(&out_log.display().to_string()),
            shell_quote(&err_log.display().to_string()),
        )
    }

    fn macos_tray_supervisor_script(
        service_name: &str,
        plist_path: &Path,
        desktop_binary: &Path,
        vault_dir: &Path,
        out_log: &Path,
        err_log: &Path,
        stop_child_path: &Path,
    ) -> Result<String> {
        let desktop_runtime_dir = desktop_runtime_dir()?;
        let singleton_socket = desktop_runtime_dir.join("desktop-tray.sock");
        Ok(format!(
            r#"#!/bin/sh
set -u

LABEL={}
PLIST={}
DESKTOP={}
VAULT={}
OUT_LOG={}
ERR_LOG={}
SINGLETON_SOCKET={}
STOP_CHILD={}
child=""

cleanup() {{
  rm -f "$PLIST" "$0" "$STOP_CHILD"
  launchctl remove "$LABEL" >/dev/null 2>&1 || true
}}

terminate() {{
  if [ -f "$STOP_CHILD" ]; then
    rm -f "$STOP_CHILD"
    if [ -n "$child" ]; then
      kill "$child" >/dev/null 2>&1 || true
      wait "$child" >/dev/null 2>&1 || true
    fi
  fi
  exit 0
}}

desktop_running() {{
  pgrep -f "$DESKTOP" >/dev/null 2>&1
}}

wait_for_existing_desktop() {{
  while desktop_running; do
    sleep 2
  done
}}

trap terminate TERM INT

if [ ! -x "$DESKTOP" ]; then
  cleanup
  exit 0
fi

mkdir -p "$(dirname "$SINGLETON_SOCKET")" >/dev/null 2>&1 || true

while [ -x "$DESKTOP" ]; do
  wait_for_existing_desktop
  AIPASS_WINDOW_TARGET=tray AIPASS_VAULT_DIR="$VAULT" "$DESKTOP" >>"$OUT_LOG" 2>>"$ERR_LOG" &
  child=$!
  while kill -0 "$child" >/dev/null 2>&1; do
    if [ ! -x "$DESKTOP" ]; then
      kill "$child" >/dev/null 2>&1 || true
      wait "$child" >/dev/null 2>&1 || true
      cleanup
      exit 0
    fi
    sleep 10
  done
  wait "$child" >/dev/null 2>&1 || true
  child=""
  sleep 2
done

cleanup
"#,
            shell_quote(service_name),
            shell_quote(&plist_path.display().to_string()),
            shell_quote(&desktop_binary.display().to_string()),
            shell_quote(&vault_dir.display().to_string()),
            shell_quote(&out_log.display().to_string()),
            shell_quote(&err_log.display().to_string()),
            shell_quote(&singleton_socket.display().to_string()),
            shell_quote(&stop_child_path.display().to_string()),
        ))
    }

    fn write_stop_child_flag(path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(atomic_write_bytes(path, b"stop\n")?)
    }

    fn request_tray_exit() {
        let Ok(socket_path) = desktop_runtime_dir().map(|dir| dir.join("desktop-tray.sock")) else {
            return;
        };
        let Ok(name) = socket_path.to_fs_name::<GenericFilePath>() else {
            return;
        };
        let Ok(mut stream) = Stream::connect(name) else {
            return;
        };
        let request = serde_json::json!({
            "version": "0.0.0",
            "target": "tray",
            "command": "quit",
        });
        if write_frame(&mut stream, &request).is_ok() {
            let _: Result<serde_json::Value> = read_frame(&mut stream);
        }
    }

    fn desktop_runtime_dir() -> Result<PathBuf> {
        if let Some(explicit) = std::env::var_os("AIPASS_DESKTOP_RUNTIME_DIR") {
            Ok(PathBuf::from(explicit))
        } else {
            Ok(home_dir()?
                .join(".aipass")
                .join("run")
                .join("aipass-desktop"))
        }
    }

    fn load_launch_agent(service_name: &str, plist_path: &Path) -> Result<()> {
        let domain = launchctl_domain()?;
        let plist = plist_path.to_string_lossy().into_owned();
        let bootstrap = Command::new("launchctl")
            .args(["bootstrap", domain.as_str(), plist.as_str()])
            .status();
        if !matches!(bootstrap, Ok(status) if status.success()) {
            let status = Command::new("launchctl")
                .args(["load", "-w", plist.as_str()])
                .status()
                .context("failed to load AIPass LaunchAgent")?;
            if !status.success() {
                anyhow::bail!("launchctl load -w failed");
            }
        }
        let service = format!("{domain}/{service_name}");
        let _ = Command::new("launchctl")
            .args(["enable", service.as_str()])
            .status();
        let _ = Command::new("launchctl")
            .args(["kickstart", "-k", service.as_str()])
            .status();
        Ok(())
    }

    fn unload_launch_agent(service_name: &str, plist_path: &Path) -> Result<()> {
        let plist = plist_path.to_string_lossy().into_owned();
        if let Ok(domain) = launchctl_domain() {
            let _ = Command::new("launchctl")
                .args(["bootout", domain.as_str(), plist.as_str()])
                .status();
        }
        let _ = Command::new("launchctl")
            .args(["remove", service_name])
            .status();
        let _ = Command::new("launchctl")
            .args(["unload", plist.as_str()])
            .status();
        Ok(())
    }

    fn launch_agent_running(service_name: &str) -> bool {
        let Ok(domain) = launchctl_domain() else {
            return false;
        };
        let service = format!("{domain}/{service_name}");
        Command::new("launchctl")
            .args(["print", service.as_str()])
            .status()
            .is_ok_and(|status| status.success())
    }

    fn launchctl_domain() -> Result<String> {
        let output = Command::new("id")
            .arg("-u")
            .output()
            .context("failed to determine current user id")?;
        if !output.status.success() {
            anyhow::bail!("id -u failed");
        }
        let uid = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(format!("gui/{uid}"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn macos_agent_supervisor_waits_for_existing_agent_and_suppresses_tray_launch() {
            let script = macos_supervisor_script(
                "dev.aipass.agent.test",
                Path::new("/tmp/dev.aipass.agent.test.plist"),
                Path::new("/tmp/aipass-agent"),
                Path::new("/tmp/aipass-vault"),
                Path::new("/tmp/agent.out.log"),
                Path::new("/tmp/agent.err.log"),
            );

            assert!(script.contains("pgrep -f \"aipass-agent --vault $VAULT\""));
            assert!(script.contains("wait_for_existing_agent"));
            assert!(script.contains("AIPASS_AGENT_SUPPRESS_TRAY=1 \"$AGENT\" --vault \"$VAULT\""));
        }

        #[test]
        fn macos_tray_supervisor_launches_tray_target_and_waits_for_existing_desktop() {
            let script = macos_tray_supervisor_script(
                "dev.aipass.desktop.tray.test",
                Path::new("/tmp/dev.aipass.desktop.tray.test.plist"),
                Path::new("/tmp/aipass-desktop"),
                Path::new("/tmp/aipass-vault"),
                Path::new("/tmp/tray.out.log"),
                Path::new("/tmp/tray.err.log"),
                Path::new("/tmp/tray.stop-child"),
            )
            .expect("tray supervisor script");

            assert!(script.contains("pgrep -f \"$DESKTOP\""));
            assert!(script.contains("wait_for_existing_desktop"));
            assert!(script
                .contains("AIPASS_WINDOW_TARGET=tray AIPASS_VAULT_DIR=\"$VAULT\" \"$DESKTOP\""));
            assert!(script.contains("STOP_CHILD="));
            assert!(script.contains("if [ -f \"$STOP_CHILD\" ]; then"));
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use crate::paths::agent_service_name;

    pub(super) fn install(agent_binary: &Path, vault_dir: &Path) -> Result<AgentAutostartStatus> {
        let service_name = agent_service_name(vault_dir)?;
        let paths = linux_paths(&service_name)?;
        let agent_binary = absolute_path(agent_binary);
        let vault_dir = absolute_path(vault_dir);

        fs::create_dir_all(
            paths
                .supervisor_path
                .parent()
                .context("invalid supervisor path")?,
        )?;
        fs::create_dir_all(paths.log_dir.as_path())?;
        write_supervisor(
            &paths.supervisor_path,
            &linux_supervisor_script(
                &paths.unit_name,
                &paths.unit_path,
                &agent_binary,
                &vault_dir,
                &paths.out_log,
                &paths.err_log,
            ),
        )?;

        fs::create_dir_all(
            paths
                .unit_path
                .parent()
                .context("invalid systemd unit path")?,
        )?;
        let unit = linux_unit(&service_name, &paths.supervisor_path);
        atomic_write_bytes(&paths.unit_path, unit.as_bytes())?;

        systemctl(["daemon-reload"])?;
        systemctl(["enable", "--now", paths.unit_name.as_str()])?;

        let running = systemctl_success(["is-active", "--quiet", paths.unit_name.as_str()]);
        Ok(AgentAutostartStatus {
            service_name: paths.unit_name,
            registered: paths.unit_path.exists(),
            running,
            install_path: Some(paths.unit_path),
            supervisor_path: Some(paths.supervisor_path),
            agent_binary: Some(agent_binary),
        })
    }

    pub(super) fn uninstall(vault_dir: &Path) -> Result<AgentAutostartStatus> {
        let service_name = agent_service_name(vault_dir)?;
        let paths = linux_paths(&service_name)?;
        let _ = systemctl(["disable", "--now", paths.unit_name.as_str()]);
        shutdown_agent(vault_dir);
        let _ = fs::remove_file(&paths.unit_path);
        let _ = fs::remove_file(&paths.supervisor_path);
        let _ = systemctl(["daemon-reload"]);
        Ok(AgentAutostartStatus {
            service_name: paths.unit_name,
            registered: false,
            running: false,
            install_path: Some(paths.unit_path),
            supervisor_path: Some(paths.supervisor_path),
            agent_binary: None,
        })
    }

    pub(super) fn stop(vault_dir: &Path) -> Result<AgentAutostartStatus> {
        let service_name = agent_service_name(vault_dir)?;
        let paths = linux_paths(&service_name)?;
        let _ = systemctl(["stop", paths.unit_name.as_str()]);
        shutdown_agent(vault_dir);
        Ok(AgentAutostartStatus {
            service_name: paths.unit_name,
            registered: paths.unit_path.exists(),
            running: false,
            install_path: Some(paths.unit_path),
            supervisor_path: Some(paths.supervisor_path),
            agent_binary: None,
        })
    }

    pub(super) fn query(vault_dir: &Path) -> Result<AgentAutostartStatus> {
        let service_name = agent_service_name(vault_dir)?;
        let paths = linux_paths(&service_name)?;
        Ok(AgentAutostartStatus {
            service_name: paths.unit_name.clone(),
            registered: paths.unit_path.exists()
                || systemctl_success(["is-enabled", "--quiet", paths.unit_name.as_str()]),
            running: systemctl_success(["is-active", "--quiet", paths.unit_name.as_str()]),
            install_path: Some(paths.unit_path),
            supervisor_path: Some(paths.supervisor_path),
            agent_binary: None,
        })
    }

    pub(super) fn install_tray(
        _desktop_binary: &Path,
        vault_dir: &Path,
    ) -> Result<TrayAutostartStatus> {
        unsupported_tray(vault_dir)
    }

    pub(super) fn uninstall_tray(vault_dir: &Path) -> Result<TrayAutostartStatus> {
        unsupported_tray(vault_dir)
    }

    pub(super) fn stop_tray(vault_dir: &Path) -> Result<TrayAutostartStatus> {
        unsupported_tray(vault_dir)
    }

    pub(super) fn query_tray(vault_dir: &Path) -> Result<TrayAutostartStatus> {
        unsupported_tray(vault_dir)
    }

    fn unsupported_tray(vault_dir: &Path) -> Result<TrayAutostartStatus> {
        Ok(TrayAutostartStatus {
            service_name: tray_service_name(vault_dir)?,
            registered: false,
            running: false,
            install_path: None,
            supervisor_path: None,
            desktop_binary: None,
        })
    }

    struct LinuxAutostartPaths {
        unit_name: String,
        unit_path: PathBuf,
        supervisor_path: PathBuf,
        log_dir: PathBuf,
        out_log: PathBuf,
        err_log: PathBuf,
    }

    fn linux_paths(service_name: &str) -> Result<LinuxAutostartPaths> {
        let home = home_dir()?;
        let unit_name = format!("aipass-agent-{service_name}.service");
        let log_dir = home.join(".aipass").join("logs");
        Ok(LinuxAutostartPaths {
            unit_path: home
                .join(".config")
                .join("systemd")
                .join("user")
                .join(&unit_name),
            supervisor_path: home
                .join(".aipass")
                .join("autostart")
                .join(format!("{service_name}.sh")),
            out_log: log_dir.join(format!("{service_name}.out.log")),
            err_log: log_dir.join(format!("{service_name}.err.log")),
            log_dir,
            unit_name,
        })
    }

    fn linux_unit(service_name: &str, supervisor_path: &Path) -> String {
        format!(
            r#"[Unit]
Description=AIPass Agent ({service_name})

[Service]
Type=simple
ExecStart={}
Restart=on-failure
RestartSec=2

[Install]
WantedBy=default.target
"#,
            systemd_quote(&supervisor_path.display().to_string()),
        )
    }

    fn linux_supervisor_script(
        unit_name: &str,
        unit_path: &Path,
        agent_binary: &Path,
        vault_dir: &Path,
        out_log: &Path,
        err_log: &Path,
    ) -> String {
        format!(
            r#"#!/bin/sh
set -u

UNIT={}
UNIT_PATH={}
AGENT={}
VAULT={}
OUT_LOG={}
ERR_LOG={}
child=""

cleanup() {{
  systemctl --user disable "$UNIT" >/dev/null 2>&1 || true
  rm -f "$UNIT_PATH" "$0"
  systemctl --user daemon-reload >/dev/null 2>&1 || true
}}

terminate() {{
  if [ -n "$child" ]; then
    kill "$child" >/dev/null 2>&1 || true
    wait "$child" >/dev/null 2>&1 || true
  fi
  exit 0
}}

trap terminate TERM INT

if [ ! -x "$AGENT" ]; then
  cleanup
  exit 0
fi

while [ -x "$AGENT" ]; do
  "$AGENT" --vault "$VAULT" >>"$OUT_LOG" 2>>"$ERR_LOG" &
  child=$!
  while kill -0 "$child" >/dev/null 2>&1; do
    if [ ! -x "$AGENT" ]; then
      kill "$child" >/dev/null 2>&1 || true
      wait "$child" >/dev/null 2>&1 || true
      cleanup
      exit 0
    fi
    sleep 10
  done
  wait "$child" >/dev/null 2>&1 || true
  child=""
  sleep 2
done

cleanup
"#,
            shell_quote(unit_name),
            shell_quote(&unit_path.display().to_string()),
            shell_quote(&agent_binary.display().to_string()),
            shell_quote(&vault_dir.display().to_string()),
            shell_quote(&out_log.display().to_string()),
            shell_quote(&err_log.display().to_string()),
        )
    }

    fn systemctl<const N: usize>(args: [&str; N]) -> Result<()> {
        let status = Command::new("systemctl")
            .arg("--user")
            .args(args)
            .status()
            .context("failed to run systemctl --user")?;
        if !status.success() {
            anyhow::bail!("systemctl --user command failed");
        }
        Ok(())
    }

    fn systemctl_success<const N: usize>(args: [&str; N]) -> bool {
        Command::new("systemctl")
            .arg("--user")
            .args(args)
            .status()
            .is_ok_and(|status| status.success())
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use super::*;

    pub(super) fn install(agent_binary: &Path, vault_dir: &Path) -> Result<AgentAutostartStatus> {
        let service = crate::windows_service::install_service(agent_binary, vault_dir)?;
        Ok(AgentAutostartStatus {
            service_name: service.service_name.clone(),
            registered: service.registered,
            running: service.running,
            install_path: Some(PathBuf::from(format!(r"SCM\{}", service.service_name))),
            supervisor_path: None,
            agent_binary: Some(agent_binary.to_path_buf()),
        })
    }

    pub(super) fn uninstall(vault_dir: &Path) -> Result<AgentAutostartStatus> {
        let service = crate::windows_service::uninstall_service(vault_dir)?;
        Ok(AgentAutostartStatus {
            service_name: service.service_name.clone(),
            registered: service.registered,
            running: service.running,
            install_path: Some(PathBuf::from(format!(r"SCM\{}", service.service_name))),
            supervisor_path: None,
            agent_binary: None,
        })
    }

    pub(super) fn stop(vault_dir: &Path) -> Result<AgentAutostartStatus> {
        let _ = crate::windows_service::stop_service(vault_dir);
        let service = crate::windows_service::query_service(vault_dir)?;
        Ok(AgentAutostartStatus {
            service_name: service.service_name.clone(),
            registered: service.registered,
            running: service.running,
            install_path: Some(PathBuf::from(format!(r"SCM\{}", service.service_name))),
            supervisor_path: None,
            agent_binary: None,
        })
    }

    pub(super) fn query(vault_dir: &Path) -> Result<AgentAutostartStatus> {
        let service = crate::windows_service::query_service(vault_dir)?;
        Ok(AgentAutostartStatus {
            service_name: service.service_name.clone(),
            registered: service.registered,
            running: service.running,
            install_path: Some(PathBuf::from(format!(r"SCM\{}", service.service_name))),
            supervisor_path: None,
            agent_binary: None,
        })
    }

    pub(super) fn install_tray(
        _desktop_binary: &Path,
        vault_dir: &Path,
    ) -> Result<TrayAutostartStatus> {
        unsupported_tray(vault_dir)
    }

    pub(super) fn uninstall_tray(vault_dir: &Path) -> Result<TrayAutostartStatus> {
        unsupported_tray(vault_dir)
    }

    pub(super) fn stop_tray(vault_dir: &Path) -> Result<TrayAutostartStatus> {
        unsupported_tray(vault_dir)
    }

    pub(super) fn query_tray(vault_dir: &Path) -> Result<TrayAutostartStatus> {
        unsupported_tray(vault_dir)
    }

    fn unsupported_tray(vault_dir: &Path) -> Result<TrayAutostartStatus> {
        Ok(TrayAutostartStatus {
            service_name: tray_service_name(vault_dir)?,
            registered: false,
            running: false,
            install_path: None,
            supervisor_path: None,
            desktop_binary: None,
        })
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod imp {
    use super::*;

    pub(super) fn install(_agent_binary: &Path, vault_dir: &Path) -> Result<AgentAutostartStatus> {
        unsupported(vault_dir)
    }

    pub(super) fn uninstall(vault_dir: &Path) -> Result<AgentAutostartStatus> {
        unsupported(vault_dir)
    }

    pub(super) fn stop(vault_dir: &Path) -> Result<AgentAutostartStatus> {
        unsupported(vault_dir)
    }

    pub(super) fn query(vault_dir: &Path) -> Result<AgentAutostartStatus> {
        unsupported(vault_dir)
    }

    pub(super) fn install_tray(
        _desktop_binary: &Path,
        vault_dir: &Path,
    ) -> Result<TrayAutostartStatus> {
        unsupported_tray(vault_dir)
    }

    pub(super) fn uninstall_tray(vault_dir: &Path) -> Result<TrayAutostartStatus> {
        unsupported_tray(vault_dir)
    }

    pub(super) fn stop_tray(vault_dir: &Path) -> Result<TrayAutostartStatus> {
        unsupported_tray(vault_dir)
    }

    pub(super) fn query_tray(vault_dir: &Path) -> Result<TrayAutostartStatus> {
        unsupported_tray(vault_dir)
    }

    fn unsupported(vault_dir: &Path) -> Result<AgentAutostartStatus> {
        Ok(AgentAutostartStatus {
            service_name: crate::paths::agent_service_name(vault_dir)?,
            registered: false,
            running: false,
            install_path: None,
            supervisor_path: None,
            agent_binary: None,
        })
    }

    fn unsupported_tray(vault_dir: &Path) -> Result<TrayAutostartStatus> {
        Ok(TrayAutostartStatus {
            service_name: tray_service_name(vault_dir)?,
            registered: false,
            running: false,
            install_path: None,
            supervisor_path: None,
            desktop_binary: None,
        })
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn write_supervisor(path: &Path, contents: &str) -> Result<()> {
    atomic_write_bytes(path, contents.as_bytes())?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .map(|dir| dir.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(target_os = "linux")]
fn systemd_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(target_os = "macos")]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
