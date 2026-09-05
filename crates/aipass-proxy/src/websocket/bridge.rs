//! Adapt Responses WS sessions to the shared HTTP/SSE forwarding pipeline.
use super::*;
use futures_util::{future::BoxFuture, stream::FuturesUnordered, FutureExt};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use zeroize::Zeroizing;

const MAX_LANES: usize = 32;
const MAX_ACTIVE: usize = 16;
const MAX_QUEUED: usize = 128;
const MAX_SESSION_BYTES: usize = 64 * 1024 * 1024;

pub(super) fn upgrade(
    mut request: Request<Incoming>,
    state: RuntimeState,
    mut route: ResolvedRoute,
    pricing: Vec<ModelPricing>,
    config_changed: tokio::sync::watch::Receiver<()>,
    response: Response<BoxBody>,
) -> Response<BoxBody> {
    route.targets.retain(|target| {
        supports(
            ProxyProtocol::OpenAiResponses,
            target
                .config
                .effective_protocol(route.config.upstream_protocol),
        )
    });
    if !route.targets.iter().any(|target| target.config.enabled) {
        return error_response(StatusCode::BAD_REQUEST, "no enabled conversion target");
    }
    let mut headers = request.headers().clone();
    let hop_headers = connection_header_names(&headers);
    // Consume the local handshake and authentication on this hop. Each HTTP
    // request gets its target credential through build_upstream_headers.
    let names: Vec<_> = headers
        .keys()
        .filter(|name| {
            is_hop_header(name)
                || hop_headers.contains(*name)
                || name.as_str().starts_with("sec-websocket-")
                || *name == header::AUTHORIZATION
                || matches!(name.as_str(), "x-api-key" | "api-key")
        })
        .cloned()
        .collect();
    for name in names {
        headers.remove(name);
    }
    let context = Arc::new(Context {
        state,
        route,
        pricing,
        headers,
        query: request.uri().query().map(str::to_owned),
    });
    let upgrade = hyper::upgrade::on(&mut request);
    tokio::spawn(async move {
        if let Ok(downstream) = upgrade.await {
            serve(TokioIo::new(downstream), context, config_changed).await;
        }
    });
    response
}

struct Context {
    state: RuntimeState,
    route: ResolvedRoute,
    pricing: Vec<ModelPricing>,
    headers: HeaderMap,
    query: Option<String>,
}

struct QueuedRequest {
    lane: String,
    value: Value,
    bytes: usize,
}

// Only the latest response in each lane is retained, as in native WS mode.
// Serialized conversation data stays in memory and is zeroized on eviction.
struct CachedResponse {
    id: String,
    history: Zeroizing<Vec<u8>>,
}

#[derive(Default)]
struct Session {
    lanes: HashSet<String>,
    queue: VecDeque<QueuedRequest>,
    queued_bytes: usize,
    active: HashSet<String>,
    cache: HashMap<String, CachedResponse>,
}

struct BridgeError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
}

impl BridgeError {
    fn invalid(code: &'static str, message: &'static str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code,
            message,
        }
    }

    fn upstream() -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            code: "upstream_error",
            message: "upstream stream failed before response completion",
        }
    }

    fn event(&self, lane: &str) -> Value {
        with_lane(
            json!({"type":"error","status":self.status.as_u16(),"error":{
                "type":if self.status.is_server_error() { "server_error" } else { "invalid_request_error" },
                "code":self.code,"message":self.message
            }}),
            lane,
        )
    }
}

fn with_lane(mut value: Value, lane: &str) -> Value {
    if !lane.is_empty() {
        value["stream_id"] = json!(lane);
    }
    value
}

