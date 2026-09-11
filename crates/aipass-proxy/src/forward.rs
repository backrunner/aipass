use super::*;

pub(crate) enum ReplayableRequestBody {
    Memory(Bytes),
    File { file: std::fs::File, len: u64 },
}

#[derive(Default, Deserialize)]
pub(crate) struct RequestMetadata {
    #[serde(default)]
    pub(crate) stream: bool,
    pub(crate) model: Option<String>,
    /// A stable client supplied key lets providers reuse their prompt cache
    /// for a conversation. `prompt_cache_key` is the OpenAI API spelling;
    /// the other fields cover clients that expose the same value as a
    /// session or conversation identifier.
    #[serde(alias = "promptCacheKey")]
    pub(crate) prompt_cache_key: Option<String>,
    #[serde(alias = "sessionId")]
    pub(crate) session_id: Option<String>,
    #[serde(alias = "sessionKey")]
    pub(crate) session: Option<String>,
    #[serde(alias = "conversationId")]
    pub(crate) conversation_id: Option<String>,
    #[serde(alias = "previousResponseId")]
    pub(crate) previous_response_id: Option<String>,
    /// Responses clients may carry a stable conversation identifier as a
    /// string or as `{ "id": "..." }`.
    pub(crate) conversation: Option<serde_json::Value>,
}

impl ReplayableRequestBody {
    pub(crate) fn bytes(&self) -> Option<&Bytes> {
        match self {
            Self::Memory(bytes) => Some(bytes),
            Self::File { .. } => None,
        }
    }

    pub(crate) async fn json(&self) -> Option<serde_json::Value> {
        match self {
            Self::Memory(bytes) => serde_json::from_slice(bytes).ok(),
            Self::File { file, .. } => {
                let mut file = file.try_clone().ok()?;
                tokio::task::spawn_blocking(move || {
                    use std::io::{BufReader, Seek, SeekFrom};

                    file.seek(SeekFrom::Start(0)).ok()?;
                    serde_json::from_reader(BufReader::new(file)).ok()
                })
                .await
                .ok()
                .flatten()
            }
        }
    }

    pub(crate) fn len(&self) -> u64 {
        match self {
            Self::Memory(bytes) => bytes.len() as u64,
            Self::File { len, .. } => *len,
        }
    }

    pub(crate) async fn metadata(&self) -> Option<RequestMetadata> {
        self.parse_metadata().await
    }

    pub(crate) async fn parse_metadata<T: serde::de::DeserializeOwned + Send + 'static>(
        &self,
    ) -> Option<T> {
        match self {
            Self::Memory(bytes) => serde_json::from_slice(bytes).ok(),
            Self::File { file, .. } => {
                let mut file = file.try_clone().ok()?;
                tokio::task::spawn_blocking(move || {
                    use std::io::{BufReader, Seek, SeekFrom};

                    file.seek(SeekFrom::Start(0)).ok()?;
                    serde_json::from_reader(BufReader::new(file)).ok()
                })
                .await
                .ok()
                .flatten()
            }
        }
    }

    pub(crate) async fn request_body(&self) -> Result<reqwest::Body, std::io::Error> {
        match self {
            Self::Memory(bytes) => Ok(reqwest::Body::from(bytes.clone())),
            Self::File { file, .. } => {
                use std::io::{Seek, SeekFrom};

                let mut file = file.try_clone()?;
                file.seek(SeekFrom::Start(0))?;
                let file = tokio::fs::File::from_std(file);
                Ok(reqwest::Body::from(file))
            }
        }
    }
}

pub(crate) enum RequestBodyReadError {
    TooLarge,
    Io(std::io::Error),
    Transport(String),
}

impl std::fmt::Debug for RequestBodyReadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => formatter.write_str("request body too large"),
            Self::Io(err) => formatter.debug_tuple("Io").field(err).finish(),
            Self::Transport(err) => formatter.debug_tuple("Transport").field(err).finish(),
        }
    }
}

