use super::*;

pub fn lock_session(state: &Arc<AgentState>, reason: LockReason) {
    crate::logging::write_component_log(
        crate::logging::AGENT_LOG,
        "INFO",
        &format!("event=session.lock reason={reason:?}"),
    );
    if let Ok(mut session) = state.session.lock() {
        if let SessionState::Unlocked(info) = &*session {
            if let Ok(settings) = load_sync_settings(&state.vault_dir) {
                if settings.mode == SyncMode::WebDav {
                    let _ = transport_password(state, &settings, Some(&info.vault));
                }
                if crate::vault_sync::queue_configured_changes(state, &info.vault, &settings)
                    .is_err()
                {
                    crate::logging::write_component_log(
                        crate::logging::AGENT_LOG,
                        "WARN",
                        "event=sync.outbox.stage_failed",
                    );
                }
            }
        }
        *session = SessionState::Locked;
    }
    state.subscription_imports.clear();
    crate::claude_cli::logins().clear();
    state.session_changed.notify_all();
    // Transition the session first. Any vault operation already in flight must
    // finish before this lock is acquired, and no new one can start after it.
    // The proxy runtime owns its resolved credentials and intentionally keeps
    // serving while locked; only redundant credentials in its management
    // configuration are cleared here.
    if let Ok(mut proxy) = state.proxy.lock() {
        proxy.lock_for_session();
    }
    if let Ok(mut last_reason) = state.last_lock_reason.lock() {
        *last_reason = Some(reason);
    }
}
