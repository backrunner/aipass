//! Native WS transport with exclusive leases from a downstream-owned pool.
use super::*;
use serde_json::Value;

pub(crate) struct RequestContext<'a> {
    pub state: &'a RuntimeState,
    pub route: &'a ResolvedRoute,
    pub target: &'a ResolvedTarget,
    pub pricing: &'a [ModelPricing],
    pub session_key: Option<&'a str>,
    pub incoming_headers: &'a HeaderMap,
    pub query: Option<&'a str>,
    pub body: &'a ReplayableRequestBody,
    pub request_id: Uuid,
    pub attempts: &'a mut u8,
    pub hold_deadline: Option<tokio::time::Instant>,
    pub pool: Option<Arc<pool::Pool>>,
    pub config_changed: ConfigWatch,
    pub streaming: bool,
}

pub(crate) enum ForwardOutcome {
    Response(Response<BoxBody>),
    HttpFallback(Option<capability::Evidence>),
    Rejected(StatusCode),
}

/// HTTP fallback is explicit and only possible before submission.
pub(crate) async fn forward(mut ctx: RequestContext<'_>) -> ForwardOutcome {
    let diagnostic = WsDiagnostic {
        store: &ctx.state.usage,
        request_id: ctx.request_id,
        route_id: ctx.route.config.id,
        provider_id: ctx.target.config.provider_entry_id,
    };
    let capability_key = capability::key(ctx.state, ctx.target);
    let observation = capability::observe(ctx.state, capability_key);
    if !capability::allowed(ctx.state, capability_key) {
        return ForwardOutcome::HttpFallback(None);
    }
    if let Some(fallback) = ctx
        .pool
        .as_ref()
        .and_then(|pool| pool.fallback(capability_key))
    {
        return ForwardOutcome::HttpFallback(fallback);
    }
    if ctx.pool.is_none() {
        if let Some(evidence) = capability::http_fallback(
            ctx.state,
            ctx.route.config.id,
            ctx.session_key,
            capability_key,
            None,
        ) {
            return ForwardOutcome::HttpFallback(evidence);
        }
    }
    let Some(mut payload) = ctx.body.json().await else {
        return ForwardOutcome::HttpFallback(None);
    };
    let Some(object) = payload.as_object_mut() else {
        return ForwardOutcome::HttpFallback(None);
    };
    // Background execution is an HTTP API operation, not a WS generation.
    if object.get("background") == Some(&Value::Bool(true)) {
        return ForwardOutcome::HttpFallback(None);
    }
    object.remove("stream");
    object.insert("type".into(), Value::String("response.create".into()));
    let Some(key) = pool::key(
        ctx.route.config.id,
        ctx.target,
        ctx.incoming_headers,
        ctx.query,
    ) else {
        return ForwardOutcome::Rejected(StatusCode::BAD_REQUEST);
    };
    let started = Instant::now();
    let client = match upstream_client_for_transport(
        ctx.state,
        ctx.route.config.retry.connect_timeout_ms,
        true,
    ) {
        Ok(client) => client,
        Err(_) => return ForwardOutcome::Rejected(StatusCode::BAD_GATEWAY),
    };
    if ctx.config_changed.has_changed().unwrap_or(true) {
        return ForwardOutcome::Response(error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "proxy configuration changed; reconnect",
        ));
    }
    let result = tokio::select! {
        biased;
        _ = ctx.config_changed.changed() => return ForwardOutcome::Response(error_response(StatusCode::SERVICE_UNAVAILABLE, "proxy configuration changed; reconnect")),
        result = async {
            if let Some(pool) = &ctx.pool {
                if let Some(socket) = pool.take(key).await {
                    diagnostic.log("reused", None, None);
                    return Ok(socket);
                }
            }
            let (upgraded, _) = capability::connect_twice(ConnectContext {
                client: &client, headers: ctx.incoming_headers, query: ctx.query,
                target: ctx.target, diagnostic: &diagnostic,
                timeout: Duration::from_millis(ctx.route.config.retry.first_byte_timeout_ms.max(1)),
                hold_deadline: ctx.hold_deadline,
                attempts: ctx.attempts, model: payload["model"].as_str(),
            }).await?;
            let config = WebSocketConfig::default()
                .max_message_size(Some(MAX_BUFFERED_RESPONSE_BYTES))
                .max_frame_size(Some(MAX_BUFFERED_RESPONSE_BYTES));
            Ok(pool::Connection {
                id: ctx.request_id,
                socket: WebSocketStream::from_raw_socket(upgraded, Role::Client, Some(config)).await,
            })
        } => result,
    };
    let socket = match result {
        Ok(connected) => connected,
        Err((error, repeated)) => {
            let status = error.status();
            diagnostic.log("bridge_handshake_failed", Some(status), None);
            if !error.permits_fallback() {
                return ForwardOutcome::Rejected(status);
            }
            *ctx.attempts = ctx.attempts.saturating_add(1);
            let evidence = repeated
                .then(|| capability::candidate(ctx.state, ctx.target, observation, started, status))
                .flatten();
            if let Some(pool) = &ctx.pool {
                pool.set_fallback(capability_key, evidence.clone());
            } else {
                capability::http_fallback(
                    ctx.state,
                    ctx.route.config.id,
                    ctx.session_key,
                    capability_key,
                    Some(evidence.clone()),
                );
            }
            return ForwardOutcome::HttpFallback(evidence);
        }
    };
    ctx.state.usage.log_diagnostic("info", format!(
        "event=proxy.request.forwarding request_id={} connection_id={} target_id={} provider_id={} attempt={} transport=websocket inbound=OpenAiResponses upstream=OpenAiResponses converted=false",
        ctx.request_id, socket.id, ctx.target.config.id, ctx.target.config.provider_entry_id, ctx.attempts,
    ));
    let summary = RequestSummary::deserialize(&payload).ok();
    RequestSummary::log(
        summary.as_ref(),
        &ctx.state.usage,
        ctx.request_id,
        "ws_upstream",
    );
    let mut target = ctx.target.clone();
    target.api_key.zeroize();
    for (_, value) in &mut target.config.headers {
        value.zeroize();
    }
    target.config.headers.clear();
    let mut usage = SessionUsage {
        connection_id: socket.id,
        observation,
        state: ctx.state.clone(),
        target,
        route_id: ctx.route.config.id,
        retry: ctx.route.config.retry.clone(),
        pricing: ctx.pricing.to_vec(),
        attempts: *ctx.attempts,
        session_key: ctx.session_key.map(str::to_owned),
        pending: HashMap::new(),
        active: HashMap::new(),
    };
    let mut request = ResponseUsage::new(&payload, usage.session_key.as_deref());
    request.request_id = ctx.request_id;
    usage
        .pending
        .entry(String::new())
        .or_default()
        .push_back(request);
    let mut source = ResponseStream {
        socket: Some(socket),
        pool: ctx.pool,
        key,
        config_changed: ctx.config_changed,
        heartbeat: keepalive::Heartbeat::new(),
        invalidated: false,
        usage,
        payload: Some(payload),
        done: false,
    };
    if !ctx.streaming {
        loop {
            match source.read_event().await {
                Ok(event) if source.done => {
                    let status = if matches!(
                        event["type"].as_str(),
                        Some("response.completed" | "response.incomplete")
                    ) {
                        StatusCode::OK
                    } else {
                        StatusCode::BAD_GATEWAY
                    };
                    let value = event.get("response").unwrap_or(&event);
                    return ForwardOutcome::Response(
                        Response::builder()
                            .status(status)
                            .header(header::CONTENT_TYPE, "application/json")
                            .body(
                                Full::new(Bytes::from(value.to_string()))
                                    .map_err(|never| -> BoxError { match never {} })
                                    .boxed_unsync(),
                            )
                            .expect("static response headers"),
                    );
                }
                Ok(_) => {}
                Err(_) => {
                    return ForwardOutcome::Response(error_response(
                        StatusCode::BAD_GATEWAY,
                        "upstream WebSocket failed before response completion",
                    ))
                }
            }
        }
    }
    let stream = stream::unfold(source, |mut source| async move {
        if source.done {
            return None;
        }
        let event = match source.read_event().await {
            Ok(event) => event,
            Err(()) => {
                let error =
                    std::io::Error::other("upstream WebSocket failed before response completion");
                return Some((Err(Box::new(error) as BoxError), source));
            }
        };
        let bytes = Bytes::from(format!("data: {event}\n\n"));
        Some((Ok(Frame::data(bytes)), source))
    });
    ForwardOutcome::Response(
        Response::builder()
            .extension(UpstreamIdentity {
                target_id: ctx.target.config.id,
            })
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(BodyExt::boxed_unsync(StreamBody::new(stream)))
            .expect("static response headers"),
    )
}

