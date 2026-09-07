//! Responses WebSocket transport. Upgrade through the shared reqwest client so
//! TLS, outbound proxy settings and credential injection match HTTP requests.
use super::*;
use diagnostics::protocol::{RequestSummary, ResponseTrace, WsDiagnostic};
use futures_util::SinkExt;

mod bridge;
pub(super) mod capability;
use capability::{ConnectContext, HandshakeError};
mod keepalive;
pub(super) mod pool;
mod probe;
pub(super) mod upstream;
pub use probe::{probe_websocket, WebsocketProbeResult};
use tokio_tungstenite::{
    tungstenite::{
        handshake::{derive_accept_key, server::create_response_with_body},
        protocol::{frame::coding::CloseCode, CloseFrame, Role, WebSocketConfig},
        Message,
    },
    WebSocketStream,
};

pub(super) fn is_upgrade_request(request: &Request<Incoming>) -> bool {
    request.headers().contains_key(header::UPGRADE)
        || request.headers().contains_key(header::SEC_WEBSOCKET_KEY)
}

fn empty_body() -> BoxBody {
    Full::new(Bytes::new())
        .map_err(|never| -> BoxError { match never {} })
        .boxed_unsync()
}

pub(super) async fn handle_request(
    mut request: Request<Incoming>,
    state: RuntimeState,
) -> Response<BoxBody> {
    let request_id = request
        .extensions()
        .get::<Uuid>()
        .copied()
        .unwrap_or_else(Uuid::new_v4);
    // Subscribe before resolving credentials, including changes during handshake.
    let mut config_changed = ConfigWatch::subscribe(&state);
    if request.uri().path().trim_end_matches('/') != "/v1/responses" {
        return error_response(StatusCode::NOT_FOUND, "unsupported WebSocket proxy path");
    }
    let (bearer, api_key) = local_proxy_tokens(request.headers());
    let session_key = session_affinity_key(request.headers(), None);
    let Some((mut route, pricing)) = select_route(
        &state,
        bearer,
        api_key,
        Some(ProxyProtocol::OpenAiResponses),
    ) else {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "invalid local proxy token or route",
        );
    };
    config_changed.scope(route.config.id);
    route.local_token.zeroize();
    route.config.token.zeroize();
    let mut downstream_response = match create_response_with_body(&request, empty_body) {
        Ok(response) => response,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid WebSocket handshake"),
    };
    // Select the actual upstream before deciding whether adaptation is needed.
    // Native Responses sessions remain pinned and application frames stay intact,
    // including when sibling targets use HTTP or a different protocol.
    let fallback_route = route.clone();
    let fallback_pool = Arc::new(pool::Pool::default());
    let client =
        match upstream_client_for_transport(&state, route.config.retry.connect_timeout_ms, true) {
            Ok(client) => client,
            Err(_) => {
                return error_response(
                    StatusCode::BAD_GATEWAY,
                    "failed to create WebSocket upstream client",
                )
            }
        };
    let started = Instant::now();
    let started_at = now_unix();
    let mut attempts = 0_u8;
    let mut last_status = StatusCode::BAD_GATEWAY;
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
    let mut hold_round = 0_u32;
    let hold_deadline = hold_deadline(&route.config.retry, started);
    'hold: loop {
        if hold_deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
            break;
        }
        for _ in 0..silent_retry_rounds(&route.config.retry) {
            for mut target in
                select_route_targets_with_affinity(&state, &route, session_key.as_deref())
            {
                if hold_deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
                    break 'hold;
                }
                if !target.supports_websockets
                    || target
                        .config
                        .effective_protocol(route.config.upstream_protocol)
                        != ProxyProtocol::OpenAiResponses
                {
                    state.usage.log_diagnostic("info", format!(
                        "event=proxy.websocket.bridge request_id={request_id} route_id={} reason=selected_target_requires_adaptation",
                        route.config.id,
                    ));
                    return bridge::upgrade(
                        request,
                        state,
                        fallback_route,
                        pricing,
                        config_changed,
                        downstream_response,
                        fallback_pool,
                    );
                }
                let capability_key = capability::key(&state, &target);
                let observation = capability::observe(&state, capability_key);
                if !capability::allowed(&state, capability_key)
                    || fallback_pool.fallback(capability_key).is_some()
                {
                    return bridge::upgrade(
                        request,
                        state,
                        fallback_route,
                        pricing,
                        config_changed,
                        downstream_response,
                        fallback_pool,
                    );
                }
                attempts = attempts.saturating_add(1);
                let attempt_started = Instant::now();
                let diagnostic = WsDiagnostic {
                    store: &state.usage,
                    request_id,
                    route_id: route.config.id,
                    provider_id: target.config.provider_entry_id,
                };
                let result = tokio::select! {
                    _ = config_changed.changed() => {
                        return error_response(StatusCode::SERVICE_UNAVAILABLE, "proxy configuration changed; reconnect");
                    }
                    result = capability::connect_twice(ConnectContext {
                        client: &client, headers: request.headers(), query: request.uri().query(),
                        target: &target, diagnostic: &diagnostic,
                        timeout: Duration::from_millis(route.config.retry.first_byte_timeout_ms.max(1)),
                        hold_deadline,
                        attempts: &mut attempts, model: None,
                    }) => result,
                };
                let (upstream, response_headers) = match result {
                    Ok(connected) => connected,
                    Err((error, repeated)) => {
                        let status = error.status();
                        last_status = status;
                        failure_target = Some((
                            target.config.provider_entry_id,
                            target.config.secret_id.clone(),
                        ));
                        if error.permits_fallback() {
                            let evidence = repeated
                                .then(|| {
                                    capability::candidate(
                                        &state,
                                        &target,
                                        observation,
                                        attempt_started,
                                        status,
                                    )
                                })
                                .flatten();
                            fallback_pool.set_fallback(capability_key, evidence);
                        }
                        continue;
                    }
                };
                // End-to-end headers include OpenAI connection metadata. The
                // handshake itself and compression are negotiated per hop.
                let hop_headers = connection_header_names(&response_headers);
                for (name, value) in &response_headers {
                    if !is_hop_header(name)
                        && !hop_headers.contains(name)
                        && name != header::SEC_WEBSOCKET_ACCEPT
                        && name != header::SEC_WEBSOCKET_EXTENSIONS
                        && name != header::CONTENT_LENGTH
                        && name != header::CONTENT_ENCODING
                    {
                        downstream_response
                            .headers_mut()
                            .append(name, value.clone());
                    }
                }
                let upgrade = hyper::upgrade::on(&mut request);
                // The upgraded connection no longer needs plaintext credentials.
                target.api_key.zeroize();
                for (_, value) in &mut target.config.headers {
                    value.zeroize();
                }
                target.config.headers.clear();
                let context = SessionUsage {
                    connection_id: request_id,
                    observation,
                    state,
                    target,
                    route_id: route.config.id,
                    retry: route.config.retry.clone(),
                    pricing,
                    attempts,
                    session_key,
                    pending: HashMap::new(),
                    active: HashMap::new(),
                };
                tokio::spawn(async move {
                    if let Ok(downstream) = upgrade.await {
                        relay(TokioIo::new(downstream), upstream, config_changed, context).await;
                    }
                });
                return downstream_response;
            }
        }
        let retry = &route.config.retry;
        if !retry.hold_on_failure {
            break;
        }
        let mut delay = hold_backoff_delay(retry, hold_round);
        if retry.hold_max_duration_ms > 0 {
            let budget = Duration::from_millis(retry.hold_max_duration_ms);
            if started.elapsed() >= budget {
                break;
            }
            delay = delay.min(budget.saturating_sub(started.elapsed()));
        }
        tokio::select! {
            _ = config_changed.changed() => {
                return error_response(StatusCode::SERVICE_UNAVAILABLE, "proxy configuration changed; reconnect");
            }
            _ = tokio::time::sleep(delay) => {}
        }
        hold_round = hold_round.saturating_add(1);
    }
    // Only handshakes have been attempted. Bridge on transport rejection;
    // auth/rate errors remain visible, and an exhausted hold budget stays final.
    if HandshakeError::from_status(last_status).permits_fallback()
        && !hold_deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline)
    {
        state.usage.log_diagnostic("info", format!(
            "event=proxy.websocket.bridge request_id={request_id} route_id={} reason=handshake_rejected status={}",
            route.config.id, last_status.as_u16(),
        ));
        return bridge::upgrade(
            request,
            state,
            fallback_route,
            pricing,
            config_changed,
            downstream_response,
            fallback_pool,
        );
    }
    record_request(&state, false, None);
    if let Some((provider_entry_id, secret_id)) = failure_target {
        let _ = state.usage.record(&UsageRecord {
            id: request_id,
            started_at,
            duration_ms: started.elapsed().as_millis() as u64,
            first_token_ms: None,
            route_id: route.config.id,
            provider_entry_id,
            secret_id,
            model: None,
            inbound_protocol: ProxyProtocol::OpenAiResponses,
            upstream_protocol: ProxyProtocol::OpenAiResponses,
            status: last_status.as_u16(),
            attempts,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            estimated_cost_micros: 0,
        });
    }
    set_error(
        &state,
        format!("WebSocket upstream handshake failed ({last_status})"),
    );
    // Keep HTTP rejection visible so SDKs can apply their own WS fallback.
    error_response(last_status, "all WebSocket upstream handshakes failed")
}

