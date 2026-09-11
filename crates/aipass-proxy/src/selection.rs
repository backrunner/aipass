use super::*;

pub(crate) fn select_route(
    state: &RuntimeState,
    bearer_token: Option<&str>,
    api_key_token: Option<&str>,
    inbound: Option<ProxyProtocol>,
) -> Option<(ResolvedRoute, Vec<ModelPricing>)> {
    state.config.read().ok().and_then(|config| {
        config.enabled.then(|| {
            config
                .routes
                .iter()
                .find(|route| {
                    route.config.enabled
                        && inbound.is_none_or(|protocol| {
                            route_accepts_inbound_protocol(&route.config, protocol)
                        })
                        && (bearer_token
                            .is_some_and(|token| tokens_match(&route.local_token, token))
                            || api_key_token
                                .is_some_and(|token| tokens_match(&route.local_token, token)))
                })
                .cloned()
                .map(|route| (route, config.pricing.clone()))
        })?
    })
}

/// A local token is scoped to one route's inbound wire protocol. Keeping this
/// check explicit prevents a token configured for Codex/Responses from being
/// accepted on another OpenAI-compatible endpoint such as Chat Completions.
pub(crate) fn route_accepts_inbound_protocol(
    route: &ProxyRouteConfig,
    protocol: ProxyProtocol,
) -> bool {
    route.inbound_protocol == protocol
}

pub(crate) fn silent_retry_rounds(policy: &RetryPolicy) -> u8 {
    if policy.silent_retry {
        policy.max_silent_retries.saturating_add(1).max(1)
    } else {
        1
    }
}

pub(crate) fn hold_backoff_delay(policy: &RetryPolicy, hold_round: u32) -> Duration {
    let mut delay_ms = policy.hold_initial_delay_ms.max(1);
    for _ in 0..hold_round {
        if delay_ms >= policy.hold_max_delay_ms {
            break;
        }
        delay_ms = delay_ms.saturating_mul(2);
    }
    Duration::from_millis(delay_ms.min(policy.hold_max_delay_ms.max(1)))
}

// The hold budget applies until the response is committed. A live stream
// uses its idle timeout after commit and must never be replayed at this deadline.
pub(crate) fn hold_deadline(
    policy: &RetryPolicy,
    started: Instant,
) -> Option<tokio::time::Instant> {
    (policy.hold_on_failure && policy.hold_max_duration_ms > 0)
        .then(|| started.checked_add(Duration::from_millis(policy.hold_max_duration_ms)))
        .flatten()
        .map(tokio::time::Instant::from_std)
}

pub(crate) fn bounded_deadline(
    timeout: Duration,
    hold_deadline: Option<tokio::time::Instant>,
) -> tokio::time::Instant {
    let deadline = tokio::time::Instant::now() + timeout;
    hold_deadline.map_or(deadline, |hold| deadline.min(hold))
}

pub(crate) fn normalize_session_affinity_key(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.len() <= MAX_SESSION_AFFINITY_KEY_BYTES).then(|| value.to_owned())
}

/// Resolve an affinity key without forwarding a proxy-only header as a
/// provider credential. The body key is used as a fallback so clients can
/// opt into affinity through the standard `prompt_cache_key` request field.
pub(crate) fn session_affinity_key(
    headers: &HeaderMap,
    metadata: Option<&RequestMetadata>,
) -> Option<String> {
    for name in SESSION_AFFINITY_HEADERS {
        if let Some(value) = headers.get(name).and_then(|value| value.to_str().ok()) {
            if let Some(value) = normalize_session_affinity_key(value) {
                return Some(value);
            }
        }
    }
    metadata
        .and_then(|metadata| {
            metadata
                .prompt_cache_key
                .as_deref()
                .or(metadata.session_id.as_deref())
                .or(metadata.session.as_deref())
                .or(metadata.conversation_id.as_deref())
                .or(metadata.previous_response_id.as_deref())
        })
        .and_then(normalize_session_affinity_key)
        .or_else(|| {
            metadata
                .and_then(|metadata| metadata.conversation.as_ref())
                .and_then(|conversation| {
                    conversation
                        .as_str()
                        .or_else(|| conversation.get("id").and_then(serde_json::Value::as_str))
                })
                .and_then(normalize_session_affinity_key)
        })
}

