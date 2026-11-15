//! Authenticated CLI requests reuse signature verification, cache and runtime exclusion.
use super::{production, Updater};
use crate::shell::{Event, Handle};
use aipass_agent::ipc::framing::write_frame_with_deadline;
use aipass_agent_protocol::{
    AuthenticatedDesktopUpdate, DesktopUpdateOperation, UpdateOutcome, UPDATE_CONTROL_VERSION,
};
use anyhow::{ensure, Context, Result};
use interprocess::local_socket::Stream;
use std::{sync::Mutex, time::Duration};
static LAST_INSTALL: Mutex<Option<UpdateOutcome>> = Mutex::new(None);

fn outcome() -> UpdateOutcome {
    let mut value = UpdateOutcome::new("desktop", env!("CARGO_PKG_VERSION"));
    value.process_id = Some(std::process::id());
    value
}
fn authenticate(shell: &Handle, request: &AuthenticatedDesktopUpdate) -> Result<()> {
    ensure!(
        request.protocol_version == UPDATE_CONTROL_VERSION,
        "update_control_protocol_mismatch"
    );
    let vault = aipass_agent::canonical_vault_dir(&request.vault_dir)?;
    ensure!(
        vault == shell.client().config.vault_dir,
        "update_vault_mismatch"
    );
    let token = aipass_agent::ipc::read_auth_token(&vault).context("update_auth_unavailable")?;
    ensure!(
        tokens_match(
            token.expose().as_bytes(),
            request.auth_token.expose().as_bytes()
        ),
        "update_auth_invalid"
    );
    Ok(())
}
fn tokens_match(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    for index in 0..a.len().max(b.len()) {
        diff |=
            usize::from(a.get(index).copied().unwrap_or(0) ^ b.get(index).copied().unwrap_or(0));
    }
    diff == 0 && !a.is_empty()
}

pub fn handle(
    mut stream: Stream,
    shell: Handle,
    request: Option<AuthenticatedDesktopUpdate>,
) -> Result<()> {
    let reply = |stream: &mut Stream, value: &UpdateOutcome| {
        write_frame_with_deadline(stream, value, Duration::from_secs(5))
    };
    let result = (|| -> Result<()> {
        let request = request.context("update_auth_required")?;
        authenticate(&shell, &request)?;
        if matches!(request.request, DesktopUpdateOperation::Status) {
            return reply(
                &mut stream,
                &LAST_INSTALL.lock().unwrap().clone().unwrap_or_else(outcome),
            );
        }
        let context = shell
            .update
            .as_ref()
            .context("update_installation_unsupported")?;
        std::fs::create_dir_all(&context.config.cache_dir)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(context.config.cache_dir.join("operation.lock"))?;
        lock.try_lock().context("update_busy")?;
        let (value, prepared) = prepare(context, &request.request)?;
        // A disconnected caller cannot initiate bundle replacement.
        reply(&mut stream, &value)?;
        if let Some((updater, release)) = prepared {
            *LAST_INSTALL.lock().unwrap() = Some(value.clone());
            let runtime = context.runtime.clone();
            let instance = context.instance.clone();
            let shell = shell.clone();
            std::thread::spawn(move || {
                let _lock = lock;
                let result = (|| -> Result<()> {
                    let prepared = updater.prepare_install(&release)?;
                    let args = vec![
                        "--vault".to_owned(),
                        shell
                            .client()
                            .config
                            .vault_dir
                            .to_string_lossy()
                            .into_owned(),
                        "--window-target=tray".into(),
                    ];
                    // CLI's explicit Install operation authorizes service interruption.
                    runtime.install(true, || {
                        updater.clear()?;
                        crate::runtime_check::installed()?;
                        prepared
                            .restart(&crate::runtime_check::restart_args(&args), Some(&instance))
                    })?;
                    Ok(())
                })();
                match result {
                    Ok(()) => {
                        shell.stop();
                        shell.event(Event::Quit);
                    }
                    Err(error) => {
                        let mut failed = value;
                        failed.status = "failed".into();
                        failed.error = Some(error.to_string());
                        *LAST_INSTALL.lock().unwrap() = Some(failed);
                    }
                }
            });
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut failed = outcome();
        failed.status = "failed".into();
        failed.error = Some(error.to_string());
        reply(&mut stream, &failed)?;
    }
    Ok(())
}

type Prepared = Option<(Updater, super::Release)>;
fn prepare(
    context: &production::Context,
    request: &DesktopUpdateOperation,
) -> Result<(UpdateOutcome, Prepared)> {
    let (channel, requested_version, download) = match request {
        DesktopUpdateOperation::Check { channel } => (*channel, None, false),
        DesktopUpdateOperation::Download { channel } => (*channel, None, true),
        DesktopUpdateOperation::Install { channel, version } => (*channel, Some(version), true),
        DesktopUpdateOperation::Status => unreachable!(),
    };
    let mut updater = context.updater()?;
    if updater.selected_channel() != channel {
        updater.select_channel(channel)?;
    }
    let mut value = outcome();
    let Some(release) = updater.check()? else {
        return Ok((value, None));
    };
    value.available = true;
    value.latest_version = Some(release.version.to_string());
    value.status = "available".into();
    if let Some(version) = requested_version {
        ensure!(
            version == &release.version.to_string(),
            "update_release_changed; check again"
        );
    }
    if download {
        updater.download(&release, |_| {})?;
        value.status = "downloaded".into();
    }
    if requested_version.is_some() {
        value.status = "installing".into();
        Ok((value, Some((updater, release))))
    } else {
        Ok((value, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn update_auth_rejects_empty_truncated_and_wrong_tokens() {
        assert!(tokens_match(b"valid-token", b"valid-token"));
        for value in [b"".as_slice(), b"valid", b"valid-tokeN", b"valid-token\0"] {
            assert!(!tokens_match(b"valid-token", value));
        }
        assert!(!tokens_match(b"", b""));
    }
}