impl Session {
    fn enqueue(&mut self, value: Value, bytes: usize) -> Result<(), BridgeError> {
        if value["type"] != "response.create" {
            return Err(BridgeError::invalid(
                "unsupported_event",
                "conversion supports response.create events only",
            ));
        }
        let lane = match value.get("stream_id") {
            None => String::new(),
            Some(Value::String(lane))
                if !lane.is_empty()
                    && lane.len() <= 256
                    && lane
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c)) =>
            {
                lane.clone()
            }
            _ => {
                return Err(BridgeError::invalid(
                    "invalid_stream_id",
                    "invalid stream_id",
                ))
            }
        };
        if !lane.is_empty() && !self.lanes.contains(&lane) && self.lanes.len() >= MAX_LANES {
            return Err(BridgeError::invalid(
                "websocket_stream_limit_reached",
                "at most 32 named streams are allowed per connection",
            ));
        }
        if self.queue.len() >= MAX_QUEUED
            || self.queued_bytes.saturating_add(bytes) > MAX_SESSION_BYTES
        {
            return Err(BridgeError::invalid(
                "websocket_queue_full",
                "WebSocket request queue is full",
            ));
        }
        if !lane.is_empty() {
            self.lanes.insert(lane.clone());
        }
        self.queued_bytes += bytes;
        self.queue.push_back(QueuedRequest { lane, value, bytes });
        Ok(())
    }

    fn next(&mut self) -> Option<QueuedRequest> {
        if self.active.len() >= MAX_ACTIVE {
            return None;
        }
        let index = self
            .queue
            .iter()
            .position(|request| !self.active.contains(&request.lane))?;
        let request = self.queue.remove(index)?;
        self.queued_bytes -= request.bytes;
        Some(request)
    }

    fn prepare(&self, mut value: Value) -> Result<(Value, Vec<Value>, bool), BridgeError> {
        let object = value
            .as_object_mut()
            .ok_or_else(|| BridgeError::invalid("invalid_request", "request must be an object"))?;
        // These semantics require upstream-managed state or active-run steering,
        // which cannot be emulated by independent HTTP generation requests.
        for key in ["background", "conversation", "context_management"] {
            if object
                .get(key)
                .is_some_and(|v| !v.is_null() && *v != Value::Bool(false))
            {
                return Err(BridgeError::invalid(
                    "unsupported_parameter",
                    "background, conversation and context_management require native Responses mode",
                ));
            }
        }
        let generate = match object.remove("generate") {
            None | Some(Value::Bool(true)) => true,
            Some(Value::Bool(false)) => false,
            _ => {
                return Err(BridgeError::invalid(
                    "invalid_request",
                    "generate must be a boolean",
                ))
            }
        };
        let mut history: Vec<Value> = match object.remove("previous_response_id") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::String(id)) => {
                let cached = self
                    .cache
                    .values()
                    .find(|cached| cached.id == id)
                    .ok_or_else(|| {
                        BridgeError::invalid(
                            "previous_response_not_found",
                            "previous response is not cached on this connection; resend full input",
                        )
                    })?;
                serde_json::from_slice(&cached.history).map_err(|_| BridgeError::upstream())?
            }
            _ => {
                return Err(BridgeError::invalid(
                    "invalid_request",
                    "previous_response_id must be a string or null",
                ))
            }
        };
        let input = match object.remove("input") {
            None => Vec::new(),
            Some(Value::String(text)) => {
                vec![json!({"type":"message","role":"user","content":text})]
            }
            Some(Value::Array(items)) => items,
            _ => {
                return Err(BridgeError::invalid(
                    "invalid_request",
                    "input must be a string or array",
                ))
            }
        };
        history.extend(input);
        object.remove("type");
        object.remove("stream_id");
        object.remove("background");
        object.insert("stream".into(), json!(true));
        object.insert("input".into(), json!(history));
        // All continuations are reconstructed locally, including store=false.
        // HTTP targets never need a response id issued by another target.
        Ok((value, history, generate))
    }

    fn remember(&mut self, lane: &str, cached: CachedResponse) -> bool {
        self.cache.remove(lane);
        let bytes = self
            .cache
            .values()
            .map(|cached| cached.history.len())
            .sum::<usize>();
        if bytes.saturating_add(cached.history.len()) > MAX_SESSION_BYTES {
            return false;
        }
        self.cache.insert(lane.to_owned(), cached);
        true
    }

    fn failed(&mut self, lane: &str, previous_id: Option<&str>) {
        // Errors evict a same-lane parent, but a failed fork must not evict
        // the parent owned by a different lane.
        if self
            .cache
            .get(lane)
            .is_some_and(|cached| Some(cached.id.as_str()) == previous_id)
        {
            self.cache.remove(lane);
        }
    }
}

enum Output {
    Event {
        lane: String,
        value: Value,
    },
    Finished {
        lane: String,
        previous_id: Option<String>,
        result: Result<(Value, Option<CachedResponse>), BridgeError>,
    },
}

