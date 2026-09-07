//! Standalone Images API forwarding. Capability evidence comes only from real
//! requests and is independent of Responses tools, model discovery and health.
use super::*;
use serde::de::{IgnoredAny, Visitor};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operation {
    Generate,
    Edit,
}

impl Operation {
    pub(super) fn from_path(path: &str) -> Option<Self> {
        match path.trim_end_matches('/') {
            "/v1/images/generations" => Some(Self::Generate),
            "/v1/images/edits" => Some(Self::Edit),
            _ => None,
        }
    }

    fn path(self) -> &'static str {
        match self {
            Self::Generate => "/v1/images/generations",
            Self::Edit => "/v1/images/edits",
        }
    }

    fn completed(self) -> &'static str {
        match self {
            Self::Generate => "image_generation.completed",
            Self::Edit => "image_edit.completed",
        }
    }
}

pub(super) fn accepts_route(route: &ResolvedRoute) -> bool {
    matches!(
        route.config.inbound_protocol,
        ProxyProtocol::OpenAiResponses | ProxyProtocol::OpenAiChatCompletions
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Support {
    Supported,
    Unsupported,
}

struct Evidence {
    support: Support,
    started: Instant,
    expires: Instant,
}

/// Ephemeral observations are cleared when the proxy stops. No prompts, image payloads,
/// provider URLs, model names or credential fingerprints are written to disk.
#[derive(Default)]
pub(super) struct Ledger {
    entries: HashMap<[u8; 32], Evidence>,
}

impl Ledger {
    fn get(&mut self, key: &[u8; 32]) -> Option<Support> {
        self.entries.retain(|_, e| e.expires > Instant::now());
        self.entries.get(key).map(|e| e.support)
    }

    fn observe(&mut self, key: [u8; 32], support: Support, started: Instant) {
        if self.entries.get(&key).is_some_and(|e| e.started > started) {
            return;
        }
        self.entries.retain(|_, e| e.expires > Instant::now());
        if self.entries.len() >= 4096 && !self.entries.contains_key(&key) {
            if let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.expires)
                .map(|(k, _)| *k)
            {
                self.entries.remove(&oldest);
            }
        }
        // Gateway accounts and deployments can change without an AIPass edit.
        // Expiry permits another real request; it never schedules a probe.
        let ttl = match support {
            Support::Supported => Duration::from_secs(24 * 60 * 60),
            Support::Unsupported => Duration::from_secs(60 * 60),
        };
        self.entries.insert(
            key,
            Evidence {
                support,
                started,
                expires: Instant::now() + ttl,
            },
        );
    }
}

fn key(
    state: &RuntimeState,
    target: &ResolvedTarget,
    operation: Operation,
    model: Option<&str>,
    streaming: bool,
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(websocket::capability::key(state, target));
    hash.update(operation.path());
    hash.update([u8::from(streaming), u8::from(model.is_some())]);
    hash.update(model.unwrap_or_default());
    hash.finalize().into()
}

#[derive(Default, Deserialize)]
struct Metadata {
    model: Option<String>,
    #[serde(default)]
    stream: bool,
}

async fn metadata(
    body: &ReplayableRequestBody,
    headers: &HeaderMap,
) -> Result<Metadata, &'static str> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let metadata = if content_type
        .split(';')
        .next()
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("application/json"))
    {
        body.parse_metadata::<Metadata>()
            .await
            .ok_or("invalid Images JSON request")?
    } else if content_type
        .split(';')
        .next()
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("multipart/form-data"))
    {
        let boundary =
            multer::parse_boundary(content_type).map_err(|_| "invalid multipart boundary")?;
        let chunks: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>> = match body {
            ReplayableRequestBody::Memory(bytes) => {
                Box::pin(stream::once(std::future::ready(Ok(bytes.clone()))))
            }
            ReplayableRequestBody::File { file, .. } => {
                let file = file
                    .try_clone()
                    .map_err(|_| "failed to read image upload")?;
                let mut file = tokio::fs::File::from_std(file);
                file.seek(std::io::SeekFrom::Start(0))
                    .await
                    .map_err(|_| "failed to read image upload")?;
                Box::pin(stream::try_unfold(file, |mut file| async move {
                    let mut bytes = vec![0; 64 * 1024];
                    let len = file.read(&mut bytes).await?;
                    bytes.truncate(len);
                    Ok((len > 0).then(|| (Bytes::from(bytes), file)))
                }))
            }
        };
        let limits = multer::SizeLimit::new()
            .whole_stream(MAX_REQUEST_BODY_BYTES as u64)
            .for_field("model", 512)
            .for_field("stream", 16);
        let mut form = multer::Multipart::with_constraints(
            chunks,
            boundary,
            multer::Constraints::new().size_limit(limits),
        );
        let mut result = Metadata::default();
        let mut seen_model = false;
        let mut seen_stream = false;
        while let Some(field) = form
            .next_field()
            .await
            .map_err(|_| "invalid image multipart request")?
        {
            match field.name() {
                Some("model") if !seen_model && field.file_name().is_none() => {
                    seen_model = true;
                    result.model = Some(
                        field
                            .text()
                            .await
                            .map_err(|_| "invalid image model field")?,
                    );
                }
                Some("stream") if !seen_stream && field.file_name().is_none() => {
                    seen_stream = true;
                    result.stream = match field
                        .text()
                        .await
                        .map_err(|_| "invalid image stream field")?
                        .as_str()
                    {
                        "true" => true,
                        "false" => false,
                        _ => return Err("image stream field must be true or false"),
                    };
                }
                Some("model" | "stream") => {
                    return Err("duplicate or invalid image metadata field")
                }
                _ => {} // multer skips file/prompt bytes without materializing them.
            }
        }
        result
    } else {
        return Err("Images API requires application/json or multipart/form-data");
    };
    if metadata
        .model
        .as_ref()
        .is_some_and(|m| m.is_empty() || m.len() > 512)
    {
        return Err("invalid image model field");
    }
    Ok(metadata)
}

