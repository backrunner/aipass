use super::*;
use crate::paths::agent_service_name;

#[path = "linux_session.rs"]
mod session;

pub(super) fn install(agent_binary: &Path, vault_dir: &Path) -> Result<AgentAutostartStatus> {
    let service_name = agent_service_name(vault_dir)?;
    let paths = linux_paths(&service_name)?;
    if session::registered(vault_dir)?
        || (!paths.unit_path.exists() && !systemctl_success(["show-environment"]))
    {
        return session::install(agent_binary, vault_dir);
    }
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
    systemctl(["enable", paths.unit_name.as_str()])?;
    // enable --now does not replace an already running process after its
    // executable path or generated configuration has changed.
    systemctl(["restart", paths.unit_name.as_str()])?;

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
    if session::registered(vault_dir)? {
        return session::uninstall(vault_dir);
    }
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
    if session::registered(vault_dir)? {
        shutdown_agent(vault_dir);
        return session::query(vault_dir);
    }
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
    if session::registered(vault_dir)? {
        return session::query(vault_dir);
    }
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

#[cfg(target_os = "linux")]
pub(super) use super::linux_tray::{install_tray, query_tray, stop_tray, uninstall_tray};

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
        unit_path: crate::paths::linux_config_home(&home)
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
sleep 1
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
        .output()
        .context("failed to run systemctl --user")?;
    if !status.status.success() {
        anyhow::bail!(
            "systemctl --user command failed: {}",
            String::from_utf8_lossy(&status.stderr).trim()
        );
    }
    Ok(())
}

fn systemctl_success<const N: usize>(args: [&str; N]) -> bool {
    Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .is_ok_and(|output| output.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systemd_exec_preserves_literal_percent_and_dollar_paths() {
        let unit = linux_unit("fixture", Path::new("/tmp/100%/$HOME/a b.sh"));
        assert!(unit.contains("ExecStart=\"/tmp/100%%/$$HOME/a b.sh\""));
    }
}
