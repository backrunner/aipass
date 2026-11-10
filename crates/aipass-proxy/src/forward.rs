mod response;
use response::deliver_upstream_response;
mod http_attempt;
mod prepare;
use self::http_attempt::run_http_attempt;
use prepare::prepare_upstream_request;

use super::*;

mod body;
pub(crate) use body::*;

pub(crate) async fn handle_request(
    mut request: Request<Incoming>,
    state: RuntimeState,
) -> Result<Response<BoxBody>, Infallible> {
    let request_id = Uuid::new_v4();
    request.extensions_mut().insert(request_id);
    let health = request.uri().path().trim_end_matches('/') == "/health";
    let started = Instant::now();
    if !health {
        state.usage.log_diagnostic(
            "info",
            format!("event=proxy.http.received request_id={request_id}"),
        );
    }
    let response = handle_request_inner(request, state.clone()).await?;
    if !health || !response.status().is_success() {
        state.usage.log_diagnostic(
            if response.status().is_client_error() || response.status().is_server_error() {
                "warn"
            } else {
                "info"
            },
            format!(
                "event=proxy.http.response_headers request_id={request_id} status={} elapsed_ms={}",
                response.status().as_u16(),
                started.elapsed().as_millis()
            ),
        );
    }
    Ok(response)
}

pub(crate) fn attach_in_flight_guard<G: Send + 'static>(
    response: Response<BoxBody>,
    guard: Option<G>,
) -> Response<BoxBody> {
    let Some(guard) = guard else {
        return response;
    };
    let (parts, body) = response.into_parts();
    let body = body
        .map_frame(move |frame| {
            // Keep the request counted until the body is fully consumed or
            // dropped, which includes long-lived HTTP streaming responses.
            let _keep_alive = &guard;
            frame
        })
        .boxed_unsync();
    Response::from_parts(parts, body)
}

pub(crate) async fn handle_request_inner(
    request: Request<Incoming>,
    state: RuntimeState,
) -> Result<Response<BoxBody>, Infallible> {
    let mut config_changed = ConfigWatch::subscribe(&state);
    let request_id = *request
        .extensions()
        .get::<Uuid>()
        .expect("assigned by HTTP entry point");
    if websocket::is_upgrade_request(&request) {
        return Ok(websocket::handle_request(request, state).await);
    }
    let started = Instant::now();
    let started_at = now_unix();
    let path = request.uri().path().to_string();
    let method = request.method().clone();
    let request_query = request.uri().query().map(str::to_owned);
    if path.trim_end_matches('/') == crate::search::MCP_PATH {
        return Ok(crate::search::handle(request, state).await);
    }
    if path.trim_end_matches('/') == "/health" {
        return Ok(handle_local_health_request(request, &state).await);
    }
    if path.trim_end_matches('/') == "/v1/models" {
        let mut changed = config_changed;
        return Ok(tokio::select! {
            biased;
            _ = changed.changed() => error_response(StatusCode::SERVICE_UNAVAILABLE, "proxy configuration changed; reconnect"),
            response = handle_models_request(request, state) => response,
        });
    }
    let image_operation = images::Operation::from_path(&path);
    let inbound = ProxyProtocol::from_path(&path);
    if inbound.is_none() && image_operation.is_none() {
        return Ok(error_response(
            StatusCode::NOT_FOUND,
            "unsupported proxy path",
        ));
    }
    if image_operation.is_some() && method != http::Method::POST {
        return Ok(error_response(
            StatusCode::METHOD_NOT_ALLOWED,
            "Images API requires POST",
        ));
    }
    let incoming_headers = request.headers().clone();
    let (bearer_token, api_key_token) = local_proxy_tokens(&incoming_headers);
    if bearer_token.is_none() && api_key_token.is_none() {
        return Ok(error_response(
            StatusCode::UNAUTHORIZED,
            "missing local proxy token",
        ));
    }
    let selected = select_route(&state, bearer_token, api_key_token, inbound)
        .filter(|(route, _)| image_operation.is_none() || images::accepts_route(route));
    let Some((mut route, pricing)) = selected else {
        return Ok(error_response(
            StatusCode::UNAUTHORIZED,
            "invalid local proxy token or route",
        ));
    };
    config_changed.scope(route.config.id);
    route.local_token.zeroize();
    let body = match read_replayable_request_body(request.into_body()).await {
        Ok(body) => body,
        Err(RequestBodyReadError::TooLarge) => {
            return Ok(error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                "proxy request body too large",
            ))
        }
        Err(RequestBodyReadError::Io(err)) => {
            return Ok(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("failed to buffer proxy request body: {err}"),
            ))
        }
        Err(RequestBodyReadError::Transport(err)) => {
            return Ok(error_response(StatusCode::BAD_REQUEST, &err))
        }
    };
    forward_request(
        ForwardRequest {
            image_operation,
            websocket: false,
            upstream_pool: None,
            affinity_fallback: None,
            config_changed,
            request_id,
            method,
            request_query,
            incoming_headers,
            body,
            started,
            started_at,
        },
        state,
        route,
        pricing,
    )
    .await
}

