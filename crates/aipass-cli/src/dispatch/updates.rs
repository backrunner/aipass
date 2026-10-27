use crate::*;
use aipass_agent::updates;
use aipass_agent_protocol::{DesktopUpdateOperation, UpdateChannel, UpdateOutcome};
use std::time::{Duration, Instant};

pub(crate) fn handle(json: bool, vault: Option<PathBuf>, args: SelfUpdateArgs) -> Result<()> {
    match run(vault, &args) {
        Ok(outcome) => {
            let message = match outcome.status.as_str() {
                "installed" => format!("Updated {} to {}", outcome.target, outcome.current_version),
                "available" => format!(
                    "{} {} is available (current {})",
                    outcome.target,
                    outcome.latest_version.as_deref().unwrap_or("unknown"),
                    outcome.current_version
                ),
                _ => format!(
                    "{} {} is up to date",
                    outcome.target, outcome.current_version
                ),
            };
            output(json, serde_json::to_value(outcome)?, &message)
        }
        Err(error) => {
            if json {
                output(
                    true,
                    serde_json::json!({ "status": "failed", "error": format!("{error:#}") }),
                    "",
                )?;
            }
            Err(error)
        }
    }
}

fn run(vault: Option<PathBuf>, args: &SelfUpdateArgs) -> Result<UpdateOutcome> {
    let executable = std::env::current_exe()?;
    let target = match args.target {
        UpdateTarget::Auto => {
            if updates::standalone_directory(&executable).is_ok() {
                UpdateTarget::Cli
            } else if updates::desktop::installed_binary().is_some() {
                UpdateTarget::Desktop
            } else {
                anyhow::bail!(
                    "cannot identify this installation; select --target cli or --target desktop"
                );
            }
        }
        other => other,
    };
    let channel = match args.channel {
        UpdateChannelArg::Auto => {
            let version = env!("CARGO_PKG_VERSION");
            if version.contains("-nightly.") {
                UpdateChannel::Nightly
            } else if version.contains("-beta.") {
                UpdateChannel::Beta
            } else {
                UpdateChannel::Official
            }
        }
        UpdateChannelArg::Official => UpdateChannel::Official,
        UpdateChannelArg::Beta => UpdateChannel::Beta,
        UpdateChannelArg::Nightly => UpdateChannel::Nightly,
    };
    let timeout = Duration::from_secs(args.timeout_seconds);
    let install = matches!(args.command, SelfUpdateCommand::Install { yes: true });
    match target {
        UpdateTarget::Cli => {
            if install {
                updates::standalone_directory(&executable)?;
            }
            let release = updates::check_cli(channel, env!("CARGO_PKG_VERSION"), timeout)?;
            if !install || !release.outcome.available {
                return Ok(release.outcome);
            }
            updates::install_cli(&release, &executable, &vault_dir(vault)?)
        }
        UpdateTarget::Desktop => desktop_update(
            &vault_dir(vault)?,
            &executable,
            args,
            channel,
            timeout,
            install,
        ),
        UpdateTarget::Auto => unreachable!(),
    }
}

fn desktop_update(
    vault: &Path,
    executable: &Path,
    args: &SelfUpdateArgs,
    channel: UpdateChannel,
    timeout: Duration,
    install: bool,
) -> Result<UpdateOutcome> {
    let kind = match args.desktop_instance {
        DesktopInstance::Release => "release",
        DesktopInstance::Development => "development",
        DesktopInstance::PackagedDevelopment => "packaged-development",
    };
    let request = |operation| updates::desktop::request(vault, kind, operation, timeout);
    let checked = match request(DesktopUpdateOperation::Check { channel }) {
        Ok(outcome) => outcome,
        Err(original) => {
            // An installed desktop may be closed. Start its tray companion,
            // preserving fixture isolation and explicit development instances.
            if kind != "release" || std::env::var_os("AIPASS_DESKTOP_RUNTIME_DIR").is_some() {
                return Err(original);
            }
            let Some(binary) = updates::desktop::installed_binary() else {
                return Err(original);
            };
            updates::desktop::start(&binary, vault)?;
            let deadline = Instant::now() + timeout.min(Duration::from_secs(15));
            loop {
                if updates::desktop::request(
                    vault,
                    kind,
                    DesktopUpdateOperation::Status,
                    Duration::from_secs(1),
                )
                .is_ok()
                {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(original).context("desktop update control did not become ready");
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            request(DesktopUpdateOperation::Check { channel })?
        }
    };
    checked_result(&checked)?;
    if !install || !checked.available {
        return Ok(checked);
    }
    let version = checked
        .latest_version
        .clone()
        .context("desktop did not return a release version")?;
    let accepted = request(DesktopUpdateOperation::Install {
        channel,
        version: version.clone(),
    })?;
    checked_result(&accepted)?;
    if accepted.status != "installing" {
        anyhow::bail!("desktop release changed before installation; check again");
    }
    let deadline = Instant::now() + timeout;
    let mut last_error = String::new();
    loop {
        std::thread::sleep(Duration::from_millis(500));
        match updates::desktop::request(
            vault,
            kind,
            DesktopUpdateOperation::Status,
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_secs(2)),
        ) {
            Ok(status)
                if matches!(
                    status.error.as_deref(),
                    Some("update_auth_unavailable" | "update_auth_invalid")
                ) =>
            {
                // Shutdown rotates/removes the Agent token. A request can read
                // the old token immediately before the desktop validates it.
                last_error = status.error.unwrap_or_default();
            }
            Ok(mut status) => {
                checked_result(&status)?;
                if status.current_version == version
                    && status.process_id.is_some()
                    && status.process_id != accepted.process_id
                {
                    // The desktop refreshes its installed CLI copy on startup.
                    // Confirm that copy too before declaring the suite updated.
                    if let Err(error) = verify_installed_cli(executable, &version).and_then(|()| {
                        AgentClient::for_vault(vault.to_path_buf())?
                            .request::<SessionStatus>(&AgentRequest::SessionStatus)
                            .map(|_| ())
                            .map_err(anyhow::Error::from)
                    }) {
                        last_error = error.to_string();
                        if Instant::now() >= deadline {
                            return Err(error);
                        }
                        continue;
                    }
                    status.status = "installed".into();
                    status.available = false;
                    status.latest_version = Some(version);
                    return Ok(status);
                }
            }
            Err(error) => last_error = error.to_string(),
        }
        if Instant::now() >= deadline {
            anyhow::bail!(
                "desktop update restart was not confirmed within {}s: {last_error}",
                timeout.as_secs()
            );
        }
    }
}

fn checked_result(outcome: &UpdateOutcome) -> Result<()> {
    if let Some(error) = &outcome.error {
        anyhow::bail!("{error}");
    }
    Ok(())
}

fn verify_installed_cli(executable: &Path, version: &str) -> Result<()> {
    let result = ProcessCommand::new(executable).arg("--version").output()?;
    if !result.status.success()
        || String::from_utf8_lossy(&result.stdout).trim() != format!("aipass {version}")
    {
        anyhow::bail!("desktop restarted but the installed CLI has not reached {version}");
    }
    Ok(())
}