async fn connect_upstream(
    client: &reqwest::Client,
    incoming_headers: &HeaderMap,
    query: Option<&str>,
    target: &ResolvedTarget,
    diagnostic: Option<&WsDiagnostic<'_>>,
) -> Result<(reqwest::Upgraded, HeaderMap), HandshakeError> {
    let path = if target.config.auth_scheme == "azure_api_key" {
        "/responses"
    } else {
        "/v1/responses"
    };
    let url = upstream_url_with_query(&target.config.base_url, path, query)
        .map_err(|_| HandshakeError::Transport(StatusCode::BAD_GATEWAY))?;
    let mut headers =
        upstream_headers(incoming_headers, target).map_err(HandshakeError::Rejected)?;
    let key = tokio_tungstenite::tungstenite::handshake::client::generate_key();
    headers.insert(
        header::SEC_WEBSOCKET_KEY,
        HeaderValue::from_str(&key).unwrap(),
    );
    headers.insert(
        header::SEC_WEBSOCKET_VERSION,
        HeaderValue::from_static("13"),
    );
    headers.insert(header::CONNECTION, HeaderValue::from_static("Upgrade"));
    headers.insert(header::UPGRADE, HeaderValue::from_static("websocket"));
    connect_with_headers(client, url, headers, &key, diagnostic, target).await
}