async fn serve(
    downstream: TokioIo<hyper::upgrade::Upgraded>,
    context: Arc<Context>,
    mut config_changed: tokio::sync::watch::Receiver<()>,
) {
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_SESSION_BYTES))
        .max_frame_size(Some(MAX_SESSION_BYTES));
    let mut socket = WebSocketStream::from_raw_socket(downstream, Role::Server, Some(config)).await;
    tokio::select! {
        biased;
        _ = config_changed.changed() => {
            let close = Some(CloseFrame { code: CloseCode::Restart, reason: "proxy configuration changed; reconnect".into() });
            let _ = tokio::time::timeout(Duration::from_secs(1), socket.close(close)).await;
        }
        _ = serve_session(&mut socket, context) => {}
    }
}

async fn serve_session(
    socket: &mut WebSocketStream<TokioIo<hyper::upgrade::Upgraded>>,
    context: Arc<Context>,
) {
    let mut session = Session::default();
    let (tx, mut rx) = mpsc::channel::<Output>(16);
    // Futures are owned by this connection. Dropping it cancels HTTP requests,
    // retry sleeps and body readers, including during configuration refresh.
    let mut tasks = FuturesUnordered::<BoxFuture<'static, ()>>::new();
    let write_timeout =
        Duration::from_millis(context.route.config.retry.stream_idle_timeout_ms.max(1));
    loop {
        while let Some(request) = session.next() {
            let previous_id = request.value["previous_response_id"]
                .as_str()
                .map(str::to_owned);
            match session.prepare(request.value) {
                Ok((body, history, generate)) => {
                    session.active.insert(request.lane.clone());
                    let context = context.clone();
                    let tx = tx.clone();
                    tasks.push(
                        async move {
                            let lane = request.lane;
                            let result =
                                run_response(context, &lane, body, history, generate, &tx).await;
                            let _ = tx
                                .send(Output::Finished {
                                    lane,
                                    previous_id,
                                    result,
                                })
                                .await;
                        }
                        .boxed(),
                    );
                }
                Err(error) => {
                    session.failed(&request.lane, previous_id.as_deref());
                    if !send(socket, error.event(&request.lane), write_timeout).await {
                        return;
                    }
                }
            }
        }
        tokio::select! {
            _ = tasks.next(), if !tasks.is_empty() => {}
            Some(output) = rx.recv() => {
                let value = match output {
                    Output::Event { lane, value } => with_lane(value, &lane),
                    Output::Finished { lane, previous_id, result } => {
                        session.active.remove(&lane);
                        match result {
                            Ok((value, cached)) => {
                                if matches!(value["type"].as_str(), Some("error" | "response.failed")) {
                                    session.failed(&lane, previous_id.as_deref());
                                } else {
                                    session.cache.remove(&lane);
                                    if let Some(cached) = cached { session.remember(&lane, cached); }
                                }
                                with_lane(value, &lane)
                            }
                            Err(error) => {
                                session.failed(&lane, previous_id.as_deref());
                                error.event(&lane)
                            },
                        }
                    }
                };
                if !send(socket, value, write_timeout).await { break; }
            }
            message = socket.next() => {
                let Some(Ok(message)) = message else { break; };
                let error = match message {
                    Message::Text(text) => match serde_json::from_str::<Value>(&text) {
                        Ok(value) => {
                            let lane = value["stream_id"].as_str().unwrap_or_default().to_owned();
                            session.enqueue(value, text.len()).err().map(|error| error.event(&lane))
                        }
                        Err(_) => Some(BridgeError::invalid("invalid_json", "WebSocket event must be valid JSON").event("")),
                    },
                    Message::Ping(_) => {
                        if !matches!(tokio::time::timeout(write_timeout, socket.flush()).await, Ok(Ok(()))) { break; }
                        None
                    }
                    Message::Close(_) => {
                        let _ = tokio::time::timeout(Duration::from_secs(1), socket.flush()).await;
                        break;
                    }
                    Message::Binary(_) => Some(BridgeError::invalid("invalid_event", "converted Responses events must be JSON text").event("")),
                    _ => None,
                };
                if let Some(error) = error {
                    if !send(socket, error, write_timeout).await { break; }
                }
            }
        }
    }
}

async fn send(
    socket: &mut WebSocketStream<TokioIo<hyper::upgrade::Upgraded>>,
    value: Value,
    timeout: Duration,
) -> bool {
    matches!(
        tokio::time::timeout(timeout, socket.send(Message::text(value.to_string()))).await,
        Ok(Ok(()))
    )
}