pub(crate) struct ForwardRequest {
    pub(crate) image_operation: Option<images::Operation>,
    pub(crate) websocket: bool,
    pub(crate) upstream_pool: Option<Arc<websocket::pool::Pool>>,
    pub(crate) affinity_fallback: Option<String>,
    pub(crate) config_changed: ConfigWatch,
    pub(crate) request_id: Uuid,
    pub(crate) method: http::Method,
    pub(crate) request_query: Option<String>,
    pub(crate) incoming_headers: HeaderMap,
    pub(crate) body: ReplayableRequestBody,
    pub(crate) started: Instant,
    pub(crate) started_at: i64,
}

#[derive(Clone, Copy)]
pub(crate) struct UpstreamIdentity {
    pub(crate) target_id: Uuid,
}

// Shared HTTP/SSE execution, including conversion, failover and usage tracking.
// WS adaptation calls this directly after its authenticated upgrade.
pub(crate) async fn forward_request(
    request: ForwardRequest,
    state: RuntimeState,
    route: ResolvedRoute,
    pricing: Vec<ModelPricing>,
) -> Result<Response<BoxBody>, Infallible> {
    let guard = InFlightGuard::new(state.in_flight_requests.clone());
    let mut changed = request.config_changed.clone();
    if changed.has_changed().unwrap_or(true) {
        return Ok(error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "proxy configuration changed; reconnect",
        ));
    }
    let response = tokio::select! {
        biased;
        _ = changed.changed() => error_response(StatusCode::SERVICE_UNAVAILABLE, "proxy configuration changed; reconnect"),
        response = crate::search::forward(request, state, route, pricing) => response?,
    };
    Ok(attach_in_flight_guard(response, Some(guard)))
}

/// What the forwarding loop should do after one target attempt.
enum AttemptDisposition {
    /// Outbound request is assembled and ready to send.
    Dispatch(Box<http_attempt::Prepared>),
    /// Completed upstream response; the caller attaches the activity guard.
    Response(Response<BoxBody>),
    /// Client-visible failure returned immediately, without retrying.
    Abort(Response<BoxBody>),
    /// This request cannot be represented by this target. No request was sent.
    Skip(String),
    /// Record the failure and move on to the next target or hold round.
    Retry {
        error: String,
        /// Whether the upstream may already have started generation; a
        /// submitted attempt must never be replayed on another target.
        generation_submitted: bool,
    },
}

/// Outcome of the WebSocket fast path for one target attempt.
enum WsDisposition {
    /// WebSocket transport is unavailable; continue over HTTP.
    Fallback,
    Response(Response<BoxBody>),
    Rejected(StatusCode),
}

/// Shared context for one target attempt inside the forwarding loop.
struct AttemptContext<'a> {
    state: &'a RuntimeState,
    route: &'a ResolvedRoute,
    target: &'a ResolvedTarget,
    pricing: &'a [ModelPricing],
    request_id: Uuid,
    method: &'a http::Method,
    request_query: Option<&'a str>,
    incoming_headers: &'a HeaderMap,
    body: &'a ReplayableRequestBody,
    request_json: Option<&'a serde_json::Value>,
    session_key: Option<&'a str>,
    streaming_request: bool,
    websocket: bool,
    config_changed: &'a ConfigWatch,
    hold_deadline: Option<tokio::time::Instant>,
    started: Instant,
    started_at: i64,
    attempt_started: Instant,
    attempt_started_at: i64,
    attempts: u8,
    ws_evidence: Option<websocket::capability::Evidence>,
    model: &'a Option<String>,
}