fn upstream_headers(
    incoming_headers: &HeaderMap,
    target: &ResolvedTarget,
) -> Result<HeaderMap, StatusCode> {
    let mut headers =
        build_upstream_headers(incoming_headers, target, ProxyProtocol::OpenAiResponses)
            .map_err(|_| StatusCode::BAD_GATEWAY)?;
    // Never allow local credentials hidden in browser subprotocols to escape.
    // Native OpenAI clients authenticate using the existing local token headers.
    headers.remove(header::SEC_WEBSOCKET_PROTOCOL);
    headers.remove(header::SEC_WEBSOCKET_EXTENSIONS);
    headers.remove(header::SEC_WEBSOCKET_ACCEPT);
    headers.remove(header::SEC_WEBSOCKET_KEY);
    headers.remove(header::SEC_WEBSOCKET_VERSION);
    headers.remove(header::CONTENT_TYPE);
    headers.remove(header::CONTENT_LENGTH);
    headers
        .entry("openai-beta")
        .or_insert(HeaderValue::from_static("responses_websockets=2026-02-06"));
    Ok(headers)
}

async fn connect_with_headers(
    client: &reqwest::Client,
    url: String,
    headers: HeaderMap,
    key: &str,
    diagnostic: Option<&WsDiagnostic<'_>>,
    target: &ResolvedTarget,
) -> Result<(reqwest::Upgraded, HeaderMap), HandshakeError> {
    let response = client
        .get(url)
        .headers(headers)
        .send()
        .await
        .map_err(|err| {
            if let Some(diagnostic) = diagnostic {
                diagnostic.log("connect_failed", None, Some(&err));
            }
            HandshakeError::Transport(StatusCode::BAD_GATEWAY)
        })?;
    if response.status() != StatusCode::SWITCHING_PROTOCOLS {
        let status = response.status();
        if let Some(diagnostic) = diagnostic {
            diagnostic.log("http_rejected", Some(status), None);
            let detail = diagnostics::upstream::read_error(response, target, &[]).await;
            diagnostic.store.log_upstream_error(
                diagnostic.request_id,
                diagnostic.route_id,
                &target.config,
                status,
                "websocket",
                &detail,
            );
        }
        return Err(if status.is_client_error() || status.is_server_error() {
            HandshakeError::from_status(status)
        } else {
            HandshakeError::Transport(StatusCode::BAD_GATEWAY)
        });
    }
    let headers = response.headers();
    if !connection_header_names(headers).contains(&header::UPGRADE)
        || !headers
            .get(header::UPGRADE)
            .is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(b"websocket"))
        || !headers
            .get(header::SEC_WEBSOCKET_ACCEPT)
            .is_some_and(|v| v.as_bytes() == derive_accept_key(key.as_bytes()).as_bytes())
        || headers.contains_key(header::SEC_WEBSOCKET_EXTENSIONS)
        || headers.contains_key(header::SEC_WEBSOCKET_PROTOCOL)
    {
        if let Some(diagnostic) = diagnostic {
            diagnostic.log("invalid_upgrade", Some(response.status()), None);
        }
        return Err(HandshakeError::Transport(StatusCode::BAD_GATEWAY));
    }
    let headers = headers.clone();
    let upgraded = response.upgrade().await.map_err(|err| {
        if let Some(diagnostic) = diagnostic {
            diagnostic.log("upgrade_failed", None, Some(&err));
        }
        HandshakeError::Transport(StatusCode::BAD_GATEWAY)
    })?;
    if let Some(diagnostic) = diagnostic {
        diagnostic.log("connected", Some(StatusCode::SWITCHING_PROTOCOLS), None);
    }
    Ok((upgraded, headers))
}