pub(crate) async fn read_replayable_request_body(
    body: Incoming,
) -> Result<ReplayableRequestBody, RequestBodyReadError> {
    read_replayable_request_chunks(
        body.into_data_stream(),
        MAX_REQUEST_BODY_BYTES,
        REQUEST_BODY_MEMORY_THRESHOLD,
    )
    .await
}

pub(crate) async fn read_replayable_request_chunks<S, E>(
    stream: S,
    max_bytes: usize,
    memory_threshold: usize,
) -> Result<ReplayableRequestBody, RequestBodyReadError>
where
    S: Stream<Item = Result<Bytes, E>> + Send,
    E: std::fmt::Display,
{
    let mut stream = Box::pin(stream);
    let mut memory = Vec::new();
    let mut file: Option<tokio::fs::File> = None;
    let mut total = 0_usize;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|err| RequestBodyReadError::Transport(err.to_string()))?;
        total = total
            .checked_add(chunk.len())
            .ok_or(RequestBodyReadError::TooLarge)?;
        if total > max_bytes {
            return Err(RequestBodyReadError::TooLarge);
        }

        if let Some(file) = file.as_mut() {
            file.write_all(&chunk)
                .await
                .map_err(RequestBodyReadError::Io)?;
            continue;
        }
        if total <= memory_threshold {
            memory.extend_from_slice(&chunk);
            continue;
        }

        let file_handle = tempfile::tempfile().map_err(RequestBodyReadError::Io)?;
        let mut async_file = tokio::fs::File::from_std(file_handle);
        async_file
            .write_all(&memory)
            .await
            .map_err(RequestBodyReadError::Io)?;
        async_file
            .write_all(&chunk)
            .await
            .map_err(RequestBodyReadError::Io)?;
        memory.clear();
        file = Some(async_file);
    }

    match file {
        Some(mut file) => {
            file.flush().await.map_err(RequestBodyReadError::Io)?;
            let file = file.into_std().await;
            Ok(ReplayableRequestBody::File {
                file,
                len: total as u64,
            })
        }
        None => Ok(ReplayableRequestBody::Memory(Bytes::from(memory))),
    }
}

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
        response = forward_request_inner(request, state, route, pricing) => response?,
    };
    Ok(attach_in_flight_guard(response, Some(guard)))
}

