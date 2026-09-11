use crate::*;
use clap_complete::generate;
use std::io;

pub(crate) fn handle_doctor(
    json: bool,
    vault: Option<PathBuf>,
    cli_password: Option<String>,
) -> Result<()> {
    let report = doctor_report(vault.clone(), cli_password.is_some())?;
    let ok = report
        .get("checks")
        .and_then(|value| value.as_array())
        .map(|checks| {
            checks.iter().all(|check| {
                check
                    .get("ok")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    let text = doctor_text(&report, ok);
    output(json, report, &text)
}

pub(crate) fn handle_completions(shell: clap_complete::Shell) -> Result<()> {
    let mut command = Cli::command();
    generate(shell, &mut command, "aipass", &mut io::stdout());
    Ok(())
}

pub(crate) fn handle_native_host_command(json: bool, command: NativeHostCommand) -> Result<()> {
    match command {
        NativeHostCommand::Manifest {
            host_path,
            extension_id,
        } => {
            let host_path = native_host_binary_path(host_path)?;
            let origins = allowed_origins(&extension_id)?;
            let manifest = native_manifest(&host_path, &origins);
            println!("{}", serde_json::to_string_pretty(&manifest)?);
            Ok(())
        }
        NativeHostCommand::Install {
            host_path,
            extension_id,
            output: manifest_output,
            browser,
        } => {
            let host_path = native_host_binary_path(host_path)?;
            let origins = allowed_origins(&extension_id)?;
            let install_path = manifest_output
                .unwrap_or_else(|| default_native_manifest_path(&browser).expect("manifest path"));
            if let Some(parent) = install_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let manifest = native_manifest(&host_path, &origins);
            atomic_write_bytes(&install_path, &serde_json::to_vec_pretty(&manifest)?)?;
            let settings_path = aipass_native_host::save_allowed_extension_ids(&extension_id)?;
            install_native_manifest_reference(&browser, &install_path)?;
            output(
                json,
                serde_json::json!({
                    "ok": true,
                    "browser": browser_name(&browser),
                    "hostPath": host_path,
                    "manifestPath": install_path,
                    "settingsPath": settings_path,
                    "allowedOrigins": origins,
                }),
                "Native messaging host installed",
            )
        }
    }
}

pub(crate) fn handle_agent_command(
    json: bool,
    vault: Option<PathBuf>,
    _cli_password: Option<String>,
    command: AgentSubcommand,
) -> Result<()> {
    match command {
        AgentSubcommand::Install => install_agent_service(json, vault.clone()),
        AgentSubcommand::Uninstall => uninstall_agent_service(json, vault.clone()),
        AgentSubcommand::Status => {
            let dir = vault_dir(vault.clone())?;
            let autostart = aipass_agent::query_agent_autostart(&dir)?;
            let status: SessionStatus = AgentClientConfig::for_vault(dir.clone())
                .ok()
                .map(AgentClient::new)
                .and_then(|client| {
                    client
                        .request::<SessionStatus>(&AgentRequest::SessionStatus)
                        .ok()
                })
                .unwrap_or(SessionStatus {
                    exists: manifest_exists(vault.clone())?,
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
                    "vaultDir": dir,
                    "autostart": autostart_status_json(&autostart),
                    "session": status,
                }),
                if autostart.running {
                    if status.locked {
                        "Agent autostart running (locked)"
                    } else {
                        "Agent autostart running (unlocked)"
                    }
                } else if autostart.registered {
                    "Agent autostart registered (stopped)"
                } else {
                    "Agent autostart not installed"
                },
            )
        }
        AgentSubcommand::Start => start_agent_service(json, vault.clone()),
        AgentSubcommand::Stop => stop_agent_service(json, vault.clone()),
    }
}

pub(crate) fn handle_unlock(
    json: bool,
    vault: Option<PathBuf>,
    cli_password: Option<String>,
) -> Result<()> {
    let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
    agent.ensure_running()?;
    let status = agent.unlock_for_request()?;
    output(json, serde_json::to_value(&status)?, "Vault unlocked")
}

pub(crate) fn handle_lock(
    json: bool,
    vault: Option<PathBuf>,
    cli_password: Option<String>,
) -> Result<()> {
    let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
    let status: SessionStatus = agent.request_no_unlock(AgentRequest::SessionLock {
        reason: LockReason::Manual,
    })?;
    output(json, serde_json::to_value(&status)?, "Vault locked")
}

pub(crate) fn handle_init(
    json: bool,
    vault: Option<PathBuf>,
    cli_password: Option<String>,
    password: Option<String>,
) -> Result<()> {
    let agent = CliAgent::from_parts(vault.clone(), cli_password.clone())?;
    agent.ensure_running()?;
    let password = password
        .or(cli_password.clone())
        .context("provide --password or AIPASS_MASTER_PASSWORD")?;
    let dir = vault_dir(vault.clone())?;
    let creation: VaultCreateResponse = agent.request_no_unlock(AgentRequest::VaultCreate {
        local_only: false,
        password: password.into(),
    })?;
    let recovery_key = creation.recovery_kit.recovery_key;
    let text = format!(
        "Vault created\nRecovery key (shown once): {recovery_key}\nStore this key offline; it cannot be shown again."
    );
    output(
        json,
        serde_json::json!({ "ok": true, "vaultDir": dir, "recoveryKey": recovery_key }),
        &text,
    )
}