async fn relay(
    downstream: TokioIo<hyper::upgrade::Upgraded>,
    upstream: reqwest::Upgraded,
    mut config_changed: ConfigWatch,
    mut usage: SessionUsage,
) {
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_BUFFERED_RESPONSE_BYTES))
        .max_frame_size(Some(MAX_BUFFERED_RESPONSE_BYTES));
    let mut downstream =
        WebSocketStream::from_raw_socket(downstream, Role::Server, Some(config)).await;
    let mut upstream = WebSocketStream::from_raw_socket(upstream, Role::Client, Some(config)).await;
    let write_timeout = Duration::from_millis(usage.retry.stream_idle_timeout_ms.max(1));
    let close_timeout = write_timeout.min(Duration::from_secs(1));
    let mut upstream_failed = false;
    let mut close_reason = "configuration_changed";
    let mut close_code = None;
    let mut heartbeat = keepalive::Heartbeat::new();
    loop {
        let (from_client, message) = tokio::select! {
            biased;
            _ = config_changed.changed() => {
                let close = Some(CloseFrame { code: CloseCode::Restart, reason: "proxy configuration changed; reconnect".into() });
                let _ = tokio::time::timeout(close_timeout, downstream.close(close.clone())).await;
                let _ = tokio::time::timeout(close_timeout, upstream.close(close)).await;
                break;
            }
            _ = tokio::time::sleep_until(heartbeat.deadline()) => {
                let result = tokio::select! {
                    biased;
                    _ = config_changed.changed() => break,
                    result = heartbeat.ping(&mut upstream) => result,
                };
                if result.is_err() {
                    close_reason = "upstream_heartbeat_failed";
                    upstream_failed = usage.has_requests();
                    break;
                }
                continue;
            }
            message = downstream.next() => (true, message),
            message = upstream.next() => (false, message),
        };
        let message = match message {
            Some(Ok(message)) => message,
            other => {
                close_reason = if from_client {
                    "downstream_read_ended"
                } else {
                    "upstream_read_ended"
                };
                if let Some(Err(err)) = other {
                    usage.diagnostic().log(close_reason, None, Some(&err));
                }
                upstream_failed = !from_client && usage.has_requests();
                break;
            }
        };
        if !from_client {
            if let Message::Pong(data) = &message {
                heartbeat.pong(data);
            }
        }
        if let Message::Text(text) = &message {
            if let Ok(value) = serde_json::from_str(text) {
                if from_client {
                    if !usage.client_event(&value) {
                        let mut error = serde_json::json!({"type":"error","status":503,"error":{"type":"server_error","code":"provider_temporarily_unavailable","message":"provider is cooling down or recovering; reconnect and resend full input"}});
                        if let Some(lane) = value.get("stream_id") {
                            error["stream_id"] = lane.clone();
                        }
                        if !matches!(
                            tokio::time::timeout(
                                write_timeout,
                                downstream.send(Message::text(error.to_string())),
                            )
                            .await,
                            Ok(Ok(()))
                        ) {
                            break;
                        }
                        continue;
                    }
                } else {
                    usage.server_event(&value);
                }
            }
        }
        let closing = message.is_close();
        if let Message::Close(frame) = &message {
            close_reason = if from_client {
                "downstream_close"
            } else {
                "upstream_close"
            };
            close_code = frame.as_ref().map(|frame| u16::from(frame.code));
        }
        // Ping/close replies are queued by tungstenite. Flush them on their
        // own hop; application text/binary messages are relayed unchanged.
        let send = tokio::time::timeout(write_timeout, async {
            if from_client {
                if message.is_ping() || closing {
                    downstream.flush().await?;
                }
                if !message.is_ping() && !message.is_pong() {
                    upstream.send(message).await?;
                }
            } else {
                if message.is_ping() || closing {
                    upstream.flush().await?;
                }
                if !message.is_ping() && !message.is_pong() {
                    downstream.send(message).await?;
                }
            }
            Ok::<_, tokio_tungstenite::tungstenite::Error>(())
        });
        let result = tokio::select! {
            _ = config_changed.changed() => break,
            result = send => result,
        };
        if closing || !matches!(result, Ok(Ok(()))) {
            if !closing {
                close_reason = if from_client {
                    "upstream_write_failed"
                } else {
                    "downstream_write_failed"
                };
                match &result {
                    Ok(Err(err)) => usage.diagnostic().log(close_reason, None, Some(err)),
                    Err(_) => usage.diagnostic().log("write_timeout", None, None),
                    _ => {}
                }
            }
            upstream_failed =
                usage.has_requests() && if closing { !from_client } else { from_client };
            break;
        }
    }
    // Never replay a committed WS session: previous_response_id is scoped to
    // its upstream connection, and a response may already have incurred usage.
    if upstream_failed {
        set_error(
            &usage.state,
            "WebSocket upstream disconnected or timed out before response completion".into(),
        );
    }
    usage.log_closed(close_reason, close_code);
    usage.disconnected(upstream_failed);
}