/// What the forwarding loop should do after one target attempt.
enum AttemptDisposition {
    /// Outbound request is assembled and ready to send.
    Dispatch(reqwest::RequestBuilder),
    /// Completed upstream response; the caller attaches the activity guard.
    Response(Response<BoxBody>),
    /// Client-visible failure returned immediately, without retrying.
    Abort(Response<BoxBody>),
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

/// Build the outbound request for one attempt: pick the client, run request
/// conversion, and assemble URL, headers and body. Any disposition other
/// than `Dispatch` replaces the send.
async fn prepare_upstream_request(ctx: &AttemptContext<'_>) -> AttemptDisposition {
    let state = ctx.state;
    let route = ctx.route;
    let target = ctx.target;
    let client = match upstream_client(state, route.config.retry.connect_timeout_ms) {
        Ok(client) => client,
        Err(err) => return ctx.fail(None, None, false, err),
    };
    let target_protocol = target
        .config
        .effective_protocol(route.config.upstream_protocol);
    let conversion =
        route.config.conversion_enabled && route.config.inbound_protocol != target_protocol;
    state.usage.log_diagnostic("info", format!(
        "event=proxy.request.forwarding request_id={} target_id={} provider_id={} attempt={} transport=http inbound={:?} upstream={target_protocol:?} converted={conversion}",
        ctx.request_id, target.config.id, target.config.provider_entry_id, ctx.attempts, route.config.inbound_protocol,
    ));
    let mut rewritten_payload = None;
    if conversion {
        let Some(json_payload) = ctx.request_json.cloned() else {
            return AttemptDisposition::Abort(error_response(
                StatusCode::BAD_REQUEST,
                "protocol conversion requires a JSON request",
            ));
        };
        rewritten_payload = Some(
            match BuiltinConversionPlugin
                .convert_request(route.config.inbound_protocol, target_protocol, json_payload)
                .and_then(|value| {
                    let summary = diagnostics::protocol::RequestSummary::deserialize(&value).ok();
                    diagnostics::protocol::RequestSummary::log(
                        summary.as_ref(),
                        &state.usage,
                        ctx.request_id,
                        "converted_upstream",
                    );
                    serde_json::to_vec(&value).map_err(|err| {
                        aipass_proxy_conversion::ConversionError::InvalidPayload {
                            protocol: route.config.inbound_protocol,
                            message: err.to_string(),
                        }
                    })
                }) {
                Ok(payload) => Bytes::from(payload),
                Err(err) => {
                    return AttemptDisposition::Abort(error_response(
                        StatusCode::BAD_REQUEST,
                        &err.to_string(),
                    ))
                }
            },
        );
    }
    if let Some(payload) = rewritten_payload
        .take()
        .or_else(|| ctx.body.bytes().cloned())
    {
        let updated = request_stream_usage(target_protocol, ctx.streaming_request, payload.clone());
        if updated != payload {
            rewritten_payload = Some(updated);
        } else if conversion {
            rewritten_payload = Some(payload);
        }
    }
    let upstream_path = if target.config.auth_scheme == "azure_api_key" {
        target_protocol
            .path()
            .strip_prefix("/v1")
            .unwrap_or(target_protocol.path())
    } else {
        target_protocol.path()
    };
    let url =
        match upstream_url_with_query(&target.config.base_url, upstream_path, ctx.request_query) {
            Ok(url) => url,
            Err(err) => return ctx.fail(None, None, false, err.to_string()),
        };
    let upstream_headers =
        match build_upstream_headers(ctx.incoming_headers, target, target_protocol) {
            Ok(headers) => headers,
            Err(err) => return ctx.fail(None, None, false, err),
        };
    let payload_len = rewritten_payload
        .as_ref()
        .map_or_else(|| ctx.body.len(), |payload| payload.len() as u64);
    let payload = match rewritten_payload {
        Some(payload) => reqwest::Body::from(payload),
        None => match ctx.body.request_body().await {
            Ok(payload) => payload,
            Err(err) => return ctx.fail(None, None, false, err.to_string()),
        },
    };
    let mut upstream_headers = upstream_headers;
    match HeaderValue::from_str(&payload_len.to_string()) {
        Ok(value) => {
            upstream_headers.insert(header::CONTENT_LENGTH, value);
        }
        Err(err) => return ctx.fail(None, None, false, err.to_string()),
    }
    AttemptDisposition::Dispatch(
        client
            .request(ctx.method.clone(), url)
            .headers(upstream_headers)
            .body(payload),
    )
}

/// Send the prepared request and turn the upstream response into the client
/// response: classify the status, stream or buffer the body, convert and
/// track usage. Retry dispositions carry whether generation may already have
/// been submitted upstream.
async fn run_http_attempt(ctx: &AttemptContext<'_>) -> AttemptDisposition {
    let upstream = match prepare_upstream_request(ctx).await {
        AttemptDisposition::Dispatch(upstream) => upstream,
        disposition => return disposition,
    };
    let response = match upstream.send().await {
        Ok(response) => response,
        // A connect failure proves no generation was submitted. A lost
        // response or partial write does not: never replay it.
        Err(err) => return ctx.fail(None, None, !err.is_connect(), err.to_string()),
    };
    deliver_upstream_response(ctx, response).await
}

/// Process the upstream response body of an attempt and produce the final
/// client-facing response, or a retry disposition.
async fn deliver_upstream_response(
    ctx: &AttemptContext<'_>,
    response: reqwest::Response,
) -> AttemptDisposition {
    let state = ctx.state;
    let route = ctx.route;
    let target = ctx.target;
    let status = response.status();
    if is_retryable_status(status) {
        // Explicit rejection before generation; the request never ran.
        let detail = diagnostics::upstream::read_error(
            response,
            target,
            &local_token_redactions(ctx.incoming_headers),
        )
        .await;
        state.usage.log_upstream_error(
            ctx.request_id,
            route.config.id,
            &target.config,
            status,
            "http",
            &detail,
        );
        if status_affects_circuit(status) {
            mark_failure(state, target.config.id, &route.config.retry);
        } else {
            clear_rejected_session(state, route.config.id, ctx.session_key, target.config.id);
        }
        persist_attempt(
            &state.usage,
            (ctx.request_id, route.config.id),
            target,
            ctx.model.as_deref(),
            ctx.attempt_started_at,
            ctx.attempt_started,
            AttemptOutcome::failure(Some(status), None),
        );
        return AttemptDisposition::Retry {
            error: format!("upstream returned {status}: {detail}"),
            generation_submitted: false,
        };
    }
    let response_headers = response.headers().clone();
    let content_type = response_headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let target_protocol = target
        .config
        .effective_protocol(route.config.upstream_protocol);
    let conversion =
        route.config.conversion_enabled && route.config.inbound_protocol != target_protocol;
    let streaming_response = ctx.streaming_request && is_event_stream(&content_type);
    // A silent retry must not commit a response before the upstream stream
    // has completed. Only an explicit upstream error can permit a retry; an
    // incomplete stream must never replay generation.
    let buffer_streaming = streaming_response && route.config.retry.silent_retry && !ctx.websocket;
    let mut upstream_stream: UpstreamBodyStream = Box::pin(response.bytes_stream());
    if is_event_stream(&content_type) {
        let diagnostic_state = state.clone();
        let request_id = ctx.request_id;
        let route_id = route.config.id;
        let target_id = target.config.id;
        let mut errors = diagnostics::upstream::ErrorEvents::default();
        upstream_stream = Box::pin(upstream_stream.map(move |chunk| {
            if let Ok(bytes) = &chunk {
                if let Some(error) = errors.observe(bytes) {
                    diagnostics::upstream::log_wire_error(
                        &diagnostic_state,
                        request_id,
                        route_id,
                        target_id,
                        status,
                        "sse",
                        &error,
                    );
                }
            }
            chunk
        }));
    }
    let first_chunk = match upstream_stream.next().await {
        Some(Ok(chunk)) => Some(chunk),
        // The response was already received, so generation was submitted.
        Some(Err(err)) => return ctx.fail(Some(status), None, true, err.to_string()),
        None => None,
    };
    let (first_chunk, first_token_observed) = if streaming_response
        && !buffer_streaming
        && !ctx.websocket
    {
        // Once this event is returned to the client, replaying on another
        // target is unsafe.
        match prefetch_sse_event(target_protocol, first_chunk, &mut upstream_stream).await {
            Ok(Some(prefetched)) => (Some(prefetched.bytes), prefetched.first_token_observed),
            Ok(None) => {
                return ctx.fail(
                    Some(status),
                    None,
                    true,
                    "upstream stream ended before the first event",
                )
            }
            Err(err) => return ctx.fail(Some(status), None, !err.confirmed_failure, err.message),
        }
    } else {
        (first_chunk, false)
    };
    let first_token_ms =
        first_token_observed.then(|| ctx.attempt_started.elapsed().as_millis() as u64);
    let upstream_protocol = target_protocol;
    let inbound_protocol = route.config.inbound_protocol;
    let model = ctx.model.clone();
    let model_pricing = model.as_deref().and_then(|model| {
        ctx.pricing
            .iter()
            .filter(|item| item.model == model || model.starts_with(&item.model))
            .max_by_key(|item| item.model.len())
            .cloned()
    });
    let record = UsageRecord {
        id: ctx.request_id,
        started_at: ctx.started_at,
        duration_ms: ctx.started.elapsed().as_millis() as u64,
        first_token_ms,
        route_id: route.config.id,
        provider_entry_id: target.config.provider_entry_id,
        secret_id: target.config.secret_id.clone(),
        model: model.clone(),
        inbound_protocol,
        upstream_protocol,
        status: status.as_u16(),
        attempts: ctx.attempts,
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        estimated_cost_micros: 0,
    };
    let mut converted_payload = None;
    let mut response_id = None;
    let body_stream: UpstreamBodyStream = if streaming_response && !buffer_streaming {
        if let Some(first_chunk) = first_chunk {
            Box::pin(stream::once(async move { Ok(first_chunk) }).chain(upstream_stream))
        } else {
            Box::pin(stream::empty())
        }
    } else {
        let buffered = match collect_upstream_body(first_chunk, &mut upstream_stream).await {
            Ok(buffered) => buffered,
            Err(err) => return ctx.fail(Some(status), first_token_ms, true, err),
        };
        if buffer_streaming
            && (stream_reports_error(&buffered)
                || !stream_reports_completion(target_protocol, &buffered))
        {
            return ctx.fail(
                Some(status),
                first_token_ms,
                !stream_reports_error(&buffered),
                "upstream stream ended before protocol completion",
            );
        }
        if status.is_success() && is_upstream_error_payload(&buffered) {
            let detail = diagnostics::upstream::error_detail(
                &buffered,
                target,
                &local_token_redactions(ctx.incoming_headers),
            );
            state.usage.log_upstream_error(
                ctx.request_id,
                route.config.id,
                &target.config,
                status,
                "http",
                &detail,
            );
            return ctx.fail(
                Some(status),
                first_token_ms,
                false,
                "upstream returned an error payload",
            );
        }
        if status.is_success() && buffered.is_empty() {
            return ctx.fail(
                Some(status),
                first_token_ms,
                false,
                "upstream returned an empty response",
            );
        }
        if !streaming_response && inbound_protocol == ProxyProtocol::OpenAiResponses {
            #[derive(Deserialize)]
            struct ResponseId {
                id: Option<String>,
            }
            response_id = serde_json::from_slice::<ResponseId>(&buffered)
                .ok()
                .and_then(|response| response.id)
                .and_then(|id| normalize_session_affinity_key(&id));
        }
        if conversion && !streaming_response {
            converted_payload = serde_json::from_slice::<serde_json::Value>(&buffered)
                .ok()
                .and_then(|value| {
                    BuiltinConversionPlugin
                        .convert_response(upstream_protocol, inbound_protocol, value)
                        .ok()
                })
                .and_then(|value| serde_json::to_vec(&value).ok())
                .map(Bytes::from);
            if converted_payload.is_none() {
                return ctx.fail(
                    Some(status),
                    first_token_ms,
                    false,
                    "protocol conversion failed for upstream response",
                );
            }
        }
        Box::pin(stream::once(async move {
            Ok::<Bytes, reqwest::Error>(buffered)
        }))
    };
    let streaming_attempt = streaming_response.then(|| {
        (
            AttemptRecord {
                id: Uuid::new_v4(),
                request_id: Some(record.id),
                started_at: ctx.attempt_started_at,
                duration_ms: 0,
                first_token_ms,
                route_id: route.config.id,
                target_id: target.config.id,
                provider_entry_id: target.config.provider_entry_id,
                secret_id: target.config.secret_id.clone(),
                model: model.clone(),
                status: Some(status.as_u16()),
                success: None,
            },
            ctx.attempt_started,
        )
    });
    let body_stream = track_usage_stream(
        body_stream,
        UsageTrackingContext {
            protocol: upstream_protocol,
            ws_evidence: ctx.ws_evidence.clone(),
            store: state.usage.clone(),
            record,
            pricing: model_pricing,
            streaming: streaming_response,
            attempt_started: ctx.attempt_started,
            started: ctx.started,
            failure_state: state.clone(),
            config_changed: ctx.config_changed.clone(),
            route_id: route.config.id,
            target_id: target.config.id,
            session_key: ctx.session_key.map(str::to_string),
            retry_policy: route.config.retry.clone(),
            attempt: streaming_attempt,
        },
    );
    let output_stream: Pin<Box<dyn Stream<Item = Result<Bytes, BoxError>> + Send>> =
        if conversion && streaming_response {
            convert_sse_stream(body_stream, upstream_protocol, inbound_protocol)
        } else if let Some(converted) = converted_payload {
            // Track usage from the original provider payload only after
            // conversion has succeeded, then emit the converted body.
            Box::pin(body_stream.map(move |result| result.map(|_| converted.clone())))
        } else {
            Box::pin(body_stream)
        };
    if !streaming_response {
        persist_attempt(
            &state.usage,
            (ctx.request_id, route.config.id),
            target,
            model.as_deref(),
            ctx.attempt_started_at,
            ctx.attempt_started,
            AttemptOutcome::success(status, first_token_ms),
        );
        complete_target_success(
            state,
            route.config.id,
            ctx.session_key,
            target.config.id,
            ctx.attempt_started,
            response_id.as_deref(),
        );
    }
    let frame_stream = output_stream.map(|result| result.map(Frame::data));
    let stream_body = BodyExt::boxed_unsync(StreamBody::new(frame_stream));
    let mut builder = Response::builder().status(status);
    let response_hop_headers = connection_header_names(&response_headers);
    for (name, value) in response_headers.iter() {
        if !(is_hop_header(name)
            || response_hop_headers.contains(name)
            || name == header::CONTENT_LENGTH
            || conversion && name == header::CONTENT_ENCODING)
        {
            builder = builder.header(name, value);
        }
    }
    let mut response = builder.body(stream_body).unwrap_or_else(|_| {
        error_response(StatusCode::BAD_GATEWAY, "failed to build proxy response")
    });
    response.extensions_mut().insert(UpstreamIdentity {
        target_id: target.config.id,
    });
    AttemptDisposition::Response(response)
}

pub(crate) async fn forward_request_inner(
    request: ForwardRequest,
    state: RuntimeState,
    route: ResolvedRoute,
    pricing: Vec<ModelPricing>,
) -> Result<Response<BoxBody>, Infallible> {
    if let Some(operation) = request.image_operation {
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
        body,
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
    let request_json = if route.config.conversion_enabled
        && route.targets.iter().any(|target| {
            target.config.enabled
                && route.config.inbound_protocol
                    != target
                        .config
                        .effective_protocol(route.config.upstream_protocol)
        }) {
        body.json().await
    } else {
        None
    };
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
    let mut target_attempts = 0u8;
    let mut generation_submitted = false;
    let mut capacity_skipped = false;
    let mut hold_round = 0u32;
    let hold_deadline = hold_deadline(&route.config.retry, started);
    'hold: loop {
        if hold_deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
            break;
        }
        for _round in 0..silent_retry_rounds(&route.config.retry) {
            let targets = ordered_route_targets(&state, &route, session_key.as_deref());
            let mut round_attempts = 0u8;
            for target in targets {
                if round_attempts >= route.config.retry.max_attempts.max(1) {
                    break;
                }
                if generation_submitted {
                    break 'hold;
                }
                if hold_deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
                    break 'hold;
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
                    model: &model,
                };
                if method == http::Method::POST
                    && route.config.inbound_protocol == ProxyProtocol::OpenAiResponses
                    && target.supports_websockets
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
    let final_status = if at_capacity {
        StatusCode::TOO_MANY_REQUESTS
    } else {
        ws_rejection.unwrap_or(StatusCode::BAD_GATEWAY)
    };
    let message = if at_capacity {
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