pub(crate) fn session_affinity_key_from_value(value: &serde_json::Value) -> Option<String> {
    let object = value.as_object()?;
    SESSION_AFFINITY_FIELDS
        .iter()
        .find_map(|field| object.get(*field).and_then(serde_json::Value::as_str))
        .and_then(normalize_session_affinity_key)
        .or_else(|| {
            object
                .get("conversation")
                .and_then(|conversation| {
                    conversation
                        .as_str()
                        .or_else(|| conversation.get("id").and_then(serde_json::Value::as_str))
                })
                .and_then(normalize_session_affinity_key)
        })
}

pub(crate) fn affinity_target(
    state: &RuntimeState,
    route_id: Uuid,
    session_key: Option<&str>,
    targets: &[ResolvedTarget],
) -> Option<Uuid> {
    let session_key = session_key.and_then(normalize_session_affinity_key)?;
    let key = (route_id, session_key);
    let now = Instant::now();
    let target_id = {
        let mut affinities = state.session_affinity.lock().ok()?;
        affinities.retain(|_, affinity| {
            now.saturating_duration_since(affinity.last_used) < SESSION_AFFINITY_TTL
        });
        affinities.get(&key)?.target_id
    };
    if !targets
        .iter()
        .any(|target| target.config.enabled && target.config.id == target_id)
        || circuit_open(state, target_id)
    {
        if let Ok(mut affinities) = state.session_affinity.lock() {
            affinities.remove(&key);
        }
        return None;
    }
    if let Ok(mut affinities) = state.session_affinity.lock() {
        if let Some(affinity) = affinities.get_mut(&key) {
            affinity.last_used = now;
        }
    }
    Some(target_id)
}

pub(crate) fn remember_affinity_target(
    state: &RuntimeState,
    route_id: Uuid,
    session_key: Option<&str>,
    target_id: Uuid,
) {
    let Some(session_key) = session_key.and_then(normalize_session_affinity_key) else {
        return;
    };
    let Ok(mut affinities) = state.session_affinity.lock() else {
        return;
    };
    let now = Instant::now();
    affinities.retain(|_, affinity| {
        now.saturating_duration_since(affinity.last_used) < SESSION_AFFINITY_TTL
    });
    // An older request finishing on a previous provider must not steal a
    // session that has already completed fallback successfully.
    let affinity = affinities
        .entry((route_id, session_key))
        .or_insert(SessionAffinity {
            target_id,
            last_used: now,
        });
    if affinity.target_id == target_id {
        affinity.last_used = now;
    }
    while affinities.len() > MAX_SESSION_AFFINITY_ENTRIES {
        let Some(oldest_key) = affinities
            .iter()
            .min_by_key(|(_, affinity)| affinity.last_used)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        affinities.remove(&oldest_key);
    }
}

pub(crate) fn clear_affinity_for_target(state: &RuntimeState, target_id: Uuid) {
    if let Ok(mut affinities) = state.session_affinity.lock() {
        affinities.retain(|_, affinity| affinity.target_id != target_id);
    }
}

pub(crate) fn clear_rejected_session(
    state: &RuntimeState,
    route_id: Uuid,
    session_key: Option<&str>,
    target_id: Uuid,
) {
    if let Some(key) = session_key {
        if let Ok(mut affinities) = state.session_affinity.lock() {
            affinities.retain(|(route, session), affinity| {
                *route != route_id || session != key || affinity.target_id != target_id
            });
        }
    }
}

#[cfg(test)]
pub(crate) fn select_route_targets(
    state: &RuntimeState,
    route: &ResolvedRoute,
) -> Vec<ResolvedTarget> {
    select_route_targets_with_affinity(state, route, None)
}

pub(crate) fn select_route_targets_with_affinity(
    state: &RuntimeState,
    route: &ResolvedRoute,
    session_key: Option<&str>,
) -> Vec<ResolvedTarget> {
    let mut targets = ordered_route_targets(state, route, session_key);
    targets.truncate(usize::from(route.config.retry.max_attempts.max(1)));
    targets
}