struct ResponseUsage {
    trace: ResponseTrace,
    activity: Option<(TargetActivityGuard, RecoveryPermit, InFlightGuard)>,
    request_id: Uuid,
    started: Instant,
    started_at: i64,
    model: Option<String>,
    session_key: Option<String>,
    response_id: Option<String>,
    first_token_ms: Option<u64>,
    stream_id: String,
    last_event: Instant,
}

impl ResponseUsage {
    fn new(value: &serde_json::Value, default_session_key: Option<&str>) -> Self {
        Self {
            trace: ResponseTrace::default(),
            activity: None,
            request_id: Uuid::new_v4(),
            started: Instant::now(),
            last_event: Instant::now(),
            started_at: now_unix(),
            model: value
                .get("model")
                .and_then(|v| v.as_str())
                .map(str::to_owned),
            session_key: session_affinity_key_from_value(value)
                .or_else(|| default_session_key.and_then(normalize_session_affinity_key)),
            response_id: None,
            first_token_ms: None,
            stream_id: value
                .get("stream_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned(),
        }
    }
}

struct SessionUsage {
    observation: Option<capability::Observation>,
    connection_id: Uuid,
    state: RuntimeState,
    target: ResolvedTarget,
    route_id: Uuid,
    retry: RetryPolicy,
    pricing: Vec<ModelPricing>,
    attempts: u8,
    session_key: Option<String>,
    pending: HashMap<String, VecDeque<ResponseUsage>>,
    active: HashMap<String, ResponseUsage>,
}

impl SessionUsage {
    fn diagnostic(&self) -> WsDiagnostic<'_> {
        WsDiagnostic {
            store: &self.state.usage,
            request_id: self.connection_id,
            route_id: self.route_id,
            provider_id: self.target.config.provider_entry_id,
        }
    }