impl AttemptContext<'_> {
    fn conversion_error(
        &self,
        error: aipass_proxy_conversion::ConversionError,
    ) -> AttemptDisposition {
        if matches!(
            error,
            aipass_proxy_conversion::ConversionError::UnsupportedFeature { .. }
                | aipass_proxy_conversion::ConversionError::Unsupported(..)
        ) {
            self.state.usage.log_diagnostic(
                "info",
                format!(
                    "event=proxy.target.incompatible request_id={} target_id={} reason={error}",
                    self.request_id, self.target.config.id
                ),
            );
            AttemptDisposition::Skip(error.to_string())
        } else {
            AttemptDisposition::Abort(error_response(StatusCode::BAD_REQUEST, &error.to_string()))
        }
    }
    /// Persist a failed attempt, trip the circuit breaker, and report the
    /// error so the loop can move on to the next target.
    fn fail(
        &self,
        status: Option<StatusCode>,
        first_token_ms: Option<u64>,
        generation_submitted: bool,
        message: impl Into<String>,
    ) -> AttemptDisposition {
        mark_failure(self.state, self.target.config.id, &self.route.config.retry);
        persist_attempt(
            &self.state.usage,
            (self.request_id, self.route.config.id),
            self.target,
            self.model.as_deref(),
            self.attempt_started_at,
            self.attempt_started,
            AttemptOutcome::failure(status, first_token_ms),
        );
        AttemptDisposition::Retry {
            error: message.into(),
            generation_submitted,
        }
    }
}

/// Try the native WebSocket transport for Responses targets before falling
/// back to plain HTTP forwarding.
async fn run_ws_attempt(
    ctx: &mut AttemptContext<'_>,
    upstream_pool: &Option<Arc<websocket::pool::Pool>>,
) -> WsDisposition {
    match websocket::upstream::forward(websocket::upstream::RequestContext {
        state: ctx.state,
        route: ctx.route,
        target: ctx.target,
        pricing: ctx.pricing,
        incoming_headers: ctx.incoming_headers,
        session_key: ctx.session_key,
        query: ctx.request_query,
        body: ctx.body,
        request_id: ctx.request_id,
        attempts: &mut ctx.attempts,
        hold_deadline: ctx.hold_deadline,
        pool: upstream_pool.clone(),
        config_changed: ctx.config_changed.clone(),
        streaming: ctx.streaming_request,
    })
    .await
    {
        websocket::upstream::ForwardOutcome::Response(response) => {
            WsDisposition::Response(response)
        }
        websocket::upstream::ForwardOutcome::HttpFallback(evidence) => {
            ctx.ws_evidence = evidence;
            WsDisposition::Fallback
        }
        websocket::upstream::ForwardOutcome::Rejected(status) => WsDisposition::Rejected(status),
    }
}

