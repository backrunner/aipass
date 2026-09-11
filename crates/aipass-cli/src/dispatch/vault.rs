use crate::*;

pub(crate) fn handle_vault_command(
    json: bool,
    vault: Option<PathBuf>,
    cli_password: Option<String>,
    command: VaultCommand,
) -> Result<()> {
    match command {
        VaultCommand::Status => {
            let dir = vault_dir(vault.clone())?;
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let status = agent
                .request_no_unlock::<SessionStatus>(AgentRequest::SessionStatus)
                .unwrap_or(SessionStatus {
                    exists: dir.join("manifest.aipmanifest").exists(),
                    locked: true,
                    policy: Default::default(),
                    last_lock_reason: Some(LockReason::AgentRestart),
                    vault_namespace: None,
                    initial_sync_pending: false,
                    initial_sync_failed: false,
                    sync_revision: 0,
                    sync_status: None,
                });
            output(
                json,
                serde_json::json!({
                    "exists": status.exists,
                    "locked": status.locked,
                    "policy": status.policy,
                    "vaultDir": dir,
                }),
                if status.exists {
                    if status.locked {
                        "Vault exists (locked)"
                    } else {
                        "Vault exists (unlocked)"
                    }
                } else {
                    "Vault not initialized"
                },
            )
        }
        VaultCommand::ChangePassword { new_password } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let result: serde_json::Value = agent.request(AgentRequest::VaultChangePassword {
                new_password: new_password.into(),
            })?;
            output(json, result, "Master password changed")
        }
        VaultCommand::Rotate { reason } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let result: serde_json::Value = agent.request(AgentRequest::VaultRotate { reason })?;
            output(json, result, "Vault epoch rotated")
        }
        VaultCommand::Devices => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let devices: Vec<aipass_vault::DeviceRecord> =
                agent.request(AgentRequest::DevicesList)?;
            output(
                json,
                serde_json::to_value(&devices)?,
                &format!("{} devices", devices.len()),
            )
        }
        VaultCommand::RevokeDevice { id } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let result: serde_json::Value = agent.request(AgentRequest::DeviceRevoke { id })?;
            output(json, result, "Device revoked and vault epoch rotated")
        }
        VaultCommand::Export {
            output: export_path,
            export_password,
        } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let result: serde_json::Value = agent.request(AgentRequest::VaultExport {
                output: export_path.clone(),
                export_password: export_password.into(),
            })?;
            output(json, result, "Encrypted vault export written")
        }
        VaultCommand::Import {
            input,
            export_password,
        } => {
            let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
            let dir = vault_dir(vault.clone())?;
            let export: serde_json::Value = agent.request(AgentRequest::VaultImport {
                input,
                export_password: export_password.into(),
            })?;
            output(
                json,
                serde_json::json!({ "ok": true, "vaultDir": dir, "result": export }),
                "Encrypted vault import restored",
            )
        }
    }
}