    fn log_closed(&self, reason: &'static str, close_code: Option<u16>) {
        self.state.usage.log_diagnostic("info", format!(
            "event=proxy.websocket.closed connection_id={} route_id={} provider_id={} reason={reason} close_code={close_code:?} active={} pending={}",
            self.connection_id, self.route_id, self.target.config.provider_entry_id,
            self.active.len(), self.pending.values().map(VecDeque::len).sum::<usize>(),
        ));
    }

    fn has_requests(&self) -> bool {
        !self.pending.is_empty() || !self.active.is_empty()
    }

    fn advance_lane(&mut self, lane: &str) {
        if let Some(request) = self.pending.get_mut(lane).and_then(VecDeque::front_mut) {
            request.last_event = Instant::now();
        }
    }

    fn client_event(&mut self, value: &serde_json::Value) -> bool {
        self.client_event_with_id(value, Uuid::new_v4())
    }

    fn client_event_with_id(&mut self, value: &serde_json::Value, request_id: Uuid) -> bool {
        // Retain metadata only, never prompts, tool results or full WS events.
        // Cap bookkeeping independently of the upstream's multiplexing limits.
        if value["type"] != "response.create"
            || self.active.len() + self.pending.values().map(VecDeque::len).sum::<usize>() >= 1024
        {
            return true;
        }
        let Some(recovery) = RecoveryPermit::acquire(&self.state, self.target.config.id) else {
            return false;
        };
        let mut request = ResponseUsage::new(value, self.session_key.as_deref());
        request.request_id = request_id;
        request.activity = Some((
            TargetActivityGuard::new(&self.state, self.target.config.id),
            recovery,
            InFlightGuard::new(self.state.in_flight_requests.clone()),
        ));
        self.state.usage.log_diagnostic(
            "info",
            format!(
                "event=proxy.websocket.request.started request_id={} connection_id={} route_id={} provider_id={}",
                request.request_id, self.connection_id, self.route_id, self.target.config.provider_entry_id,
            ),
        );
        let summary = RequestSummary::deserialize(value).ok();
        RequestSummary::log(
            summary.as_ref(),
            &self.state.usage,
            request.request_id,
            "ws_upstream",
        );
        self.pending
            .entry(request.stream_id.clone())
            .or_default()
            .push_back(request);
        true
    }

