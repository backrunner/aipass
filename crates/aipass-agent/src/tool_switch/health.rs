//! Short-lived probe observations keyed by exact credential fingerprint.
use super::*;
use aipass_provider_registry::SecretRef;
use std::{
    collections::HashMap,
    sync::LazyLock,
    time::{Duration, Instant},
};
type Key = (PathBuf, Uuid, String);
struct Observation {
    fingerprint: String,
    state: &'static str,
    at: Instant,
}
static CHECKS: LazyLock<Mutex<HashMap<Key, Observation>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
pub(crate) fn remember(
    state: &Arc<AgentState>,
    entry_id: Uuid,
    secret: Option<&SecretRef>,
    ok: bool,
    status: Option<u16>,
) {
    let Some(secret) = secret else {
        return;
    };
    let label = if ok {
        "ready"
    } else {
        match status {
            Some(401 | 403) => "api_key_invalid",
            Some(429) => "quota_exhausted",
            Some(500..=599) => "service_unavailable",
            None => "network_error",
            _ => return,
        }
    };
    if let Ok(mut checks) = CHECKS.lock() {
        checks.retain(|_, v| v.at.elapsed() < Duration::from_secs(600));
        if checks.len() >= 512 {
            checks.clear();
        }
        checks.insert(
            (state.vault_dir.clone(), entry_id, secret.id.clone()),
            Observation {
                fingerprint: secret.fingerprint.clone(),
                state: label,
                at: Instant::now(),
            },
        );
    }
}
pub(super) fn observed(
    state: &Arc<AgentState>,
    vault: &Vault,
    request: &ToolConfigRequest,
) -> Option<&'static str> {
    let entry = vault.get_provider_summary(request.id).ok()?;
    let selected = entry
        .secret_refs
        .iter()
        .find(|s| Some(&s.id) == request.secret_id.as_ref())?;
    let checks = CHECKS.lock().ok()?;
    let item = checks.get(&(state.vault_dir.clone(), request.id, selected.id.clone()))?;
    (item.fingerprint == selected.fingerprint && item.at.elapsed() < Duration::from_secs(600))
        .then_some(item.state)
}