/// Process the upstream response body of an attempt and produce the final
/// client-facing response, or a retry disposition.
pub(crate) async fn forward_request_inner(
    request: ForwardRequest,
    state: RuntimeState,
    route: ResolvedRoute,
    pricing: Vec<ModelPricing>,
) -> Result<Response<BoxBody>, Infallible> {
    if let Some(operation) = request.image_operation {
        if model_routes::model_id(&route).is_some() {
            return Ok(error_response(
                StatusCode::BAD_REQUEST,
                "group model routing supports conversation endpoints only",
            ));
        }
        return Ok(images::forward(request, state, route, operation).await);
    }
    let ForwardRequest {
        image_operation: _,
        websocket,
        upstream_pool,
        affinity_fallback,
        config_changed,
        request_id,
        method,
        request_query,
        incoming_headers,
        mut body,
        started,
        started_at,
    } = request;
    state.usage.log_diagnostic(
        "info",
        format!(
            "event=proxy.request.started request_id={request_id} route_id={}",
            route.config.id
        ),
    );
    let mut request_json = if route.config.conversion_enabled
        || route.targets.iter().any(|target| {
            target.upstream_kind != UpstreamKind::Standard
                || target.profile != ProviderProfile::Generic
                || target.config.model.is_some()
        }) {
        body.json().await
    } else {
        None
    };
    if request_json.as_ref().is_some_and(|v| !v.is_object()) {
        return Ok(error_response(
            StatusCode::BAD_REQUEST,
            "request must be a JSON object",
        ));
    }
    let request_metadata = body.metadata().await.unwrap_or_default();
    let tool_summary = body
        .parse_metadata::<diagnostics::protocol::RequestSummary>()
        .await;
    diagnostics::protocol::RequestSummary::log(
        tool_summary.as_ref(),
        &state.usage,
        request_id,
        if websocket {
            "ws_bridge_prepared"
        } else {
            "http_inbound"
        },
    );
    let session_key =
        session_affinity_key(&incoming_headers, Some(&request_metadata)).or(affinity_fallback);
    let streaming_request = request_metadata.stream;
    let model = request_metadata.model;
    if let Err(error) =
        model_routes::validate_request(&route, model.as_deref(), request_json.as_ref())
    {
        return Ok(error_response(StatusCode::BAD_REQUEST, &error));
    }
    if let Some(payload) = request_json.as_mut() {
        if model_routes::make_portable(&route, payload) {
            // Native HTTP and the websocket bridge read the replayable body,
            // while conversions and subscription backends read request_json.
            // Both paths must see the same history and identifiers.
            body = ReplayableRequestBody::Memory(Bytes::from(payload.to_string()));
        }
    }
    let mut last_error = None;
    let mut ws_rejection = None;
    // Keep an identity for a request that is rejected after every target is
    // filtered by its circuit breaker. Such failures still belong in the
    // request-level usage history and must not disappear from the denominator.
    let mut failure_target = route
        .targets
        .iter()
        .find(|target| target.config.enabled)
        .map(|target| {
            (
                target.config.provider_entry_id,
                target.config.secret_id.clone(),
            )
        });
    let history_owner = if let Some(payload) = request_json.as_ref() {
        let owner = (|| -> Result<Option<Uuid>, String> {
            let gemini = state
                .gemini_signatures
                .lock()
                .map_err(|_| "Gemini session unavailable")?
                .history_owner(payload)?;
            let claude = state
                .subscription_backend
                .read()
                .map_err(|_| "Claude session unavailable")?
                .as_ref()
                .map(|backend| {
                    backend.history_owner_for_session(
                        payload,
                        &route.targets,
                        session_key.as_deref(),
                    )
                })
                .transpose()?
                .flatten();
            if gemini.is_some() && claude.is_some() {
                return Err("history mixes provider-bound tool sessions".into());
            }
            model_routes::history_owner(
                &state,
                &route,
                payload,
                session_key.as_deref(),
                gemini.or(claude),
            )
        })();
        match owner {
            Ok(owner) => owner,
            Err(error) => return Ok(error_response(StatusCode::BAD_REQUEST, &error)),
        }
    } else {
        None
    };
    let mut target_attempts = 0u8;
    let mut generation_submitted = false;
    let mut capacity_skipped = false;
    let mut incompatible_targets = std::collections::HashSet::new();
    let mut hold_round = 0u32;
    let hold_deadline = hold_deadline(&route.config.retry, started);
    'hold: loop {
        if hold_deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
            break;
        }
        for _round in 0..silent_retry_rounds(&route.config.retry) {
            let targets = live_route_targets_for_model(
                &state,
                &route,
                session_key.as_deref(),
                model.as_deref(),
            );
            let mut round_attempts = 0u8;
            for mut target in targets {
                if incompatible_targets.contains(&target.config.id) {
                    continue;
                }
                if history_owner.is_some_and(|owner| owner != target.config.id) {
                    continue;
                }
                if round_attempts >= route.config.retry.max_attempts.max(1) {
                    break;
                }
                if generation_submitted {
                    break 'hold;
                }
                if hold_deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
                    break 'hold;
                }
                if target.upstream_kind.is_copilot() {
                    let upstream_model = target.model_override.clone().or_else(|| model.clone());
                    if let Err(error) = copilot::resolve(
                        &state,
                        &mut target,
                        upstream_model.as_deref(),
                        route.config.inbound_protocol,
                        request_json.as_ref(),
                        route.config.retry.connect_timeout_ms,
                    )
                    .await
                    {
                        last_error = Some(error);
                        continue;
                    }
                    if target
                        .config
                        .effective_protocol(route.config.upstream_protocol)
                        != route.config.inbound_protocol
                        && !route.config.conversion_enabled
                    {
                        return Ok(error_response(
                            StatusCode::BAD_REQUEST,
                            "Copilot model requires another protocol; enable route conversion",
                        ));
                    }
                }
                let Some(recovery) = RecoveryPermit::acquire(&state, target.config.id) else {
                    continue;
                };
                let Some(provider_permit) = ProviderPermit::acquire(&state, &target) else {
                    capacity_skipped = true;
                    continue;
                };
                round_attempts += 1;
                let target_activity = (
                    TargetActivityGuard::new(&state, target.config.id),
                    recovery,
                    provider_permit,
                );
                target_attempts = target_attempts.saturating_add(1);
                failure_target = Some((
                    target.config.provider_entry_id,
                    target.config.secret_id.clone(),
                ));
                let effective_model = target.model_override.clone().or_else(|| model.clone());
                let mut ctx = AttemptContext {
                    state: &state,
                    route: &route,
                    target: &target,
                    pricing: &pricing,
                    request_id,
                    method: &method,
                    request_query: request_query.as_deref(),
                    incoming_headers: &incoming_headers,
                    body: &body,
                    request_json: request_json.as_ref(),
                    session_key: session_key.as_deref(),
                    streaming_request,
                    websocket,
                    config_changed: &config_changed,
                    hold_deadline,
                    started,
                    started_at,
                    attempt_started: Instant::now(),
                    attempt_started_at: now_unix(),
                    attempts: target_attempts,
                    ws_evidence: None,
                    model: &effective_model,
                };
                if method == http::Method::POST
                    && route.config.inbound_protocol == ProxyProtocol::OpenAiResponses
                    && target.supports_websockets
                    && target.upstream_kind == UpstreamKind::Standard
                    && target
                        .config
                        .effective_protocol(route.config.upstream_protocol)
                        == ProxyProtocol::OpenAiResponses
                {
                    match run_ws_attempt(&mut ctx, &upstream_pool).await {
                        WsDisposition::Response(response) => {
                            return Ok(attach_in_flight_guard(response, Some(target_activity)))
                        }
                        WsDisposition::Fallback => {}
                        WsDisposition::Rejected(status) => {
                            last_error = Some(format!("WebSocket request rejected ({status})"));
                            ws_rejection = Some(status);
                            target_attempts = ctx.attempts;
                            if status_affects_circuit(status) {
                                mark_failure(&state, target.config.id, &route.config.retry);
                            }
                            continue;
                        }
                    }
                    if hold_deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline)
                    {
                        break 'hold;
                    }
                    target_attempts = ctx.attempts;
                }
                match run_http_attempt(&ctx).await {
                    AttemptDisposition::Response(response) => {
                        return Ok(attach_in_flight_guard(response, Some(target_activity)))
                    }
                    AttemptDisposition::Abort(response) => return Ok(response),
                    AttemptDisposition::Skip(error) => {
                        incompatible_targets.insert(target.config.id);
                        last_error = Some(error);
                        round_attempts = round_attempts.saturating_sub(1);
                        target_attempts = target_attempts.saturating_sub(1);
                    }
                    AttemptDisposition::Dispatch(_) => {
                        unreachable!("dispatch is consumed inside run_http_attempt")
                    }
                    AttemptDisposition::Retry {
                        error,
                        generation_submitted: submitted,
                    } => {
                        last_error = Some(error);
                        generation_submitted = submitted;
                        continue;
                    }
                }
            }
        }
        if generation_submitted {
            break;
        }
        if route
            .targets
            .iter()
            .filter(|t| t.config.enabled && history_owner.is_none_or(|owner| owner == t.config.id))
            .all(|t| incompatible_targets.contains(&t.config.id))
        {
            break;
        }
        let retry = &route.config.retry;
        if !retry.hold_on_failure || !route.targets.iter().any(|target| target.config.enabled) {
            break;
        }
        let elapsed = started.elapsed();
        let mut delay = hold_backoff_delay(retry, hold_round);
        if retry.hold_max_duration_ms > 0 {
            let budget = Duration::from_millis(retry.hold_max_duration_ms);
            if elapsed >= budget {
                break;
            }
            delay = delay.min(budget - elapsed);
        }
        tokio::time::sleep(delay).await;
        hold_round = hold_round.saturating_add(1);
    }

    let at_capacity = capacity_skipped && target_attempts == 0;
    let incompatible = !at_capacity && target_attempts == 0 && !incompatible_targets.is_empty();
    let final_status = if incompatible {
        StatusCode::BAD_REQUEST
    } else if at_capacity {
        StatusCode::TOO_MANY_REQUESTS
    } else {
        ws_rejection.unwrap_or(StatusCode::BAD_GATEWAY)
    };
    let message = if incompatible {
        "request cannot be preserved by the available targets"
    } else if at_capacity {
        "all available providers are at their concurrency limit"
    } else {
        "all upstream targets failed"
    };
    let diagnostic = last_error.unwrap_or_else(|| message.into());
    record_request(&state, false, None);
    set_error(&state, diagnostic);
    if let Some((provider_entry_id, secret_id)) = failure_target {
        let _ = state.usage.record(&UsageRecord {
            id: request_id,
            started_at,
            duration_ms: started.elapsed().as_millis() as u64,
            first_token_ms: None,
            route_id: route.config.id,
            provider_entry_id,
            secret_id,
            model,
            inbound_protocol: route.config.inbound_protocol,
            upstream_protocol: route.config.upstream_protocol,
            status: final_status.as_u16(),
            attempts: target_attempts,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            cache_creation_1h_tokens: 0,
            estimated_cost_micros: 0,
        });
    }
    Ok(if at_capacity {
        capacity_response()
    } else {
        error_response(final_status, message)
    })
}