fn candidates(
    state: &RuntimeState,
    route: &ResolvedRoute,
    operation: Operation,
    metadata: &Metadata,
) -> Vec<ResolvedTarget> {
    let mut eligible = route.clone();
    eligible.targets.retain(|target| {
        let protocol = target
            .config
            .effective_protocol(route.config.upstream_protocol);
        matches!(
            protocol,
            ProxyProtocol::OpenAiResponses | ProxyProtocol::OpenAiChatCompletions
        ) && state.image_capabilities.lock().ok().and_then(|mut ledger| {
            ledger.get(&key(
                state,
                target,
                operation,
                metadata.model.as_deref(),
                metadata.stream,
            ))
        }) != Some(Support::Unsupported)
    });
    // Filter before the attempt limit, so excluded targets do not consume slots.
    // Image calls have no Responses lineage and must not steal its affinity.
    let mut targets = ordered_route_targets(state, &eligible, None);
    targets.sort_by_key(|target| {
        state.image_capabilities.lock().ok().and_then(|mut ledger| {
            ledger.get(&key(
                state,
                target,
                operation,
                metadata.model.as_deref(),
                metadata.stream,
            ))
        }) != Some(Support::Supported)
    });
    targets
}

/// Deliberately conservative: bare 404s, permissions, quota, safety failures,
/// invalid sizes and generic invalid_request errors are not capability evidence.
fn unsupported(status: StatusCode, bytes: &[u8]) -> bool {
    if !matches!(status.as_u16(), 400 | 404 | 405 | 422 | 501) {
        return false;
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return false;
    };
    let Some(error) = value.get("error").filter(|e| e.is_object()) else {
        return false;
    };
    let code = error.get("code").and_then(|v| v.as_str()).unwrap_or("");
    if matches!(
        code,
        "unsupported_endpoint"
            | "endpoint_not_supported"
            | "unsupported_model"
            | "model_not_supported"
            | "image_generation_not_supported"
            | "image_edit_not_supported"
    ) {
        return true;
    }
    let message = error
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let names_image = [
        "image generation",
        "image editing",
        "images/generations",
        "images/edits",
        "image_generation",
        "image_edit",
    ]
    .iter()
    .any(|s| message.contains(s));
    names_image
        && [
            "not supported",
            "does not support",
            "unsupported endpoint",
            "not implemented",
        ]
        .iter()
        .any(|s| message.contains(s))
        && ![
            "size",
            "quality",
            "format",
            "background",
            "parameter",
            "stream",
            "permission",
            "quota",
            "balance",
            "policy",
        ]
        .iter()
        .any(|s| message.contains(s))
}

