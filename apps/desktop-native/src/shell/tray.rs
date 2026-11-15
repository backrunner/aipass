//! One bounded tray worker owns readiness, status polls and actions on every OS.
use super::{
    i18n::{tf, tr},
    status::*,
    Event, Handle,
};
use aipass_agent_protocol::{
    AgentRequest, ControlPanelStatus, LanguageSettings, LockReason, ProxyConfig, ProxyStatus,
    SensitiveString, SessionStatus, SessionUnlockMode,
};
use std::{sync::mpsc, time::Duration};

pub enum Command {
    Refresh,
    Action(String),
    Unlock(SensitiveString),
}

pub fn action(handle: &Handle, action: &str) {
    match action {
        "open" => handle.event(Event::Activate("main".into())),
        "hide" => handle.event(Event::Activate("tray".into())),
        "proxy-open" => handle.event(Event::Activate("server".into())),
        "panel-open" | "refresh" => handle.refresh(),
        _ => {
            if !handle.command(Command::Action(action.into())) {
                handle.event(Event::Error(tr("tray.agentUnavailable")));
            }
        }
    }
}
pub fn unlock(handle: &Handle, password: String) {
    if !handle.command(Command::Unlock(SensitiveString::new(password))) {
        handle.event(Event::UnlockResult(Some(tr("tray.agentUnavailable"))));
    }
}

pub(super) fn start(handle: Handle, jobs: mpsc::Receiver<Command>) {
    std::thread::spawn(move || {
        publish(&handle);
        while !handle.stopped() {
            match jobs.recv_timeout(Duration::from_secs(30)) {
                Ok(Command::Action(action)) => {
                    if let Err(error) = perform(&handle, &action) {
                        // Never display raw IPC data or credentials in desktop diagnostics.
                        let _ = error;
                        handle.event(Event::Error(tr("tray.agent.startFailed")));
                    }
                }
                Ok(Command::Unlock(password)) => {
                    let result = handle.request::<SessionStatus>(AgentRequest::SessionUnlock {
                        mode: SessionUnlockMode::Password { password },
                    });
                    let error = result.err().map(|error| friendly_unlock_error(&error));
                    handle.event(Event::UnlockResult(error));
                    handle.event(Event::Refresh);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(Command::Refresh) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if !handle.stopped() && !handle.gate.suspended() {
                publish(&handle);
            }
        }
    });
}

fn publish(handle: &Handle) {
    if handle.gate.suspended() {
        return;
    }
    let snapshot =
        snapshot(handle).unwrap_or_else(|_| TraySnapshot::unavailable(tr("tray.agentUnavailable")));
    handle.event(Event::Tray(Box::new(snapshot.dto())));
}
fn snapshot(handle: &Handle) -> anyhow::Result<TraySnapshot> {
    let _guard = handle.gate.enter()?;
    handle.services.ensure_ready()?;
    let client = handle.client();
    if let Ok(language) = client.request::<LanguageSettings>(&AgentRequest::LanguageSettingsGet {
        legacy_locale: None,
    }) {
        super::i18n::set_locale(language.resolved_locale);
    }
    let session = client.request::<SessionStatus>(&AgentRequest::SessionStatus)?;
    let routes = client
        .request::<ProxyConfig>(&AgentRequest::ServerConfigGet)
        .map(|config| {
            config
                .routes
                .into_iter()
                .map(|route| TrayRoute {
                    id: route.id,
                    name: route.name,
                    active: route.enabled,
                })
                .collect()
        })
        .unwrap_or_default();
    let proxy = client
        .request::<ProxyStatus>(&AgentRequest::ServerStatus)
        .map(ProxyTrayStatus::Available)
        .unwrap_or_else(|_| ProxyTrayStatus::Unavailable(tr("tray.unavailable")));
    Ok(TraySnapshot {
        agent: TrayStatus::Running(session),
        proxy,
        routes,
        panel: client.request(&AgentRequest::ControlPanelStatus).ok(),
    })
}

fn perform(handle: &Handle, action: &str) -> anyhow::Result<()> {
    match action {
        "start-agent" => {
            let _guard = handle.gate.enter()?;
            handle.services.ensure_ready()?;
        }
        "lock-vault" => {
            aipass_desktop_platform::clipboard::invalidate();
            aipass_desktop_platform::clipboard::clear_now().map_err(anyhow::Error::msg)?;
            handle.request::<SessionStatus>(AgentRequest::SessionLock {
                reason: LockReason::Manual,
            })?;
            handle.event(Event::Locked);
        }
        "proxy-start" => {
            handle.request::<ProxyStatus>(AgentRequest::ServerStart)?;
        }
        "proxy-stop" => {
            handle.request::<ProxyStatus>(AgentRequest::ServerStop)?;
        }
        "repair-autostart" => handle.repair()?,
        "quit" => return handle.quit(),
        "panel-stop" => {
            handle.request::<ControlPanelStatus>(AgentRequest::ControlPanelStop)?;
        }
        "panel-open-url" | "panel-copy" => {
            let status = handle.request::<ControlPanelStatus>(AgentRequest::ControlPanelStatus)?;
            let url = status
                .url
                .ok_or_else(|| anyhow::anyhow!("control panel unavailable"))?;
            if action == "panel-copy" {
                handle.event(Event::Copy(url));
            } else {
                handle.event(Event::OpenUrl(url));
            }
        }
        id if id.starts_with("proxy-group-") => {
            let route_id = id.trim_start_matches("proxy-group-").parse()?;
            let current = handle.request::<ProxyConfig>(AgentRequest::ServerConfigGet)?;
            let route = current
                .routes
                .iter()
                .find(|route| route.id == route_id)
                .ok_or_else(|| anyhow::anyhow!("route no longer exists"))?;
            handle.request::<ProxyConfig>(AgentRequest::ServerRouteSetEnabled {
                route_id,
                enabled: !route.enabled,
            })?;
        }
        _ => return Ok(()),
    }
    handle.event(Event::Refresh);
    Ok(())
}

fn friendly_unlock_error(error: &aipass_agent::AgentCommandError) -> String {
    use aipass_agent_protocol::AgentErrorCode;
    match error.code {
        Some(AgentErrorCode::InvalidPassword) => tr("error.incorrectMasterPassword"),
        Some(AgentErrorCode::ServiceUnavailable) => tr("tray.agentUnavailable"),
        Some(AgentErrorCode::Locked) => tr("auth.unlock.desc"),
        _ => tf("error.unlockFailed", &[]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unlock_errors_never_expose_internal_details() {
        let error = aipass_agent::AgentCommandError {
            code: Some(aipass_agent_protocol::AgentErrorCode::InvalidPassword),
            message: "secret internal detail".into(),
        };
        assert!(!friendly_unlock_error(&error).contains("secret"));
    }
}
