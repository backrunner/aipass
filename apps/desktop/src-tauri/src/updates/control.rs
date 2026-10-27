//! Authenticated CLI adapter over the desktop's existing signed updater.
use super::*;
use aipass_agent::ipc::framing::write_frame_with_deadline;
use aipass_agent_protocol::{
    AuthenticatedDesktopUpdate, DesktopUpdateOperation, UpdateOutcome, UPDATE_CONTROL_VERSION,
};
use anyhow::Result;
use interprocess::local_socket::Stream;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::Duration,
};

static UNATTENDED: AtomicBool = AtomicBool::new(false);
static LAST_INSTALL: Mutex<Option<UpdateOutcome>> = Mutex::new(None);

pub(super) fn restart_target() -> &'static str {
    if UNATTENDED.load(Ordering::SeqCst) {
        "tray"
    } else {
        "main"
    }
}

fn desktop_outcome(app: &AppHandle) -> UpdateOutcome {
    let mut outcome = UpdateOutcome::new("desktop", &app.package_info().version.to_string());
    outcome.process_id = Some(std::process::id());
    outcome
}

fn authenticated(app: &AppHandle, request: &AuthenticatedDesktopUpdate) -> Result<(), String> {
    if request.protocol_version != UPDATE_CONTROL_VERSION {
        return Err("update_control_protocol_mismatch".into());
    }
    let client = crate::agent_client(app)?;
    let requested =
        aipass_agent::canonical_vault_dir(&request.vault_dir).map_err(|e| e.to_string())?;
    if requested != client.config.vault_dir {
        return Err("update_vault_mismatch".into());
    }
    let expected =
        aipass_agent::ipc::read_auth_token(&requested).map_err(|_| "update_auth_unavailable")?;
    if !tokens_match(
        expected.expose().as_bytes(),
        request.auth_token.expose().as_bytes(),
    ) {
        return Err("update_auth_invalid".into());
    }
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

pub(crate) fn handle(
    mut stream: Stream,
    app: AppHandle,
    request: Option<AuthenticatedDesktopUpdate>,
) -> Result<()> {
    let base = || desktop_outcome(&app);
    let reply = |stream: &mut Stream, outcome: &UpdateOutcome| {
        write_frame_with_deadline(stream, outcome, Duration::from_secs(5))
    };
    let error = |message: String| {
        let mut outcome = base();
        outcome.status = "failed".into();
        outcome.error = Some(message);
        outcome
    };
    let Some(request) = request else {
        return reply(&mut stream, &error("update_auth_required".into()));
    };
    if let Err(message) = authenticated(&app, &request) {
        return reply(&mut stream, &error(message));
    }
    if matches!(request.request, DesktopUpdateOperation::Status) {
        let outcome = LAST_INSTALL
            .lock()
            .ok()
            .and_then(|v| v.clone())
            .unwrap_or_else(base);
        return reply(&mut stream, &outcome);
    }
    let guard = match UPDATE_OPERATION.try_lock() {
        Ok(guard) => guard,
        Err(_) => return reply(&mut stream, &error("update_busy".into())),
    };
    let result = tauri::async_runtime::block_on(prepare(&app, &request.request));
    let (outcome, install) = match result {
        Ok(value) => value,
        Err(message) => return reply(&mut stream, &error(message)),
    };
    if let Some((update, package)) = install {
        // A disconnected caller cannot start a destructive install. A delivered
        // acknowledgement means "installing"; the CLI still verifies restart.
        reply(&mut stream, &outcome)?;
        if let Ok(mut status) = LAST_INSTALL.lock() {
            *status = Some(outcome.clone());
        }
        UNATTENDED.store(true, Ordering::SeqCst);
        std::thread::spawn(move || {
            let _guard = guard;
            if let Err(message) = install_verified_update(&app, update, package) {
                UNATTENDED.store(false, Ordering::SeqCst);
                let mut failed = outcome;
                failed.status = "failed".into();
                failed.error = Some(message);
                if let Ok(mut status) = LAST_INSTALL.lock() {
                    *status = Some(failed);
                }
            }
        });
        Ok(())
    } else {
        reply(&mut stream, &outcome)
    }
}

type PreparedInstall = Option<(tauri_plugin_updater::Update, Vec<u8>)>;
async fn prepare(
    app: &AppHandle,
    operation: &DesktopUpdateOperation,
) -> Result<(UpdateOutcome, PreparedInstall), String> {
    let mut outcome = desktop_outcome(app);
    let (channel, install_version, download) = match operation {
        DesktopUpdateOperation::Check { channel } => (*channel, None, false),
        DesktopUpdateOperation::Download { channel } => (*channel, None, true),
        DesktopUpdateOperation::Install { channel, version } => (*channel, Some(version), true),
        DesktopUpdateOperation::Status => unreachable!(),
    };
    let updater = updater_for_channel(app, channel.as_str())?;
    let Some(update) = updater.check().await.map_err(updater_error)? else {
        return Ok((outcome, None));
    };
    outcome.available = true;
    outcome.latest_version = Some(update.version.clone());
    outcome.status = "available".into();
    if let Some(version) = install_version {
        if version != &update.version {
            return Err("update_release_changed; check again".into());
        }
        // System-package authorization can open a GUI. Unattended users should
        // use their distribution's package manager instead of Polkit dialogs.
        #[cfg(target_os = "linux")]
        if linux::kind()? != linux::Kind::AppImage {
            return Err(
                "update_unattended_installation_unsupported; use the system package manager".into(),
            );
        }
    }
    if !download {
        return Ok((outcome, None));
    }
    let package = cached_or_downloaded_package(app, channel.as_str(), &update).await?;
    verify_cached_package(app, &update, &package).map_err(|_| "update_signature_invalid")?;
    if install_version.is_some() {
        outcome.status = "installing".into();
        Ok((outcome, Some((update, package))))
    } else {
        outcome.status = "downloaded".into();
        Ok((outcome, None))
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