async fn error_body(response: reqwest::Response) -> Zeroizing<Vec<u8>> {
    let mut body = Zeroizing::new(Vec::new());
    let mut chunks = response.bytes_stream();
    let _ = tokio::time::timeout(Duration::from_secs(1), async {
        while let Some(Ok(bytes)) = chunks.next().await {
            let left = (16 * 1024_usize).saturating_sub(body.len());
            body.extend_from_slice(&bytes[..bytes.len().min(left)]);
            if body.len() >= 16 * 1024 {
                break;
            }
        }
    })
    .await;
    body
}

fn has_string<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    struct Nonempty;
    impl<'de> Visitor<'de> for Nonempty {
        type Value = bool;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a string or null")
        }
        fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<bool, E> {
            Ok(!value.is_empty())
        }
        fn visit_unit<E: serde::de::Error>(self) -> Result<bool, E> {
            Ok(false)
        }
    }
    deserializer.deserialize_any(Nonempty)
}

#[derive(Default, Deserialize)]
struct ImageOutput {
    #[serde(default, deserialize_with = "has_string")]
    b64_json: bool,
    #[serde(default, deserialize_with = "has_string")]
    url: bool,
}

#[derive(Default, Deserialize)]
struct ImageResponse {
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(default, deserialize_with = "has_string")]
    b64_json: bool,
    data: Option<Vec<ImageOutput>>,
    error: Option<IgnoredAny>,
}

impl ImageResponse {
    fn has_image(&self) -> bool {
        self.b64_json
            || self
                .data
                .as_ref()
                .is_some_and(|items| items.iter().any(|i| i.b64_json || i.url))
    }
    fn failed(&self) -> bool {
        self.error.is_some()
            || self
                .kind
                .as_deref()
                .is_some_and(|k| k == "error" || k.ends_with(".failed"))
    }
}

struct Observer {
    buffer: Zeroizing<Vec<u8>>,
    operation: Operation,
    streaming: bool,
    completed: bool,
    failed: bool,
    unsupported: bool,
}

impl Observer {
    fn new(operation: Operation, streaming: bool) -> Self {
        Self {
            buffer: Zeroizing::new(Vec::new()),
            operation,
            streaming,
            completed: false,
            failed: false,
            unsupported: false,
        }
    }
    fn push(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
        if self.buffer.len().saturating_add(bytes.len()) > MAX_BUFFERED_RESPONSE_BYTES {
            return Err("image response event exceeds proxy buffer limit");
        }
        self.buffer.extend_from_slice(bytes);
        if self.streaming {
            let mut offset = 0;
            while let Some(end) =
                sse_event_boundary_end(&self.buffer[offset..]).map(|end| offset + end)
            {
                if let Some(data) = sse_event_data(&self.buffer[offset..end]) {
                    let data = Zeroizing::new(data);
                    if let Ok(value) = serde_json::from_slice::<ImageResponse>(&data) {
                        self.failed |= value.failed();
                        self.unsupported |=
                            value.failed() && unsupported(StatusCode::BAD_REQUEST, &data);
                        self.completed |= value.kind.as_deref() == Some(self.operation.completed())
                            && value.has_image()
                            && !value.failed();
                    }
                }
                offset = end;
            }
            if offset > 0 {
                self.buffer[..offset].zeroize();
                self.buffer.drain(..offset);
            }
        }
        Ok(())
    }
    fn finish(&mut self) {
        if !self.streaming {
            if let Ok(value) = serde_json::from_slice::<ImageResponse>(&self.buffer) {
                self.failed |= value.failed();
                self.unsupported |=
                    value.failed() && unsupported(StatusCode::BAD_REQUEST, &self.buffer);
                self.completed = value.has_image() && !value.failed();
            }
        }
    }
}

struct Attempt {
    state: RuntimeState,
    target: ResolvedTarget,
    route_id: Uuid,
    request_id: Uuid,
    model: Option<String>,
    inbound: ProxyProtocol,
    upstream: ProxyProtocol,
    retry: RetryPolicy,
    operation: Operation,
    key: [u8; 32],
    started: Instant,
    started_at: i64,
    request_started: Instant,
    request_started_at: i64,
    number: u8,
    status: Option<StatusCode>,
    outcome: Option<bool>,
    record_request: bool,
}

