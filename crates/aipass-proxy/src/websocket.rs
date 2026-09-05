//! Responses WebSocket transport. Upgrade through the shared reqwest client so
//! TLS, outbound proxy settings and credential injection match HTTP requests.
use super::*;
use futures_util::SinkExt;

mod bridge;
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
    let mut config_changed = state.config_changed.subscribe();
    if request.uri().path().trim_end_matches('/') != "/v1/responses" {
        return error_response(StatusCode::NOT_FOUND, "unsupported WebSocket proxy path");
    }
    let (bearer, api_key) = local_proxy_tokens(request.headers());
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
    route.local_token.zeroize();
    route.config.token.zeroize();
    let mut downstream_response = match create_response_with_body(&request, empty_body) {
        Ok(response) => response,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid WebSocket handshake"),
    };
    if route.config.conversion_enabled {
        return bridge::upgrade(
            request,
            state,
            route,
            pricing,
            config_changed,
            downstream_response,
        );
    }
    route.targets.retain(|target| {
        target
            .config
            .effective_protocol(route.config.upstream_protocol)
            == ProxyProtocol::OpenAiResponses
    });
    if !route.targets.iter().any(|target| target.config.enabled) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "WebSocket requires a native Responses upstream or enabled protocol conversion",
        );
    }
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
            for mut target in select_route_targets(&state, &route, hold_round > 0) {
                if hold_deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
                    break 'hold;
                }
                attempts = attempts.saturating_add(1);
                let attempt_started = Instant::now();
                let attempt_started_at = now_unix();
                let connect = connect_upstream(&client, &request, &target);
                let result = tokio::select! {
                    _ = config_changed.changed() => {
                        return error_response(StatusCode::SERVICE_UNAVAILABLE, "proxy configuration changed; reconnect");
                    }
                    result = tokio::time::timeout_at(
                        bounded_deadline(Duration::from_millis(route.config.retry.first_byte_timeout_ms.max(1)), hold_deadline),
                        connect,
                    ) => result.unwrap_or(Err(StatusCode::GATEWAY_TIMEOUT)),
                };
                let (upstream, response_headers) = match result {
                    Ok(connected) => connected,
                    Err(status) => {
                        last_status = status;
                        failure_target = Some((
                            target.config.provider_entry_id,
                            target.config.secret_id.clone(),
                        ));
                        if status_affects_circuit(status) {
                            mark_failure(&state, target.config.id, &route.config.retry);
                        }
                        persist_attempt(
                            &state.usage,
                            (request_id, route.config.id),
                            &target,
                            None,
                            attempt_started_at,
                            attempt_started,
                            AttemptOutcome::failure(Some(status), None),
                        );
                        continue;
                    }
                };
                mark_success(&state, target.config.id);
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
                    state,
                    target,
                    route_id: route.config.id,
                    retry: route.config.retry.clone(),
                    pricing,
                    attempts,
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
    request: &Request<Incoming>,
    target: &ResolvedTarget,
) -> Result<(reqwest::Upgraded, HeaderMap), StatusCode> {
    let path = if target.config.auth_scheme == "azure_api_key" {
        "/responses"
    } else {
        "/v1/responses"
    };
    let url = upstream_url_with_query(&target.config.base_url, path, request.uri().query())
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    let mut headers =
        build_upstream_headers(request.headers(), target, ProxyProtocol::OpenAiResponses)
            .map_err(|_| StatusCode::BAD_GATEWAY)?;
    // Never allow local credentials hidden in browser subprotocols to escape.
    // Native OpenAI clients authenticate using the existing local token headers.
    headers.remove(header::SEC_WEBSOCKET_PROTOCOL);
    headers.remove(header::SEC_WEBSOCKET_EXTENSIONS);
    headers.remove(header::SEC_WEBSOCKET_ACCEPT);
    headers.remove(header::CONTENT_TYPE);
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
    let response = client
        .get(url)
        .headers(headers)
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    if response.status() != StatusCode::SWITCHING_PROTOCOLS {
        return Err(
            if response.status().is_client_error() || response.status().is_server_error() {
                response.status()
            } else {
                StatusCode::BAD_GATEWAY
            },
        );
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
        return Err(StatusCode::BAD_GATEWAY);
    }
    let headers = headers.clone();
    let upgraded = response
        .upgrade()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok((upgraded, headers))
}

