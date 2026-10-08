//! Compatibility namespace for timestamps and the subscription background loop.
//! Authentication and renewal are performed by official local CLIs.
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
#[cfg(test)]
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}
pub(crate) fn spawn_token_refresh(state: Arc<crate::session::AgentState>) {
    std::thread::spawn(move || {
        let mut cache = crate::provider_runtime::Background::default();
        while !state.shutdown.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(30));
            if state.shutdown.load(Ordering::Relaxed) {
                break;
            }
            if crate::session::session_status(&state).map_or(true, |s| s.locked) {
                continue;
            }
            if crate::official_accounts::refresh_registered_accounts(&state).is_err() {
                crate::logging::write_component_log(
                    crate::logging::AGENT_LOG,
                    "WARN",
                    "event=subscription.local_refresh.failed",
                );
            }
            crate::community::refresh_due(&state);
            let _ = crate::provider_runtime::refresh(&state, &mut cache);
        }
    });
}
