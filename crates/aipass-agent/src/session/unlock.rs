//! Post-unlock lifecycle, shared by local and explicitly authorized remote sessions.
use super::*;
/// Shared post-unlock lifecycle for local passwords and explicitly granted remote codes.
pub(crate) fn complete_unlock(state: &Arc<AgentState>) {
    if crate::tool_switch::recover(state).is_err() {
        crate::logging::write_component_log(
            crate::logging::AGENT_LOG,
            "WARN",
            "event=tool.switch.recovery_pending",
        );
    }
    if with_vault(state, false, |vault| {
        crate::official_accounts::migrate(vault)
    })
    .is_err()
    {
        crate::logging::write_component_log(
            crate::logging::AGENT_LOG,
            "WARN",
            "event=subscription.cli_handoff.failed",
        );
    }
    clear_last_lock_reason(state);
    restore_proxy_if_enabled(state);
    state.session_changed.notify_all();
    crate::sync_watch::start_sync_watcher_for_current_settings(state);
}