struct ResponseStream {
    socket: Option<pool::Connection>,
    pool: Option<Arc<pool::Pool>>,
    key: pool::Key,
    config_changed: ConfigWatch,
    heartbeat: keepalive::Heartbeat,
    invalidated: bool,
    usage: SessionUsage,
    payload: Option<Value>,
    done: bool,
}

impl ResponseStream {
    async fn read_event(&mut self) -> Result<Value, ()> {
        let mut changed = self.config_changed.clone();
        let result = tokio::select! {
            biased;
            _ = changed.changed() => {
                self.invalidated = true;
                Err(())
            }
            result = self.next_event() => result,
        };
        let event = match result {
            Ok(event) => event,
            _ => {
                self.usage.log_closed(
                    if self.invalidated {
                        "configuration_changed"
                    } else {
                        "upstream_event_failed"
                    },
                    None,
                );
                self.done = true;
                self.usage.disconnected(!self.invalidated);
                self.socket.take();
                return Err(());
            }
        };
        self.usage.server_event(&event);
        self.done = matches!(
            event["type"].as_str(),
            Some(
                "response.completed"
                    | "response.incomplete"
                    | "response.failed"
                    | "response.cancelled"
                    | "error"
            )
        );
        if self.done
            && !self.usage.has_requests()
            && matches!(
                event["type"].as_str(),
                Some("response.completed" | "response.incomplete")
            )
        {
            if let Some(socket) = self.socket.take() {
                if let Some(pool) = &self.pool {
                    pool.put(self.key, socket, self.config_changed.clone());
                }
            }
        }
        Ok(event)
    }