pub(crate) fn ordered_route_targets(
    state: &RuntimeState,
    route: &ResolvedRoute,
    session_key: Option<&str>,
) -> Vec<ResolvedTarget> {
    let mut targets = route.targets.clone();
    targets.retain(|target| target.config.enabled);
    targets.sort_by_key(|target| target.config.priority);
    targets.retain(|target| !circuit_open(state, target.config.id));
    // Weight only eligible peers in the best stability tier, so an unavailable
    // or degraded high-weight provider cannot donate traffic to another peer.
    let ranks = state
        .health
        .lock()
        .map(|health| {
            let now = Instant::now();
            targets.retain(|target| {
                !health
                    .get(&target.config.id)
                    .is_some_and(|h| h.probe_id.is_some())
            });
            targets
                .iter()
                .map(|target| {
                    (
                        target.config.id,
                        health
                            .get(&target.config.id)
                            .map_or(0, |h| h.stability_rank(now)),
                    )
                })
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    targets.sort_by_key(|target| ranks.get(&target.config.id).copied().unwrap_or_default());
    if route.config.strategy == RouteStrategy::RoundRobin && !targets.is_empty() {
        let best_rank = ranks.get(&targets[0].config.id);
        let peer_count = targets
            .iter()
            .take_while(|target| ranks.get(&target.config.id) == best_rank)
            .count();
        let start = round_robin_start(
            state,
            route.config.id,
            &targets[..peer_count]
                .iter()
                .map(|target| target.config.weight)
                .collect::<Vec<_>>(),
        );
        targets[..peer_count].rotate_left(start);
    }
    if let Some(target_id) = affinity_target(state, route.config.id, session_key, &route.targets) {
        if let Some(index) = targets
            .iter()
            .position(|target| target.config.id == target_id)
        {
            let preferred = targets.remove(index);
            targets.insert(0, preferred);
        }
    }
    targets
}

pub(crate) fn round_robin_start(state: &RuntimeState, route_id: Uuid, weights: &[u32]) -> usize {
    let counter = {
        let Ok(mut counters) = state.rr_counters.lock() else {
            return 0;
        };
        counters
            .entry(route_id)
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed)
    };
    weighted_start_index(counter, weights)
}

pub(crate) fn weighted_start_index(counter: u64, weights: &[u32]) -> usize {
    if weights.is_empty() {
        return 0;
    }
    let total: u64 = weights.iter().map(|weight| u64::from(*weight)).sum();
    if total == 0 {
        return (counter % weights.len() as u64) as usize;
    }
    let mut position = counter % total;
    for (index, weight) in weights.iter().enumerate() {
        let weight = u64::from(*weight);
        if position < weight {
            return index;
        }
        position -= weight;
    }
    0
}

pub(crate) fn circuit_open(state: &RuntimeState, target_id: Uuid) -> bool {
    state
        .health
        .lock()
        .ok()
        .and_then(|mut health| {
            health
                .get_mut(&target_id)
                .map(|target| target.circuit_open(Instant::now()))
        })
        .unwrap_or(false)
}

pub(crate) fn mark_failure(state: &RuntimeState, target_id: Uuid, policy: &RetryPolicy) {
    if let Ok(mut health) = state.health.lock() {
        let target = health.entry(target_id).or_default();
        let previous = target.open_until;
        let now = Instant::now();
        target.fail(policy, now);
        if target.open_until != previous {
            if let Some(until) = target.open_until {
                state.usage.log_diagnostic("warn", format!(
                    "event=proxy.target.cooldown target_id={target_id} cooldown_ms={} reopen_count={}",
                    until.saturating_duration_since(now).as_millis(), target.reopen_count,
                ));
            }
        }
        clear_affinity_for_target(state, target_id);
    }
}

#[cfg(test)]
pub(crate) fn mark_success(state: &RuntimeState, target_id: Uuid) {
    complete_target_success(state, Uuid::nil(), None, target_id, Instant::now(), None);
}

pub(crate) fn is_retryable_status(status: StatusCode) -> bool {
    // A response that reached the upstream can still be a target failure:
    // quota/authentication errors (including 403 insufficient balance),
    // client errors, redirects and server errors all need to advance the
    // fallback chain before anything is committed to the caller.
    status.is_redirection() || status.is_client_error() || status.is_server_error()
}

pub(crate) fn status_affects_circuit(status: StatusCode) -> bool {
    status.is_server_error()
        || matches!(
            status,
            StatusCode::UNAUTHORIZED
                | StatusCode::FORBIDDEN
                | StatusCode::REQUEST_TIMEOUT
                | StatusCode::TOO_MANY_REQUESTS
        )
}