async fn run_response(
    context: Arc<Context>,
    lane: &str,
    body: Value,
    mut history: Vec<Value>,
    generate: bool,
    tx: &mpsc::Sender<Output>,
) -> Result<(Value, Option<CachedResponse>), BridgeError> {
    let id = format!("resp_aipass_{}", Uuid::new_v4().simple());
    if !generate {
        let response = json!({"id":id,"object":"response","created_at":now_unix(),"status":"completed","model":body["model"],"output":[],"usage":{"input_tokens":0,"output_tokens":0,"total_tokens":0}});
        let mut created = response.clone();
        created["status"] = json!("in_progress");
        for (sequence, kind) in ["response.created", "response.in_progress"]
            .into_iter()
            .enumerate()
        {
            emit(
                tx,
                lane,
                json!({"type":kind,"sequence_number":sequence,"response":created}),
            )
            .await?;
        }
        return Ok((
            json!({"type":"response.completed","sequence_number":2,"response":response}),
            cache(&id, &history)?,
        ));
    }
    let payload = serde_json::to_vec(&body).map_err(|_| BridgeError::upstream())?;
    if payload.len() > MAX_SESSION_BYTES {
        return Err(BridgeError::invalid(
            "context_too_large",
            "converted request exceeds the session context limit",
        ));
    }
    let response = forward_request(
        ForwardRequest {
            request_id: Uuid::new_v4(),
            method: http::Method::POST,
            request_query: context.query.clone(),
            incoming_headers: context.headers.clone(),
            body: ReplayableRequestBody::Memory(Bytes::from(payload)),
            started: Instant::now(),
            started_at: now_unix(),
        },
        context.state.clone(),
        context.route.clone(),
        context.pricing.clone(),
    )
    .await
    .unwrap_or_else(|never| match never {});
    if !response.status().is_success() {
        return Err(BridgeError {
            status: response.status(),
            code: "upstream_error",
            message: "converted upstream request failed",
        });
    }
    let mut stream = response.into_body().into_data_stream();
    let mut buffer = Vec::new();
    let idle_timeout =
        Duration::from_millis(context.route.config.retry.stream_idle_timeout_ms.max(1));
    loop {
        let chunk = match tokio::time::timeout(idle_timeout, stream.next()).await {
            Ok(Some(Ok(chunk))) => chunk,
            _ => return Err(BridgeError::upstream()),
        };
        if buffer.len().saturating_add(chunk.len()) > MAX_SESSION_BYTES {
            return Err(BridgeError::upstream());
        }
        buffer.extend_from_slice(&chunk);
        while let Some(end) = sse_event_boundary_end(&buffer) {
            let data = sse_event_data(&buffer[..end]);
            buffer.drain(..end);
            let Some(data) = data else {
                continue;
            };
            if trim_ascii(&data).is_empty() {
                continue;
            }
            let mut value: Value =
                serde_json::from_slice(&data).map_err(|_| BridgeError::upstream())?;
            if !value.is_object() {
                return Err(BridgeError::upstream());
            }
            if value.get("response").is_some_and(Value::is_object) {
                value["response"]["id"] = json!(id);
            }
            if value.get("response_id").is_some() {
                value["response_id"] = json!(id);
            }
            match value["type"].as_str() {
                Some("response.completed" | "response.incomplete") => {
                    let output = value
                        .pointer("/response/output")
                        .and_then(Value::as_array)
                        .ok_or_else(BridgeError::upstream)?;
                    history.extend(output.iter().cloned());
                    return Ok((value, cache(&id, &history)?));
                }
                Some("response.failed" | "error") => return Ok((value, None)),
                _ => emit(tx, lane, value).await?,
            }
        }
    }
}

fn cache(id: &str, history: &[Value]) -> Result<Option<CachedResponse>, BridgeError> {
    let bytes = Zeroizing::new(serde_json::to_vec(history).map_err(|_| BridgeError::upstream())?);
    Ok((bytes.len() <= MAX_SESSION_BYTES).then(|| CachedResponse {
        id: id.to_owned(),
        history: bytes,
    }))
}

async fn emit(tx: &mpsc::Sender<Output>, lane: &str, value: Value) -> Result<(), BridgeError> {
    tx.send(Output::Event {
        lane: lane.to_owned(),
        value,
    })
    .await
    .map_err(|_| BridgeError::upstream())
}
