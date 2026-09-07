//! Provider-wide admission shared by all routes and transports.
use super::*;

/// Distinguish local admission rejection from an upstream's own HTTP 429.
#[derive(Clone)]
pub(super) struct ProviderAtCapacity;

pub(super) fn capacity_response() -> Response<BoxBody> {
    let mut response = error_response(
        StatusCode::TOO_MANY_REQUESTS,
        "all available providers are at their concurrency limit",
    );
    response.extensions_mut().insert(ProviderAtCapacity);
    response
}

pub(super) struct ProviderPermit {
    counts: Arc<Mutex<HashMap<Uuid, u64>>>,
    provider_id: Uuid,
}

impl ProviderPermit {
    pub(super) fn acquire(state: &RuntimeState, target: &ResolvedTarget) -> Option<Self> {
        // Read the current limit, even for a request/session holding an older
        // route snapshot. Keep config -> counts lock order through admission.
        let config = state.config.read().ok()?;
        let provider_id = target.config.provider_entry_id;
        let limit = config
            .routes
            .iter()
            .flat_map(|route| &route.targets)
            .filter(|candidate| candidate.config.provider_entry_id == provider_id)
            .filter_map(|candidate| candidate.max_concurrent_requests)
            .filter(|limit| *limit > 0)
            .min();
        let counts = state.provider_activity.clone();
        {
            let mut counts = counts.lock().ok()?;
            let count = counts.entry(provider_id).or_default();
            if limit.is_some_and(|limit| *count >= u64::from(limit)) {
                return None;
            }
            // Count unlimited providers too, so setting a limit while requests
            // are running cannot reset the provider's actual occupancy.
            *count += 1;
        }
        Some(Self {
            counts,
            provider_id,
        })
    }
}

impl Drop for ProviderPermit {
    fn drop(&mut self) {
        if let Ok(mut counts) = self.counts.lock() {
            if let Some(count) = counts.get_mut(&self.provider_id) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    counts.remove(&self.provider_id);
                }
            }
        }
    }
}
