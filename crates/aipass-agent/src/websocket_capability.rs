//! Persist proxy capability observations through the trusted vault owner.
use crate::session::{with_vault, AgentState, ServiceError};
use std::sync::{atomic::Ordering, Arc};
use std::time::Duration;

pub(crate) fn spawn(state: Arc<AgentState>) {
    std::thread::spawn(move || loop {
        if state.shutdown.load(Ordering::Relaxed) {
            break;
        }
        match with_vault(&state, false, |vault| {
            state
                .proxy
                .lock()
                .map_err(|_| ServiceError::internal(anyhow::anyhow!("proxy lock poisoned")))?
                .persist_ws_capabilities(vault)
        }) {
            Ok(true) => {
                state.sync_revision.fetch_add(1, Ordering::Relaxed);
                state.sync_wake.fetch_add(1, Ordering::Relaxed);
            }
            Ok(false) => {}
            Err(error) if error.code == aipass_agent_protocol::AgentErrorCode::Locked => {}
            Err(error) => crate::logging::write_component_log(
                crate::logging::AGENT_LOG,
                "WARN",
                &format!("WS capability persistence failed: {}", error.message),
            ),
        }
        std::thread::sleep(Duration::from_secs(1));
    });
}