pub(crate) struct AttemptOutcome {
    pub(crate) first_token_ms: Option<u64>,
    pub(crate) status: Option<u16>,
    pub(crate) success: bool,
}

impl AttemptOutcome {
    pub(crate) fn failure(status: Option<StatusCode>, first_token_ms: Option<u64>) -> Self {
        Self {
            first_token_ms,
            status: status.map(|status| status.as_u16()),
            success: false,
        }
    }

    pub(crate) fn success(status: StatusCode, first_token_ms: Option<u64>) -> Self {
        Self {
            first_token_ms,
            status: Some(status.as_u16()),
            success: true,
        }
    }
}

pub(crate) fn persist_attempt(
    store: &UsageStore,
    ids: (Uuid, Uuid),
    target: &ResolvedTarget,
    model: Option<&str>,
    started_at: i64,
    started: Instant,
    outcome: AttemptOutcome,
) {
    let (request_id, route_id) = ids;
    let _ = store.record_attempt(&AttemptRecord {
        id: Uuid::new_v4(),
        request_id: Some(request_id),
        started_at,
        duration_ms: started.elapsed().as_millis() as u64,
        first_token_ms: outcome.first_token_ms,
        route_id,
        target_id: target.config.id,
        provider_entry_id: target.config.provider_entry_id,
        secret_id: target.config.secret_id.clone(),
        model: model.map(str::to_owned),
        status: outcome.status,
        success: Some(outcome.success),
    });
}