    async fn next_event(&mut self) -> Result<Value, ()> {
        if self.config_changed.has_changed().unwrap_or(true) {
            self.invalidated = true;
            return Err(());
        }
        let socket = &mut self.socket.as_mut().ok_or(())?.socket;
        if let Some(payload) = self.payload.take() {
            // A failed write may have partially submitted the request. It is
            // deliberately surfaced as a stream error, never HTTP fallback.
            socket
                .send(Message::text(payload.to_string()))
                .await
                .map_err(|err| {
                    self.usage
                        .diagnostic()
                        .log("upstream_write_failed", None, Some(&err));
                })?;
        }
        loop {
            let message = tokio::select! {
                biased;
                _ = self.config_changed.changed() => {
                    self.invalidated = true;
                    return Err(());
                }
                _ = tokio::time::sleep_until(self.heartbeat.deadline()) => {
                    self.heartbeat.ping(socket).await?;
                    continue;
                }
                message = socket.next() => message,
            };
            match message {
                Some(Ok(Message::Text(text))) => {
                    let value: Value = serde_json::from_str(&text).map_err(|_| ())?;
                    if !value.is_object()
                        || !value["type"]
                            .as_str()
                            .is_some_and(|kind| !kind.trim().is_empty())
                    {
                        return Err(());
                    }
                    // Providers also send auxiliary events such as
                    // codex.rate_limits. Preserve typed notifications for SSE
                    // clients; only response terminal events finish the request.
                    if matches!(
                        value["type"].as_str(),
                        Some("response.completed" | "response.incomplete")
                    ) && !value["response"].is_object()
                    {
                        return Err(());
                    }
                    return Ok(value);
                }
                Some(Ok(Message::Ping(_))) => socket.flush().await.map_err(|_| ())?,
                Some(Ok(Message::Pong(data))) => self.heartbeat.pong(&data),
                Some(Ok(Message::Close(frame))) => {
                    self.usage.log_closed(
                        "upstream_close",
                        frame.as_ref().map(|frame| u16::from(frame.code)),
                    );
                    return Err(());
                }
                Some(Err(err)) => {
                    self.usage
                        .diagnostic()
                        .log("upstream_read_failed", None, Some(&err));
                    return Err(());
                }
                _ => return Err(()),
            }
        }
    }
}

impl Drop for ResponseStream {
    fn drop(&mut self) {
        // Cancellation/config refresh records an unknown outcome, without
        // treating downstream backpressure as a WS capability failure.
        if self.usage.has_requests() {
            self.usage.log_closed("downstream_dropped", None);
        }
        self.usage.disconnected(false);
    }
}