impl Attempt {
    fn evidence(&self, support: Support) {
        if let Ok(mut ledger) = self.state.image_capabilities.lock() {
            ledger.observe(self.key, support, self.started);
        }
        self.state.usage.log_diagnostic("info", format!("event=proxy.images.capability request_id={} target_id={} operation={:?} support={support:?}", self.request_id, self.target.config.id, self.operation));
    }
}

impl Drop for Attempt {
    fn drop(&mut self) {
        let _ = self.state.usage.record_attempt(&AttemptRecord {
            id: Uuid::new_v4(),
            request_id: Some(self.request_id),
            started_at: self.started_at,
            duration_ms: self.started.elapsed().as_millis() as u64,
            first_token_ms: None,
            route_id: self.route_id,
            target_id: self.target.config.id,
            provider_entry_id: self.target.config.provider_entry_id,
            secret_id: self.target.config.secret_id.clone(),
            model: self.model.clone(),
            status: self.status.map(|s| s.as_u16()),
            success: self.outcome,
        });
        if self.record_request {
            if let Some(success) = self.outcome {
                record_request(&self.state, success, None);
            }
            let _ = self.state.usage.record(&UsageRecord {
                id: self.request_id,
                started_at: self.request_started_at,
                duration_ms: self.request_started.elapsed().as_millis() as u64,
                first_token_ms: None,
                route_id: self.route_id,
                provider_entry_id: self.target.config.provider_entry_id,
                secret_id: self.target.config.secret_id.clone(),
                model: self.model.clone(),
                inbound_protocol: self.inbound,
                upstream_protocol: self.upstream,
                status: if self.outcome == Some(true) {
                    self.status.unwrap_or(StatusCode::OK).as_u16()
                } else {
                    502
                },
                attempts: self.number,
                // Image billing differs from mainline text token pricing. Do not
                // invent a text-model cost from an image response's usage object.
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                estimated_cost_micros: 0,
            });
        }
    }
}

