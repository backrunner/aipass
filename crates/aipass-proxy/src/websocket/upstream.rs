//! Native WS transport with exclusive leases from a downstream-owned pool.
use super::*;
use serde_json::Value;

pub(crate) struct RequestContext<'a> {
    pub state: &'a RuntimeState,
    pub route: &'a ResolvedRoute,
    pub target: &'a ResolvedTarget,
    pub pricing: &'a [ModelPricing],
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

/// None is safe HTTP fallback: only a handshake has been attempted. Once the
/// returned body starts sending response.create, no caller may replay it.
pub(crate) async fn forward(mut ctx: RequestContext<'_>) -> Option<Response<BoxBody>> {
    let diagnostic = WsDiagnostic {
        store: &ctx.state.usage,
        request_id: ctx.request_id,
        route_id: ctx.route.config.id,
        provider_id: ctx.target.config.provider_entry_id,
    };
    let permit = acquire_ws_transport(ctx.state, ctx.target.config.provider_entry_id)?;
    let mut payload = ctx.body.json().await?;
    let object = payload.as_object_mut()?;
    // Background execution is an HTTP API operation, not a WS generation.
    if object.get("background") == Some(&Value::Bool(true)) {
        return None;
    }
    object.remove("stream");
    object.insert("type".into(), Value::String("response.create".into()));
    let key = pool::key(
        ctx.route.config.id,
        ctx.target,
        ctx.incoming_headers,
        ctx.query,
    )?;
    let started = Instant::now();
    let started_at = now_unix();
    let client =
        upstream_client_for_transport(ctx.state, ctx.route.config.retry.connect_timeout_ms, true)
            .ok()?;
    let deadline = bounded_deadline(
        Duration::from_millis(ctx.route.config.retry.first_byte_timeout_ms.max(1)),
        ctx.hold_deadline,
    );
    if ctx.config_changed.has_changed().unwrap_or(true) {
        return Some(error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "proxy configuration changed; reconnect",
        ));
    }
    let result = tokio::select! {
        biased;
        _ = ctx.config_changed.changed() => return Some(error_response(StatusCode::SERVICE_UNAVAILABLE, "proxy configuration changed; reconnect")),
        result = tokio::time::timeout_at(deadline, async {
            if let Some(pool) = &ctx.pool {
                if let Some(socket) = pool.take(key).await {
                    diagnostic.log("reused", None, None);
                    return Ok(socket);
                }
            }
            let (upgraded, _) = connect_upstream(
                &client, ctx.incoming_headers, ctx.query, ctx.target, Some(&diagnostic),
            ).await?;
            let config = WebSocketConfig::default()
                .max_message_size(Some(MAX_BUFFERED_RESPONSE_BYTES))
                .max_frame_size(Some(MAX_BUFFERED_RESPONSE_BYTES));
            Ok(pool::Connection {
                id: ctx.request_id,
                socket: WebSocketStream::from_raw_socket(upgraded, Role::Client, Some(config)).await,
            })
        }) => result,
    }.unwrap_or_else(|_| {
        diagnostic.log("handshake_timeout", Some(StatusCode::GATEWAY_TIMEOUT), None);
        Err(StatusCode::GATEWAY_TIMEOUT)
    });
    let socket = match result {
        Ok(connected) => connected,
        Err(status) => {
            diagnostic.log("bridge_handshake_failed", Some(status), None);
            if ws_handshake_affects_transport(status) {
                mark_ws_failure(ctx.state, ctx.target.config.provider_entry_id);
            }
            persist_attempt(
                &ctx.state.usage,
                (ctx.request_id, ctx.route.config.id),
                ctx.target,
                payload["model"].as_str(),
                started_at,
                started,
                AttemptOutcome::failure(Some(status), None),
            );
            *ctx.attempts = ctx.attempts.saturating_add(1);
            return None;
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
        state: ctx.state.clone(),
        target,
        route_id: ctx.route.config.id,
        retry: ctx.route.config.retry.clone(),
        pricing: ctx.pricing.to_vec(),
        attempts: *ctx.attempts,
        session_key: session_affinity_key(ctx.incoming_headers, None),
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
        permit,
        socket: Some(socket),
        pool: ctx.pool,
        key,
        config_changed: ctx.config_changed,
        heartbeat: keepalive::Heartbeat::new(),
        invalidated: false,
        hold_deadline: if ctx.streaming {
            None
        } else {
            ctx.hold_deadline
        },
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
                    return Some(
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
                    return Some(error_response(
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
    Some(
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
    permit: WsTransportPermit,
    socket: Option<pool::Connection>,
    pool: Option<Arc<pool::Pool>>,
    key: pool::Key,
    config_changed: ConfigWatch,
    heartbeat: keepalive::Heartbeat,
    invalidated: bool,
    hold_deadline: Option<tokio::time::Instant>,
    usage: SessionUsage,
    payload: Option<Value>,
    done: bool,
}

impl ResponseStream {
    async fn read_event(&mut self) -> Result<Value, ()> {
        let timeout = Duration::from_millis(
            if self.payload.is_some() {
                self.usage.retry.first_byte_timeout_ms
            } else {
                self.usage.retry.stream_idle_timeout_ms
            }
            .max(1),
        );
        let mut changed = self.config_changed.clone();
        let deadline = bounded_deadline(timeout, self.hold_deadline);
        let result = tokio::select! {
            biased;
            _ = changed.changed() => {
                self.invalidated = true;
                Ok(Err(()))
            }
            result = tokio::time::timeout_at(deadline, self.next_event()) => result,
        };
        let event = match result {
            Ok(Ok(event)) => event,
            _ => {
                self.usage.log_closed(
                    if self.invalidated {
                        "configuration_changed"
                    } else if result.is_err() {
                        "upstream_idle_timeout"
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
                            .is_some_and(|kind| kind == "error" || kind.starts_with("response."))
                    {
                        return Err(());
                    }
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
        let _keep_recovery_permit = &self.permit;
    }
}
