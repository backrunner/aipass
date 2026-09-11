use crate::*;

pub(crate) fn manifest_exists(explicit_vault: Option<PathBuf>) -> Result<bool> {
    Ok(vault_dir(explicit_vault)?
        .join("manifest.aipmanifest")
        .exists())
}

pub(crate) fn install_agent_service(json: bool, explicit_vault: Option<PathBuf>) -> Result<()> {
    let vault_dir = vault_dir(explicit_vault)?;
    let agent_binary = aipass_agent::agent_binary_path()?;
    let namespace = aipass_agent::namespace_for_vault_dir(&vault_dir)?;
    let status = aipass_agent::install_agent_autostart(&agent_binary, &vault_dir)
        .context("failed to install AIPass agent autostart")?;
    output(
        json,
        serde_json::json!({
            "ok": true,
            "vaultDir": vault_dir,
            "agentBinary": agent_binary,
            "autostart": autostart_status_json(&status),
            "namespace": namespace,
        }),
        "Agent autostart installed",
    )
}

pub(crate) fn start_agent_service(json: bool, explicit_vault: Option<PathBuf>) -> Result<()> {
    let vault_dir = vault_dir(explicit_vault)?;
    let agent_binary = aipass_agent::agent_binary_path()?;
    let status = aipass_agent::install_agent_autostart(&agent_binary, &vault_dir)
        .context("failed to start AIPass agent autostart")?;
    let client = AgentClient::for_vault(vault_dir.clone())?;
    client.ensure_running()?;
    output(
        json,
        serde_json::json!({
            "ok": true,
            "vaultDir": vault_dir,
            "autostart": autostart_status_json(&status),
        }),
        "Agent autostart started",
    )
}

pub(crate) fn stop_agent_service(json: bool, explicit_vault: Option<PathBuf>) -> Result<()> {
    let vault_dir = vault_dir(explicit_vault)?;
    let status = aipass_agent::stop_agent_autostart(&vault_dir)
        .context("failed to stop AIPass agent autostart")?;
    output(
        json,
        serde_json::json!({
            "ok": true,
            "vaultDir": vault_dir,
            "autostart": autostart_status_json(&status),
        }),
        "Agent autostart stopped",
    )
}

pub(crate) fn uninstall_agent_service(json: bool, explicit_vault: Option<PathBuf>) -> Result<()> {
    let vault_dir = vault_dir(explicit_vault)?;
    let status = aipass_agent::uninstall_agent_autostart(&vault_dir)
        .context("failed to uninstall AIPass agent autostart")?;
    output(
        json,
        serde_json::json!({
            "ok": true,
            "vaultDir": vault_dir,
            "autostart": autostart_status_json(&status),
        }),
        "Agent autostart uninstalled",
    )
}

pub(crate) fn autostart_status_json(
    status: &aipass_agent::AgentAutostartStatus,
) -> serde_json::Value {
    serde_json::json!({
        "serviceName": &status.service_name,
        "registered": status.registered,
        "running": status.running,
        "installPath": &status.install_path,
        "supervisorPath": &status.supervisor_path,
        "agentBinary": &status.agent_binary,
    })
}
