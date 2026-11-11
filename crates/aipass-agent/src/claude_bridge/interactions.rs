//! Model-API continuation for ACP permission and structured-input requests.
use super::*;

pub(super) fn response(result: &Value) -> Result<Value, String> {
    if result["isError"] == true {
        return Ok(json!({"outcome":{"outcome":"cancelled"}}));
    }
    let parts = result["content"]
        .as_array()
        .ok_or("ACP reply requires text JSON")?;
    if parts.len() != 1 || parts[0]["type"] != "text" {
        return Err("ACP reply requires one JSON text block".into());
    }
    serde_json::from_str(
        parts[0]["text"]
            .as_str()
            .ok_or("ACP reply requires JSON text")?,
    )
    .map_err(|_| "invalid ACP reply JSON".into())
}
pub(super) fn validate(run: &Run, results: &[(String, Value)]) -> Result<(), String> {
    for (id, result) in results {
        if let Some(control) = run
            .calls
            .iter()
            .find(|c| c.id == *id)
            .and_then(|c| c.control_id.as_deref())
        {
            run.acp.validate_reply(control, &response(result)?)?;
        }
    }
    Ok(())
}

/// Permissions and autonomous output can arrive between HTTP segments.
/// Preserve backpressure and ownership until the next matching caller turn.
pub(super) fn await_receiver(inner: &Weak<Inner>, key: &str) -> bool {
    loop {
        let Some(inner) = inner.upgrade() else {
            return false;
        };
        let Ok(mut runs) = inner.runs.lock() else {
            return false;
        };
        let Some(run) = runs.get_mut(key) else {
            return false;
        };
        if run.sender.is_some() {
            return true;
        }
        run.deferred = true;
        let ttl = if run.parked { PARK_TTL } else { IDLE_TTL };
        if run.last_used.elapsed() >= ttl && (run.parked || !run.acp.background_active()) {
            return false;
        }
        drop(runs);
        drop(inner);
        std::thread::sleep(Duration::from_millis(25));
    }
}