pub(super) async fn forward(
    request: ForwardRequest,
    state: RuntimeState,
    route: ResolvedRoute,
    operation: Operation,
) -> Response<BoxBody> {
    let metadata = match metadata(&request.body, &request.incoming_headers).await {
        Ok(metadata) => metadata,
        Err(message) => return error_response(StatusCode::BAD_REQUEST, message),
    };
    let mut targets = candidates(&state, &route, operation, &metadata);
    state.usage.log_diagnostic(
        "info",
        format!(
            "event=proxy.images.routing request_id={} operation={operation:?} eligible={}",
            request.request_id,
            targets.len()
        ),
    );
    if targets.is_empty() {
        record_request(&state, false, None);
        return error_response(StatusCode::SERVICE_UNAVAILABLE, "no eligible provider for this Images API request; providers are disabled, unavailable, or known not to support it");
    }
    let mut final_attempt = None;
    let mut attempts = 0;
    let mut capacity_skipped = false;
    let mut hold_round = 0;
    let deadline = hold_deadline(&route.config.retry, request.started);
    let mut changed = request.config_changed.clone();
    loop {
        for target in targets {
            if attempts >= route.config.retry.max_attempts.max(1) {
                break;
            }
            let key = key(
                &state,
                &target,
                operation,
                metadata.model.as_deref(),
                metadata.stream,
            );
            // Another in-flight request may have learned a rejection after ranking.
            if state
                .image_capabilities
                .lock()
                .ok()
                .and_then(|mut ledger| ledger.get(&key))
                == Some(Support::Unsupported)
            {
                continue;
            }
            let Some(recovery) = RecoveryPermit::acquire(&state, target.config.id) else {
                continue;
            };
            let Some(provider_permit) = ProviderPermit::acquire(&state, &target) else {
                capacity_skipped = true;
                continue;
            };
            attempts += 1;
            let activity = TargetActivityGuard::new(&state, target.config.id);
            let protocol = target
                .config
                .effective_protocol(route.config.upstream_protocol);
            let mut attempt = Attempt {
                state: state.clone(),
                target,
                route_id: route.config.id,
                request_id: request.request_id,
                model: metadata.model.clone(),
                inbound: route.config.inbound_protocol,
                upstream: protocol,
                retry: route.config.retry.clone(),
                operation,
                key,
                started: Instant::now(),
                started_at: now_unix(),
                request_started: request.started,
                request_started_at: request.started_at,
                number: attempts,
                status: None,
                outcome: None,
                record_request: false,
            };
            // Keep only the last failed attempt for request-level accounting.
            drop(final_attempt.take());
            let setup = async {
                let client = upstream_client(&state, route.config.retry.connect_timeout_ms)?;
                let path = if attempt.target.config.auth_scheme == "azure_api_key" {
                    operation.path().trim_start_matches("/v1")
                } else {
                    operation.path()
                };
                let url = upstream_url_with_query(
                    &attempt.target.config.base_url,
                    path,
                    request.request_query.as_deref(),
                )
                .map_err(|e| e.to_string())?;
                let mut headers =
                    build_upstream_headers(&request.incoming_headers, &attempt.target, protocol)?;
                headers.insert(
                    header::CONTENT_LENGTH,
                    HeaderValue::from_str(&request.body.len().to_string())
                        .map_err(|e| e.to_string())?,
                );
                let payload = request
                    .body
                    .request_body()
                    .await
                    .map_err(|e| e.to_string())?;
                Ok::<_, String>(client.post(url).headers(headers).body(payload))
            }
            .await;
            let upstream = match setup {
                Ok(upstream) => upstream,
                Err(_) => {
                    attempt.outcome = Some(false);
                    final_attempt = Some(attempt);
                    continue;
                }
            };
            state.usage.log_diagnostic("info", format!("event=proxy.images.forwarding request_id={} target_id={} operation={operation:?} attempt={}", request.request_id, attempt.target.config.id, attempt.number));
            let response = match upstream.send().await {
                Ok(response) => response,
                Err(error) => {
                    let safe_retry = error.is_connect();
                    attempt.outcome = Some(false);
                    mark_failure(&state, attempt.target.config.id, &route.config.retry);
                    final_attempt = Some(attempt);
                    if safe_retry {
                        continue;
                    }
                    // A timeout/lost response may already have generated and billed.
                    break;
                }
            };
            let status = response.status();
            attempt.status = Some(status);
            if !status.is_success() {
                let body = error_body(response).await;
                let detail = diagnostics::upstream::error_detail(
                    &body,
                    &attempt.target,
                    &local_token_redactions(&request.incoming_headers),
                );
                state.usage.log_upstream_error(
                    request.request_id,
                    route.config.id,
                    &attempt.target.config,
                    status,
                    "images_http",
                    &detail,
                );
                let unavailable = unsupported(status, &body);
                if unavailable {
                    attempt.evidence(Support::Unsupported);
                }
                if !unavailable && status_affects_circuit(status) {
                    mark_failure(&state, attempt.target.config.id, &route.config.retry);
                }
                attempt.outcome = Some(false);
                final_attempt = Some(attempt);
                // Only explicit pre-execution rejection permits replay. In particular,
                // an arbitrary 5xx after submission is ambiguous for image generation.
                if unavailable || matches!(status.as_u16(), 400 | 401 | 403 | 404 | 405 | 422 | 429)
                {
                    continue;
                }
                break;
            }
            let response_headers = response.headers().clone();
            let streaming = response_headers
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .is_some_and(is_event_stream);
            let stream_body = if streaming {
                attempt.record_request = true;
                let stream = relay(response, attempt, request.config_changed.clone(), operation);
                BodyExt::boxed_unsync(StreamBody::new(
                    stream.map(|result| result.map(Frame::data)),
                ))
            } else {
                // Validate the complete JSON result before returning success. Lost or
                // truncated responses remain ambiguous and must not be replayed.
                let mut source: UpstreamBodyStream = Box::pin(response.bytes_stream());
                let buffered = match collect_upstream_body(None, &mut source).await {
                    Ok(bytes) => bytes,
                    Err(_) => {
                        attempt.outcome = Some(false);
                        final_attempt = Some(attempt);
                        break;
                    }
                };
                let mut observed = Observer::new(operation, false);
                let valid = observed.push(&buffered).is_ok();
                observed.finish();
                if !valid || !observed.completed || observed.failed {
                    if observed.failed {
                        let detail = diagnostics::upstream::error_detail(
                            &buffered,
                            &attempt.target,
                            &local_token_redactions(&request.incoming_headers),
                        );
                        state.usage.log_upstream_error(
                            request.request_id,
                            route.config.id,
                            &attempt.target.config,
                            status,
                            "images_http",
                            &detail,
                        );
                    }
                    if observed.unsupported {
                        attempt.evidence(Support::Unsupported);
                    }
                    attempt.outcome = Some(false);
                    final_attempt = Some(attempt);
                    if observed.unsupported {
                        continue;
                    }
                    break;
                }
                attempt.outcome = Some(true);
                attempt.record_request = true;
                attempt.evidence(Support::Supported);
                complete_target_success(
                    &state,
                    route.config.id,
                    None,
                    attempt.target.config.id,
                    attempt.started,
                    None,
                );
                BodyExt::boxed_unsync(
                    Full::new(buffered).map_err(|never| -> BoxError { match never {} }),
                )
            };
            let mut response = Response::builder().status(status);
            let hop = connection_header_names(&response_headers);
            for (name, value) in &response_headers {
                if !is_hop_header(name) && !hop.contains(name) && name != header::CONTENT_LENGTH {
                    response = response.header(name, value);
                }
            }
            return attach_in_flight_guard(
                response.body(stream_body).unwrap_or_else(|_| {
                    error_response(StatusCode::BAD_GATEWAY, "failed to build image response")
                }),
                Some((activity, recovery, provider_permit)),
            );
        }
        // Wait only when nothing was submitted. Image requests with ambiguous
        // upstream results retain their existing no-replay behavior.
        if attempts > 0 || !capacity_skipped || !route.config.retry.hold_on_failure {
            break;
        }
        let mut delay = hold_backoff_delay(&route.config.retry, hold_round);
        if let Some(deadline) = deadline {
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            delay = delay.min(deadline.saturating_duration_since(tokio::time::Instant::now()));
        }
        tokio::select! {
            _ = changed.changed() => return error_response(StatusCode::SERVICE_UNAVAILABLE, "proxy configuration changed"),
            _ = tokio::time::sleep(delay) => {}
        }
        if deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
            break;
        }
        hold_round = hold_round.saturating_add(1);
        capacity_skipped = false;
        targets = candidates(&state, &route, operation, &metadata);
    }
    if let Some(mut attempt) = final_attempt {
        attempt.record_request = true;
    } else {
        record_request(&state, false, None);
    }
    if capacity_skipped && attempts == 0 {
        return capacity_response();
    }
    error_response(
        StatusCode::BAD_GATEWAY,
        "image request failed; no usable upstream result (ambiguous submissions are not replayed)",
    )
}