    fn take_pending(&mut self, value: &serde_json::Value) -> Option<ResponseUsage> {
        let stream_id = value["stream_id"].as_str().unwrap_or_default();
        let pending = self
            .pending
            .get_mut(stream_id)
            .and_then(VecDeque::pop_front);
        if self.pending.get(stream_id).is_some_and(VecDeque::is_empty) {
            self.pending.remove(stream_id);
        }
        pending
    }

    fn server_event(&mut self, value: &serde_json::Value) {
        let kind = value["type"].as_str().unwrap_or_default();
        let id = value
            .pointer("/response/id")
            .or_else(|| value.get("response_id"))
            .and_then(|v| v.as_str());
        let lane = value["stream_id"].as_str().unwrap_or_default();
        let request = self
            .active
            .iter_mut()
            .find(|(response_id, request)| {
                id.map_or(request.stream_id == lane, |id| id == response_id.as_str())
            })
            .map(|(_, request)| request)
            .or_else(|| self.pending.get_mut(lane).and_then(VecDeque::front_mut));
        if let Some(request) = request {
            if let Some(id) = id.and_then(normalize_session_affinity_key) {
                request.response_id = Some(id);
            }
            request.trace.observe(value);
            if kind == "error" || kind == "response.failed" {
                let status = value["status"]
                    .as_u64()
                    .and_then(|status| u16::try_from(status).ok())
                    .and_then(|status| StatusCode::from_u16(status).ok())
                    .unwrap_or(StatusCode::BAD_GATEWAY);
                diagnostics::upstream::log_wire_error(
                    &self.state,
                    request.request_id,
                    self.route_id,
                    self.target.config.id,
                    status,
                    "websocket",
                    value,
                );
            }
        }
        if kind.starts_with("response.") {
            let lane = value["stream_id"].as_str().unwrap_or_default();
            for (response_id, request) in &mut self.active {
                if id.map_or(request.stream_id == lane, |id| id == response_id) {
                    request.last_event = Instant::now();
                }
            }
        }
        if matches!(kind, "response.created" | "response.in_progress") {
            if let Some(id) = id {
                if self.active.contains_key(id) {
                    return;
                }
                let Some(mut request) = self.take_pending(value) else {
                    return;
                };
                request.last_event = Instant::now();
                if self.active.len() < 1024 {
                    self.active.insert(id.to_owned(), request);
                }
            }
        } else if matches!(
            kind,
            "response.output_text.delta"
                | "response.function_call_arguments.delta"
                | "response.output_audio.delta"
        ) {
            let request = if let Some(id) = id {
                self.active.get_mut(id)
            } else {
                // Responses text/tool deltas identify their item and lane;
                // they do not always repeat the parent response_id.
                let stream_id = value["stream_id"].as_str().unwrap_or_default();
                self.active
                    .values_mut()
                    .find(|request| request.stream_id == stream_id)
            };
            if let Some(request) = request {
                request
                    .first_token_ms
                    .get_or_insert_with(|| request.started.elapsed().as_millis() as u64);
            }
        } else if matches!(
            kind,
            "response.completed" | "response.failed" | "response.incomplete" | "response.cancelled"
        ) {
            let request = id
                .and_then(|id| self.active.remove(id))
                .or_else(|| self.take_pending(value));
            let Some(mut request) = request else {
                return;
            };
            if let Some(model) = value.pointer("/response/model").and_then(|v| v.as_str()) {
                request.model = Some(model.to_owned());
            }
            let status = if matches!(kind, "response.completed" | "response.incomplete") {
                StatusCode::OK
            } else if kind == "response.cancelled" {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::BAD_GATEWAY
            };
            if capability::valid_completion(value) {
                capability::success(&self.state, self.observation);
            }
            self.advance_lane(&request.stream_id);
            self.finish(
                request,
                usage_from_wire_value(ProxyProtocol::OpenAiResponses, value),
                status,
                Some(status.is_success()),
                false,
            );
        } else if kind == "error" {
            let lane = value["stream_id"].as_str().unwrap_or_default();
            let request = id.and_then(|id| self.active.remove(id)).or_else(|| {
                if self.pending.contains_key(lane) {
                    self.take_pending(value)
                } else {
                    let active_id = self
                        .active
                        .iter()
                        .find(|(_, request)| request.stream_id == lane)
                        .map(|(id, _)| id.clone())?;
                    self.active.remove(&active_id)
                }
            });
            // A connection-level error with no request must not create usage.
            let Some(request) = request else {
                return;
            };
            let status = value["status"]
                .as_u64()
                .and_then(|v| u16::try_from(v).ok())
                .and_then(|v| StatusCode::from_u16(v).ok())
                .filter(|status| status.is_client_error() || status.is_server_error())
                .unwrap_or(StatusCode::BAD_GATEWAY);
            self.advance_lane(&request.stream_id);
            self.finish(request, TokenUsage::default(), status, Some(false), false);
        }
    }