async fn relay(
    downstream: TokioIo<hyper::upgrade::Upgraded>,
    upstream: reqwest::Upgraded,
    mut config_changed: tokio::sync::watch::Receiver<()>,
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
    loop {
        let idle_deadline = usage.idle_deadline(write_timeout);
        let (from_client, message) = tokio::select! {
            biased;
            _ = config_changed.changed() => {
                let close = Some(CloseFrame { code: CloseCode::Restart, reason: "proxy configuration changed; reconnect".into() });
                let _ = tokio::time::timeout(close_timeout, downstream.close(close.clone())).await;
                let _ = tokio::time::timeout(close_timeout, upstream.close(close)).await;
                break;
            }
            _ = tokio::time::sleep_until(idle_deadline), if usage.has_requests() => {
                upstream_failed = true;
                let close = Some(CloseFrame { code: CloseCode::Error, reason: "upstream response idle timeout".into() });
                let _ = tokio::time::timeout(close_timeout, downstream.close(close.clone())).await;
                let _ = tokio::time::timeout(close_timeout, upstream.close(close)).await;
                break;
            }
            message = downstream.next() => (true, message),
            message = upstream.next() => (false, message),
        };
        let Some(Ok(message)) = message else {
            upstream_failed = !from_client && usage.has_requests();
            break;
        };
        if let Message::Text(text) = &message {
            if let Ok(value) = serde_json::from_str(text) {
                if from_client {
                    usage.client_event(&value);
                } else {
                    usage.server_event(&value);
                }
            }
        }
        let closing = message.is_close();
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
    usage.disconnected(upstream_failed);
}

struct ResponseUsage {
    request_id: Uuid,
    started: Instant,
    started_at: i64,
    model: Option<String>,
    first_token_ms: Option<u64>,
    stream_id: String,
    last_event: Instant,
}

impl ResponseUsage {
    fn new(value: &serde_json::Value) -> Self {
        Self {
            request_id: Uuid::new_v4(),
            started: Instant::now(),
            last_event: Instant::now(),
            started_at: now_unix(),
            model: value
                .get("model")
                .and_then(|v| v.as_str())
                .map(str::to_owned),
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
    state: RuntimeState,
    target: ResolvedTarget,
    route_id: Uuid,
    retry: RetryPolicy,
    pricing: Vec<ModelPricing>,
    attempts: u8,
    pending: HashMap<String, VecDeque<ResponseUsage>>,
    active: HashMap<String, ResponseUsage>,
}

impl SessionUsage {
    fn has_requests(&self) -> bool {
        !self.pending.is_empty() || !self.active.is_empty()
    }

    fn idle_deadline(&self, timeout: Duration) -> tokio::time::Instant {
        // Queued requests on an active lane wait for that lane's response.
        // Activity on a different lane (or a WS ping) cannot extend its budget.
        let last_event = self
            .active
            .values()
            .map(|request| request.last_event)
            .chain(
                self.pending
                    .iter()
                    .filter(|(lane, _)| {
                        !self
                            .active
                            .values()
                            .any(|request| request.stream_id == **lane)
                    })
                    .filter_map(|(_, queue)| queue.front().map(|request| request.last_event)),
            )
            .min()
            .unwrap_or_else(Instant::now);
        tokio::time::Instant::from_std(last_event + timeout)
    }

    fn advance_lane(&mut self, lane: &str) {
        if let Some(request) = self.pending.get_mut(lane).and_then(VecDeque::front_mut) {
            request.last_event = Instant::now();
        }
    }

    fn client_event(&mut self, value: &serde_json::Value) {
        // Retain metadata only, never prompts, tool results or full WS events.
        // Cap bookkeeping independently of the upstream's multiplexing limits.
        if value["type"] != "response.create"
            || self.active.len() + self.pending.values().map(VecDeque::len).sum::<usize>() >= 1024
        {
            return;
        }
        let request = ResponseUsage::new(value);
        self.state.usage.log_diagnostic(
            "info",
            format!(
                "event=proxy.websocket.request.started request_id={} route_id={}",
                request.request_id, self.route_id
            ),
        );
        self.pending
            .entry(request.stream_id.clone())
            .or_default()
            .push_back(request);
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
            self.advance_lane(&request.stream_id);
            self.finish(
                request,
                usage_from_wire_value(ProxyProtocol::OpenAiResponses, value),
                status,
                Some(status.is_success()),
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
            self.finish(request, TokenUsage::default(), status, Some(false));
        }
    }

    fn finish(
        &self,
        request: ResponseUsage,
        usage: TokenUsage,
        status: StatusCode,
        outcome: Option<bool>,
    ) {
        let success = status.is_success();
        if success {
            mark_success(&self.state, self.target.config.id);
        } else if outcome == Some(false) && status_affects_circuit(status) {
            mark_failure(&self.state, self.target.config.id, &self.retry);
        }
        record_request(&self.state, success, request.first_token_ms);
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
            );
        }
    }
}

#[cfg(test)]
mod tests;