fn relay(
    response: reqwest::Response,
    mut attempt: Attempt,
    mut changed: ConfigWatch,
    operation: Operation,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, BoxError>> + Send>> {
    let (sender, receiver) = tokio::sync::mpsc::channel(8);
    tokio::spawn(async move {
        let mut source = response.bytes_stream();
        let mut observer = Observer::new(operation, true);
        loop {
            let next = tokio::select! {
                biased;
                _ = changed.changed() => return,
                _ = sender.closed() => return,
                next = source.next() => next,
            };
            let result = match next {
                Some(Ok(bytes)) => observer.push(&bytes).map(|_| Some(bytes)),
                None => {
                    observer.finish();
                    Ok(None)
                }
                Some(Err(_)) => Err("upstream image stream failed"),
            };
            match result {
                Ok(bytes) => {
                    let ended = bytes.is_none();
                    if let Some(bytes) = bytes {
                        // Preserve all event bytes, including previews and errors.
                        tokio::select! {
                            biased;
                            _ = changed.changed() => return,
                            sent = sender.send(Ok(bytes)) => if sent.is_err() { return; },
                        }
                    }
                    if observer.failed || ended || observer.completed {
                        let success = observer.completed && !observer.failed;
                        attempt.outcome = Some(success);
                        if success {
                            attempt.evidence(Support::Supported);
                            complete_target_success(
                                &attempt.state,
                                attempt.route_id,
                                None,
                                attempt.target.config.id,
                                attempt.started,
                                None,
                            );
                        } else {
                            if observer.unsupported && !observer.completed {
                                attempt.evidence(Support::Unsupported);
                            }
                            // No image is not proof of missing capability; never
                            // replay or blacklist on a model refusal/partial result.
                            // An explicit SSE error has already been forwarded;
                            // preserve it instead of replacing it with a broken
                            // HTTP body. EOF without a terminal result is truncated.
                            if !observer.failed {
                                let _ = sender
                                    .send(Err(std::io::Error::other(
                                        "upstream did not complete an image result",
                                    )
                                    .into()))
                                    .await;
                            }
                        }
                        return;
                    }
                }
                Err(message) => {
                    attempt.outcome = Some(false);
                    mark_failure(&attempt.state, attempt.target.config.id, &attempt.retry);
                    let _ = sender
                        .send(Err(std::io::Error::other(message).into()))
                        .await;
                    return;
                }
            }
        }
    });
    Box::pin(stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|item| (item, receiver))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_capability_requires_specific_errors_not_auth_quota_or_parameters() {
        for (status, body, expected) in [
            (400, r#"{"error":{"code":"unsupported_model"}}"#, true),
            (404, r#"{"error":{"code":"unsupported_endpoint"}}"#, true),
            (
                400,
                r#"{"error":{"message":"This model does not support image generation"}}"#,
                true,
            ),
            (404, r#"{"error":{"message":"not found"}}"#, false),
            (404, "not found", false),
            (403, r#"{"error":{"code":"unsupported_model"}}"#, false),
            (
                429,
                r#"{"error":{"message":"image generation is not supported"}}"#,
                false,
            ),
            (500, r#"{"error":{"code":"unsupported_model"}}"#, false),
            (
                400,
                r#"{"error":{"message":"image generation size is not supported"}}"#,
                false,
            ),
            (
                400,
                r#"{"error":{"message":"image generation stream is not supported"}}"#,
                false,
            ),
            (
                400,
                r#"{"input":"image generation is not supported"}"#,
                false,
            ),
        ] {
            assert_eq!(
                unsupported(StatusCode::from_u16(status).unwrap(), body.as_bytes()),
                expected,
                "{body}"
            );
        }
    }

    #[test]
    fn image_capability_cache_expires_and_ignores_older_in_flight_evidence() {
        let mut ledger = Ledger::default();
        let key = [1; 32];
        let old = Instant::now();
        let new = old + Duration::from_millis(1);
        ledger.observe(key, Support::Supported, new);
        ledger.observe(key, Support::Unsupported, old);
        assert_eq!(ledger.get(&key), Some(Support::Supported));
        ledger.observe(key, Support::Unsupported, new + Duration::from_millis(1));
        assert_eq!(ledger.get(&key), Some(Support::Unsupported));
        ledger.entries.get_mut(&key).unwrap().expires = Instant::now() - Duration::from_secs(1);
        assert_eq!(ledger.get(&key), None);
    }

    #[test]
    fn image_observation_distinguishes_previews_errors_and_operation_completion() {
        for operation in [Operation::Generate, Operation::Edit] {
            let mut observer = Observer::new(operation, true);
            observer
                .push(
                    b"data: {\"type\":\"image_generation.partial_image\",\"b64_json\":\"abc\"}\n\n",
                )
                .unwrap();
            assert!(!observer.completed);
            let data = format!(
                "data: {{\"type\":\"{}\",\"b64_json\":\"image\"}}\r\n\r\n",
                operation.completed()
            );
            for chunk in data.as_bytes().chunks(3) {
                observer.push(chunk).unwrap();
            }
            assert!(observer.completed);
            assert!(!observer.failed);
        }
        for bytes in [
            b"{}".as_slice(),
            br#"{"data":[]}"#,
            br#"{"data":[{"b64_json":""}]}"#,
            br#"{"output":[{"type":"image_generation_call","result":"image"}]}"#,
        ] {
            let mut observer = Observer::new(Operation::Generate, false);
            observer.push(bytes).unwrap();
            observer.finish();
            assert!(!observer.completed);
        }
        let mut observer = Observer::new(Operation::Generate, true);
        observer
            .push(b"data: {\"type\":\"error\",\"error\":{\"code\":\"unsupported_endpoint\"}}\n\n")
            .unwrap();
        assert!(observer.failed && observer.unsupported);
    }
}