    fn finish(
        &self,
        request: ResponseUsage,
        usage: TokenUsage,
        status: StatusCode,
        outcome: Option<bool>,
        transport_failure: bool,
    ) {
        request
            .trace
            .log(&self.state.usage, request.request_id, "websocket_upstream");
        let success = status.is_success();
        if success {
            complete_target_success(
                &self.state,
                self.route_id,
                request.session_key.as_deref(),
                self.target.config.id,
                request.started,
                request.response_id.as_deref(),
            );
        } else if !transport_failure && outcome == Some(false) && status_affects_circuit(status) {
            mark_failure(&self.state, self.target.config.id, &self.retry);
        }
        if outcome.is_some() {
            record_request(&self.state, success, request.first_token_ms);
        }
        record_recent_tokens(
            &self.state,
            usage
                .input_tokens
                .saturating_add(usage.output_tokens)
                .saturating_add(usage.cache_read_tokens)
                .saturating_add(usage.cache_creation_tokens),
        );
        let pricing = request.model.as_deref().and_then(|model| {
            self.pricing
                .iter()
                .filter(|p| model == p.model || model.starts_with(&p.model))
                .max_by_key(|p| p.model.len())
        });
        let _ = self.state.usage.record(&UsageRecord {
            id: request.request_id,
            started_at: request.started_at,
            duration_ms: request.started.elapsed().as_millis() as u64,
            first_token_ms: request.first_token_ms,
            route_id: self.route_id,
            provider_entry_id: self.target.config.provider_entry_id,
            secret_id: self.target.config.secret_id.clone(),
            model: request.model.clone(),
            inbound_protocol: ProxyProtocol::OpenAiResponses,
            upstream_protocol: ProxyProtocol::OpenAiResponses,
            status: status.as_u16(),
            attempts: self.attempts,
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_read_tokens: usage.cache_read_tokens,
            cache_creation_tokens: usage.cache_creation_tokens,
            estimated_cost_micros: pricing
                .map(|p| estimate_cost(&usage, p))
                .unwrap_or_default(),
        });
        let _ = self.state.usage.record_attempt(&AttemptRecord {
            id: Uuid::new_v4(),
            request_id: Some(request.request_id),
            started_at: request.started_at,
            duration_ms: request.started.elapsed().as_millis() as u64,
            first_token_ms: request.first_token_ms,
            route_id: self.route_id,
            target_id: self.target.config.id,
            provider_entry_id: self.target.config.provider_entry_id,
            secret_id: self.target.config.secret_id.clone(),
            model: request.model,
            status: Some(status.as_u16()),
            success: outcome,
        });
    }

    fn disconnected(&mut self, upstream_failed: bool) {
        let unfinished: Vec<_> = self
            .active
            .drain()
            .map(|(_, r)| r)
            .chain(self.pending.drain().flat_map(|(_, requests)| requests))
            .collect();
        for request in unfinished {
            self.finish(
                request,
                TokenUsage::default(),
                StatusCode::BAD_GATEWAY,
                upstream_failed.then_some(false),
                true,
            );
        }
    }
}

#[cfg(test)]
mod tests;
