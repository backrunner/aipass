use super::*;

// Keep flapping history after recovery, without generating billable probes or
// moving an established session back to a higher-priority target.
const STABILITY_WINDOW: Duration = Duration::from_secs(10 * 60);
const MAX_CIRCUIT_COOLDOWN: Duration = Duration::from_secs(15 * 60);

pub(super) fn preserved_targets(old: &RuntimeConfig, new: &RuntimeConfig) -> HashSet<(Uuid, Uuid)> {
    let mut preserved = HashSet::new();
    if old.upstream_proxy != new.upstream_proxy {
        return preserved;
    }
    for route in new.routes.iter().filter(|route| route.config.enabled) {
        let Some(previous) = old.routes.iter().find(|previous| {
            previous.config.id == route.config.id
                && previous.config.enabled
                && tokens_match(&previous.local_token, &route.local_token)
                && previous.config.inbound_protocol == route.config.inbound_protocol
                && previous.config.upstream_protocol == route.config.upstream_protocol
                && previous.config.conversion_enabled == route.config.conversion_enabled
        }) else {
            continue;
        };
        for target in route.targets.iter().filter(|target| target.config.enabled) {
            if previous.targets.iter().any(|before| {
                before.config.enabled
                    && before.config.id == target.config.id
                    && before.config.provider_entry_id == target.config.provider_entry_id
                    && before.config.secret_id == target.config.secret_id
                    && before.config.base_url == target.config.base_url
                    && before.config.auth_scheme == target.config.auth_scheme
                    && before.config.headers == target.config.headers
                    && before.config.protocol == target.config.protocol
                    && tokens_match(&before.api_key, &target.api_key)
            }) {
                preserved.insert((route.config.id, target.config.id));
            }
        }
    }
    preserved
}

impl TargetHealth {
    pub(super) fn degraded(&self) -> bool {
        self.recovering || self.consecutive_failures > 0 || self.open_until.is_some()
    }

    pub(super) fn stability_rank(&self, now: Instant) -> u8 {
        if self.degraded() {
            2
        } else if self
            .last_failure_at
            .is_some_and(|at| now.saturating_duration_since(at) < STABILITY_WINDOW)
        {
            1
        } else {
            0
        }
    }

    pub(super) fn circuit_open(&mut self, now: Instant) -> bool {
        if let Some(until) = self.open_until {
            if now < until {
                return true;
            }
            self.open_until = None;
            self.consecutive_successes = 0;
            self.recovering = true;
        }
        false
    }

    pub(super) fn fail(&mut self, policy: &RetryPolicy, now: Instant) {
        if self.stability_rank(now) == 0 {
            self.reopen_count = 0;
        }
        self.last_failure_at = Some(now);
        self.consecutive_successes = 0;
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        // Failures already in flight must not repeatedly extend the same ban.
        if self.open_until.is_some_and(|until| until > now) {
            return;
        }
        if self.recovering || self.consecutive_failures >= policy.failure_threshold.max(1) {
            let multiplier = 1_u32 << self.reopen_count.min(10);
            let cooldown = Duration::from_secs(policy.circuit_open_seconds.max(1))
                .saturating_mul(multiplier)
                .min(MAX_CIRCUIT_COOLDOWN);
            self.open_until = Some(now + cooldown);
            self.reopen_count = self.reopen_count.saturating_add(1);
            self.recovering = true;
        }
    }

    pub(super) fn succeed(&mut self, started: Instant, now: Instant) -> bool {
        if self
            .last_failure_at
            .is_some_and(|failure| started <= failure)
            || self.circuit_open(now)
        {
            return false;
        }
        if self.degraded() {
            self.consecutive_successes = self.consecutive_successes.saturating_add(1);
            if self.consecutive_successes >= RECOVERY_SUCCESS_THRESHOLD {
                self.consecutive_failures = 0;
                self.consecutive_successes = 0;
                self.recovering = false;
            }
        }
        true
    }
}

/// Reserve recovery at submission, not when building a list of fallbacks.
/// The permit lives through completion/cancellation, including streaming bodies.
pub(super) struct RecoveryPermit {
    health: Arc<Mutex<HashMap<Uuid, TargetHealth>>>,
    target_id: Uuid,
    probe_id: Option<Uuid>,
}

impl RecoveryPermit {
    pub(super) fn acquire(state: &RuntimeState, target_id: Uuid) -> Option<Self> {
        let mut health = state.health.lock().ok()?;
        let mut probe_id = None;
        if let Some(target) = health.get_mut(&target_id) {
            if target.circuit_open(Instant::now()) {
                return None;
            }
            if target.recovering {
                if target.probe_id.is_some() {
                    return None;
                }
                probe_id = Some(Uuid::new_v4());
                target.probe_id = probe_id;
            }
        }
        Some(Self {
            health: state.health.clone(),
            target_id,
            probe_id,
        })
    }
}

impl Drop for RecoveryPermit {
    fn drop(&mut self) {
        let Some(probe_id) = self.probe_id else {
            return;
        };
        if let Ok(mut health) = self.health.lock() {
            if let Some(target) = health.get_mut(&self.target_id) {
                if target.probe_id == Some(probe_id) {
                    target.probe_id = None;
                }
            }
        }
    }
}

pub(super) fn complete_target_success(
    state: &RuntimeState,
    route_id: Uuid,
    session_key: Option<&str>,
    target_id: Uuid,
    started: Instant,
    response_id: Option<&str>,
) {
    let Ok(mut health) = state.health.lock() else {
        return;
    };
    if health
        .get_mut(&target_id)
        .is_none_or(|target| target.succeed(started, Instant::now()))
    {
        // Keep health -> affinity lock order so a concurrent failure cannot
        // clear the binding and then have this older completion restore it.
        remember_affinity_target(state, route_id, session_key, target_id);
        // Responses clients may identify the next turn only by the last
        // response ID. Retain its origin in the same bounded, expiring map.
        remember_affinity_target(state, route_id, response_id, target_id);
    }
}