pub(crate) fn request_stream_usage(
    protocol: ProxyProtocol,
    streaming: bool,
    payload: Bytes,
) -> Bytes {
    if !streaming || protocol != ProxyProtocol::OpenAiChatCompletions {
        return payload;
    }
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&payload) else {
        return payload;
    };
    let Some(object) = value.as_object_mut() else {
        return payload;
    };
    match object.get_mut("stream_options") {
        Some(serde_json::Value::Object(options)) => {
            options.insert("include_usage".into(), serde_json::Value::Bool(true));
        }
        Some(serde_json::Value::Null) | None => {
            object.insert(
                "stream_options".into(),
                serde_json::json!({ "include_usage": true }),
            );
        }
        Some(_) => return payload,
    }
    serde_json::to_vec(&value).map_or(payload, Bytes::from)
}

pub(crate) fn local_token_redactions(headers: &HeaderMap) -> [&str; 2] {
    let (bearer, api_key) = local_proxy_tokens(headers);
    [bearer.unwrap_or_default(), api_key.unwrap_or_default()]
}

pub(crate) fn local_proxy_tokens(headers: &HeaderMap) -> (Option<&str>, Option<&str>) {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            let (scheme, token) = value.split_once(' ')?;
            scheme
                .eq_ignore_ascii_case("bearer")
                .then_some(token.trim())
        })
        .filter(|token| !token.is_empty());
    let api_key = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|token| !token.is_empty());
    (bearer, api_key)
}

pub(crate) fn error_response(status: StatusCode, message: &str) -> Response<BoxBody> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(BodyExt::boxed_unsync(
            Full::new(Bytes::from(
                serde_json::json!({"error":{"message":message,"type":"aipass_proxy_error"}})
                    .to_string(),
            ))
            .map_err(|never| -> BoxError { match never {} }),
        ))
        .unwrap()
}
