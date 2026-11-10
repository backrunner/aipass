use super::*;
mod conversion_tracking;
mod progress;
pub(crate) use conversion_tracking::track_converted_usage_stream;
pub(crate) use progress::buffered_response_has_output;

pub(crate) struct PrefetchedSse {
    pub(crate) bytes: Bytes,
    pub(crate) first_token_observed: bool,
}

pub(crate) struct PrefetchError {
    pub(crate) message: String,
    pub(crate) confirmed_failure: bool,
}

impl From<String> for PrefetchError {
    fn from(message: String) -> Self {
        Self {
            message,
            confirmed_failure: false,
        }
    }
}
impl From<&str> for PrefetchError {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}

pub(crate) async fn prefetch_sse_event(
    protocol: ProxyProtocol,
    first_chunk: Option<Bytes>,
    source: &mut UpstreamBodyStream,
) -> Result<Option<PrefetchedSse>, PrefetchError> {
    let Some(first_chunk) = first_chunk else {
        return Ok(None);
    };
    let mut buffered = first_chunk.to_vec();
    let mut inspected = 0;
    loop {
        if buffered.len() > MAX_BUFFERED_RESPONSE_BYTES {
            return Err("upstream first event exceeds proxy buffer limit".into());
        }
        while let Some(relative_end) = sse_event_boundary_end(&buffered[inspected..]) {
            let event_end = inspected + relative_end;
            let event = &buffered[inspected..event_end];
            inspected = event_end;
            if sse_event_reports_error(event) {
                return Err(PrefetchError {
                    message: "upstream returned an error event".into(),
                    confirmed_failure: !sse_event_reports_output(protocol, event),
                });
            }
            if sse_event_is_heartbeat(event) {
                continue;
            }
            if sse_event_reports_output(protocol, event) {
                return Ok(Some(PrefetchedSse {
                    bytes: Bytes::from(buffered),
                    first_token_observed: true,
                }));
            }
            if sse_event_reports_completion(protocol, event) {
                return Ok(Some(PrefetchedSse {
                    bytes: Bytes::from(buffered),
                    first_token_observed: false,
                }));
            }
        }
        match source.next().await {
            Some(Ok(chunk)) => buffered.extend_from_slice(&chunk),
            Some(Err(err)) => return Err(err.to_string().into()),
            None => return Err("upstream stream ended before the first complete event".into()),
        }
    }
}

pub(crate) fn sse_event_boundary_end(bytes: &[u8]) -> Option<usize> {
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        if matches!(bytes[index], b'\r' | b'\n') {
            let empty = index == start;
            let crlf = bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n');
            index += if crlf { 2 } else { 1 };
            if empty {
                return Some(index);
            }
            start = index;
        } else {
            index += 1;
        }
    }
    None
}

pub(crate) fn sse_event_reports_error(event: &[u8]) -> bool {
    // An event name without nonempty data is a control frame, not a
    // dispatched error. It cannot authorize replaying a generation.
    let Some(data) = sse_event_data(event).filter(|data| !trim_ascii(data).is_empty()) else {
        return false;
    };
    let event_is_error = sse_event_name(event)
        .is_some_and(|name| name.eq_ignore_ascii_case(b"error") || name.ends_with(b".failed"));
    if event_is_error {
        return true;
    }
    is_upstream_error_payload(&data)
}

pub(crate) fn sse_event_reports_output(protocol: ProxyProtocol, event: &[u8]) -> bool {
    let Some(data) = sse_event_data(event) else {
        return false;
    };
    let data = trim_ascii(&data);
    if data.is_empty() || data == b"[DONE]" {
        return false;
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(data) else {
        return !sse_event_reports_error(event);
    };
    match protocol {
        ProxyProtocol::OpenAiResponses => {
            if progress::response_output(&value)
                || value.get("response").is_some_and(progress::response_output)
                || value.get("item").is_some_and(progress::item_output)
                || value.get("part").is_some_and(progress::part_output)
            {
                return true;
            }
            let Some(kind) = value.get("type").and_then(serde_json::Value::as_str) else {
                return !sse_event_reports_error(event);
            };
            if kind == "response.output_item.added" {
                let item = &value["item"];
                return item["type"]
                    .as_str()
                    .is_some_and(|kind| kind.ends_with("_call"))
                    || item
                        .get("encrypted_content")
                        .is_some_and(json_value_has_output);
            }
            if matches!(
                kind,
                "response.created"
                    | "response.in_progress"
                    | "response.output_item.added"
                    | "response.content_part.added"
            ) {
                return false;
            }
            if kind.ends_with(".delta") {
                return value.get("delta").is_some_and(json_value_has_output);
            }
            value
                .get("text")
                .or_else(|| value.get("content"))
                .is_some_and(json_value_has_output)
        }
        ProxyProtocol::OpenAiChatCompletions => {
            let Some(choices) = value.get("choices").and_then(serde_json::Value::as_array) else {
                return value.get("usage").is_none() && !sse_event_reports_error(event);
            };
            choices.iter().any(|choice| {
                let Some(delta) = choice.get("delta").or_else(|| choice.get("message")) else {
                    return false;
                };
                delta.get("content").is_some_and(json_value_has_output)
                    || ["reasoning_content", "reasoning", "refusal", "audio"]
                        .iter()
                        .any(|key| delta.get(*key).is_some_and(json_value_has_output))
                    || delta.get("tool_calls").is_some_and(json_value_has_output)
                    || delta
                        .get("function_call")
                        .is_some_and(json_value_has_output)
            })
        }
        ProxyProtocol::AnthropicMessages => {
            let Some(kind) = value.get("type").and_then(serde_json::Value::as_str) else {
                return !sse_event_reports_error(event);
            };
            match kind {
                "message_start" => value["message"]["content"]
                    .as_array()
                    .is_some_and(|parts| parts.iter().any(progress::part_output)),
                "message_delta" | "message_stop" | "error" => false,
                "content_block_start" => progress::part_output(&value["content_block"]),
                "content_block_delta" => value.get("delta").is_some_and(json_value_has_output),
                _ => true,
            }
        }
    }
}

pub(crate) fn json_value_has_output(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(_) | serde_json::Value::Number(_) => true,
        serde_json::Value::String(value) => !value.is_empty(),
        serde_json::Value::Array(value) => !value.is_empty(),
        serde_json::Value::Object(value) => !value.is_empty(),
    }
}

pub(crate) fn sse_event_is_heartbeat(event: &[u8]) -> bool {
    if sse_event_name(event).is_some_and(|name| {
        name.eq_ignore_ascii_case(b"ping") || name.eq_ignore_ascii_case(b"heartbeat")
    }) {
        return true;
    }
    sse_event_data(event)
        .and_then(|data| serde_json::from_slice::<serde_json::Value>(&data).ok())
        .is_some_and(|value| {
            value
                .get("type")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|kind| {
                    kind.eq_ignore_ascii_case("ping") || kind.eq_ignore_ascii_case("heartbeat")
                })
        })
}

pub(crate) fn sse_event_name(event: &[u8]) -> Option<&[u8]> {
    event
        .strip_prefix(b"\xef\xbb\xbf")
        .unwrap_or(event)
        .split(|byte| matches!(byte, b'\r' | b'\n'))
        .filter_map(|line| {
            if line == b"event" {
                Some(&[][..])
            } else {
                line.strip_prefix(b"event:")
                    .map(|v| v.strip_prefix(b" ").unwrap_or(v))
            }
        })
        .next_back()
}

pub(crate) fn sse_event_data(event: &[u8]) -> Option<Vec<u8>> {
    let mut found = false;
    let mut data = Vec::new();
    for line in event
        .strip_prefix(b"\xef\xbb\xbf")
        .unwrap_or(event)
        .split(|byte| matches!(byte, b'\r' | b'\n'))
    {
        let value = if line == b"data" {
            &[][..]
        } else if let Some(value) = line.strip_prefix(b"data:") {
            value.strip_prefix(b" ").unwrap_or(value)
        } else {
            continue;
        };
        if found {
            data.push(b'\n');
        }
        found = true;
        data.extend_from_slice(value);
    }
    found.then_some(data)
}

pub(crate) fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

pub(crate) fn is_upstream_error_payload(bytes: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .is_some_and(|value| {
            value.get("error").is_some_and(|error| !error.is_null())
                || value
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|kind| kind == "error" || kind.ends_with(".failed"))
                || value.get("status").and_then(serde_json::Value::as_str) == Some("failed")
                || value
                    .pointer("/response/error")
                    .is_some_and(|error| !error.is_null())
                || value
                    .pointer("/response/status")
                    .and_then(serde_json::Value::as_str)
                    == Some("failed")
        })
}

pub(crate) fn stream_reports_error(bytes: &[u8]) -> bool {
    let mut offset = 0;
    while let Some(relative_end) = sse_event_boundary_end(&bytes[offset..]) {
        let end = offset + relative_end;
        if sse_event_reports_error(&bytes[offset..end]) {
            return true;
        }
        offset = end;
    }
    offset < bytes.len() && sse_event_reports_error(&bytes[offset..])
}

pub(crate) fn stream_reports_completion(protocol: ProxyProtocol, bytes: &[u8]) -> bool {
    let mut offset = 0;
    while let Some(relative_end) = sse_event_boundary_end(&bytes[offset..]) {
        let end = offset + relative_end;
        if sse_event_reports_completion(protocol, &bytes[offset..end]) {
            return true;
        }
        offset = end;
    }
    offset < bytes.len() && sse_event_reports_completion(protocol, &bytes[offset..])
}

pub(crate) fn stream_reports_output(protocol: ProxyProtocol, bytes: &[u8]) -> bool {
    let mut offset = 0;
    while offset < bytes.len() {
        let end = sse_event_boundary_end(&bytes[offset..])
            .map(|n| offset + n)
            .unwrap_or(bytes.len());
        let event = &bytes[offset..end];
        if !sse_event_is_heartbeat(event) && sse_event_reports_output(protocol, event) {
            return true;
        }
        offset = end;
    }
    false
}

pub(crate) fn sse_event_reports_completion(protocol: ProxyProtocol, event: &[u8]) -> bool {
    let Some(data) = sse_event_data(event) else {
        return false;
    };
    if data == b"[DONE]" && protocol != ProxyProtocol::AnthropicMessages {
        return true;
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&data) else {
        return false;
    };
    match protocol {
        ProxyProtocol::OpenAiResponses => {
            matches!(
                value.get("type").and_then(serde_json::Value::as_str),
                Some("response.completed" | "response.incomplete")
            ) || matches!(
                value
                    .pointer("/response/status")
                    .and_then(serde_json::Value::as_str),
                Some("completed" | "incomplete")
            )
        }
        ProxyProtocol::OpenAiChatCompletions => value
            .get("choices")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|choices| {
                !choices.is_empty()
                    && choices.iter().all(|choice| {
                        choice
                            .get("finish_reason")
                            .is_some_and(|reason| !reason.is_null())
                    })
            }),
        ProxyProtocol::AnthropicMessages => {
            value.get("type").and_then(serde_json::Value::as_str) == Some("message_stop")
        }
    }
}

pub(crate) fn sse_event_is_terminal(protocol: ProxyProtocol, event: &[u8]) -> bool {
    let Some(data) = sse_event_data(event) else {
        return false;
    };
    if protocol == ProxyProtocol::OpenAiChatCompletions {
        return trim_ascii(&data) == b"[DONE]";
    }
    sse_event_reports_completion(protocol, event)
}

pub(crate) fn is_event_stream(content_type: &str) -> bool {
    content_type
        .split(';')
        .next()
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/event-stream"))
}

pub(crate) async fn collect_upstream_body(
    first_chunk: Option<Bytes>,
    source: &mut UpstreamBodyStream,
) -> Result<Bytes, String> {
    let mut buffered = first_chunk.map_or_else(Vec::new, |chunk| chunk.to_vec());
    loop {
        if buffered.len() > MAX_BUFFERED_RESPONSE_BYTES {
            return Err("upstream response exceeds proxy buffer limit".into());
        }
        match source.next().await {
            Some(Ok(chunk)) => buffered.extend_from_slice(&chunk),
            Some(Err(err)) => return Err(err.to_string()),
            None => return Ok(Bytes::from(buffered)),
        }
    }
}

pub(crate) fn convert_sse_stream<S>(
    source: S,
    from: ProxyProtocol,
    to: ProxyProtocol,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, BoxError>> + Send>>
where
    S: Stream<Item = Result<Bytes, BoxError>> + Send + 'static,
{
    let converter = match StreamConverter::new(from, to) {
        Ok(converter) => converter,
        Err(err) => return Box::pin(stream::once(async move { Err(Box::new(err) as BoxError) })),
    };
    let source = Box::pin(source);
    Box::pin(stream::unfold(
        (
            source,
            Vec::<u8>::new(),
            VecDeque::<Bytes>::new(),
            converter,
            false,
        ),
        move |(mut source, mut buffer, mut pending, mut converter, mut done)| async move {
            loop {
                if let Some(value) = pending.pop_front() {
                    return Some((Ok(value), (source, buffer, pending, converter, done)));
                }
                if done {
                    return None;
                }
                if let Some(end) = sse_event_boundary_end(&buffer) {
                    let event = String::from_utf8_lossy(&buffer[..end]).to_string();
                    buffer.drain(..end);
                    match converter.push_event(&event) {
                        Ok(events) => pending.extend(events.into_iter().map(Bytes::from)),
                        Err(err) => {
                            done = true;
                            return Some((
                                Err(Box::new(err) as BoxError),
                                (source, buffer, pending, converter, done),
                            ));
                        }
                    }
                    continue;
                }
                match source.next().await {
                    Some(Ok(chunk)) => {
                        if buffer.len().saturating_add(chunk.len()) > MAX_BUFFERED_RESPONSE_BYTES {
                            done = true;
                            return Some((
                                Err(std::io::Error::other(
                                    "upstream event exceeds proxy buffer limit",
                                )
                                .into()),
                                (source, buffer, pending, converter, done),
                            ));
                        }
                        buffer.extend_from_slice(&chunk);
                    }
                    Some(Err(err)) => {
                        done = true;
                        return Some((Err(err), (source, buffer, pending, converter, done)));
                    }
                    None => {
                        done = true;
                        if !buffer.is_empty() {
                            let event = String::from_utf8_lossy(&buffer).to_string();
                            buffer.clear();
                            match converter.push_event(&event) {
                                Ok(events) => pending.extend(events.into_iter().map(Bytes::from)),
                                Err(err) => {
                                    return Some((
                                        Err(Box::new(err) as BoxError),
                                        (source, buffer, pending, converter, done),
                                    ));
                                }
                            }
                        }
                        match converter.finish() {
                            Ok(events) => pending.extend(events.into_iter().map(Bytes::from)),
                            Err(err) => {
                                return Some((
                                    Err(Box::new(err) as BoxError),
                                    (source, buffer, pending, converter, done),
                                ))
                            }
                        }
                    }
                }
            }
        },
    ))
}

pub(crate) struct UsageTrackingContext {
    pub(crate) ws_evidence: Option<websocket::capability::Evidence>,
    pub(crate) protocol: ProxyProtocol,
    pub(crate) store: Arc<UsageStore>,
    pub(crate) record: UsageRecord,
    pub(crate) pricing: Option<ModelPricing>,
    pub(crate) streaming: bool,
    pub(crate) attempt_started: Instant,
    pub(crate) started: Instant,
    pub(crate) failure_state: RuntimeState,
    pub(crate) config_changed: ConfigWatch,
    pub(crate) route_id: Uuid,
    pub(crate) target_id: Uuid,
    pub(crate) session_key: Option<String>,
    pub(crate) retry_policy: RetryPolicy,
    pub(crate) attempt: Option<(AttemptRecord, Instant)>,
}

pub(crate) fn track_usage_stream<S>(
    source: S,
    context: UsageTrackingContext,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, BoxError>> + Send>>
where
    S: Stream<Item = Result<Bytes, BoxError>> + Send + 'static,
{
    track_usage_stream_inner(source, context, None)
}

fn track_usage_stream_inner<S>(
    source: S,
    context: UsageTrackingContext,
    conversion: Option<tokio::sync::oneshot::Receiver<(bool, tokio::sync::oneshot::Sender<()>)>>,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, BoxError>> + Send>>
where
    S: Stream<Item = Result<Bytes, BoxError>> + Send + 'static,
{
    let UsageTrackingContext {
        ws_evidence,
        protocol,
        store,
        mut record,
        pricing,
        streaming,
        attempt_started,
        started,
        failure_state,
        mut config_changed,
        route_id,
        target_id,
        session_key,
        retry_policy,
        attempt,
    } = context;
    let mut source = Box::pin(source);
    let (sender, receiver) = tokio::sync::mpsc::channel(8);
    tokio::spawn(async move {
        let mut tail = Vec::new();
        let mut usage_event_buffer = Vec::new();
        let mut observed_usage = UsageAccumulator::default();
        let mut source_ended = false;
        let mut config_cancelled = false;
        let mut transport_failed = false;
        let mut protocol_completed = false;
        let mut protocol_terminal = false;
        let mut protocol_failed = false;
        let mut response_id = None;
        let mut response_trace = (streaming && protocol == ProxyProtocol::OpenAiResponses)
            .then(diagnostics::protocol::ResponseTrace::default);
        loop {
            let next = tokio::select! {
                _ = sender.closed() => break,
                _ = config_changed.changed() => { config_cancelled = true; break; },
                result = source.next() => result,
            };
            let result: Result<Bytes, BoxError> = match next {
                Some(result) => result,
                None => {
                    source_ended = true;
                    break;
                }
            };
            if let Ok(chunk) = &result {
                if streaming {
                    let signals = observe_sse_usage_traced(
                        protocol,
                        &mut usage_event_buffer,
                        chunk,
                        &mut observed_usage,
                        response_trace.as_mut(),
                    );
                    protocol_completed |= signals.completed;
                    protocol_terminal |= signals.terminal;
                    protocol_failed |= signals.failed;
                    response_id = signals.response_id.or(response_id);
                } else {
                    observe_usage_bytes(&mut observed_usage, protocol, chunk);
                }
                tail.extend_from_slice(chunk);
                const USAGE_TAIL_LIMIT: usize = 256 * 1024;
                if tail.len() > USAGE_TAIL_LIMIT {
                    tail.drain(..tail.len() - USAGE_TAIL_LIMIT);
                }
            }
            transport_failed = result.is_err();
            if let Err(err) = &result {
                mark_failure(&failure_state, target_id, &retry_policy);
                set_error(&failure_state, err.to_string());
            }
            if sender.send(result).await.is_err() {
                break;
            }
            if transport_failed || (streaming && (protocol_terminal || protocol_failed)) {
                break;
            }
        }
        // The converter needs EOF to validate and flush the terminal event.
        // Close its source before awaiting its outcome; otherwise both tasks
        // would wait for each other. A dropped converter means cancellation.
        let _keep_open = if conversion.is_none() {
            Some(sender)
        } else {
            drop(sender);
            None
        };
        let (converted, recorded) = match conversion {
            Some(result) => match result.await {
                Ok((success, recorded)) => (Some(success), Some(recorded)),
                Err(_) => (None, None),
            },
            None => (Some(true), None),
        };
        let conversion_failed = converted == Some(false) && !config_cancelled;
        let stream_succeeded = if streaming {
            (protocol_terminal || (source_ended && protocol_completed))
                && !protocol_failed
                && !transport_failed
        } else {
            source_ended && !transport_failed
        } && converted == Some(true)
            && !config_cancelled;
        if stream_succeeded {
            if protocol == ProxyProtocol::OpenAiResponses {
                websocket::capability::http_success(&failure_state, ws_evidence.as_ref());
            }
            if streaming {
                complete_target_success(
                    &failure_state,
                    route_id,
                    session_key.as_deref(),
                    target_id,
                    attempt_started,
                    response_id.as_deref(),
                );
            }
        } else if protocol_failed {
            mark_failure(&failure_state, target_id, &retry_policy);
            set_error(
                &failure_state,
                "upstream returned an error event after stream commit".into(),
            );
        } else if conversion_failed && !transport_failed {
            mark_failure(&failure_state, target_id, &retry_policy);
            set_error(
                &failure_state,
                "upstream stream could not be converted".into(),
            );
        } else if streaming && source_ended && !protocol_completed {
            mark_failure(&failure_state, target_id, &retry_policy);
            set_error(
                &failure_state,
                "upstream stream ended before protocol completion".into(),
            );
        }
        if !streaming {
            observe_usage_bytes(&mut observed_usage, protocol, &tail);
        }
        let observed_usage = observed_usage.totals(protocol);
        if let Some(trace) = response_trace {
            trace.log(&store, record.id, "http_sse_upstream");
        }
        record_recent_tokens(
            &failure_state,
            observed_usage
                .input_tokens
                .saturating_add(observed_usage.output_tokens)
                .saturating_add(observed_usage.cache_read_tokens)
                .saturating_add(observed_usage.cache_creation_tokens),
        );
        record.duration_ms = started.elapsed().as_millis() as u64;
        if !stream_succeeded {
            record.status = StatusCode::BAD_GATEWAY.as_u16();
        }
        if stream_succeeded
            || transport_failed
            || protocol_failed
            || conversion_failed
            || source_ended
        {
            record_request(&failure_state, stream_succeeded, record.first_token_ms);
        }
        record.input_tokens = observed_usage.input_tokens;
        record.output_tokens = observed_usage.output_tokens;
        record.cache_read_tokens = observed_usage.cache_read_tokens;
        record.cache_creation_tokens = observed_usage.cache_creation_tokens;
        record.cache_creation_1h_tokens = observed_usage.cache_creation_1h_tokens;
        record.estimated_cost_micros = pricing
            .as_ref()
            .map(|pricing| estimate_cost(&observed_usage, pricing))
            .unwrap_or(0);
        let _ = store.record(&record);
        if let Some((mut attempt, attempt_started)) = attempt {
            attempt.duration_ms = attempt_started.elapsed().as_millis() as u64;
            attempt.success = if stream_succeeded {
                Some(true)
            } else if transport_failed || protocol_failed || conversion_failed || source_ended {
                Some(false)
            } else {
                None
            };
            let _ = store.record_attempt(&attempt);
        }
        if let Some(recorded) = recorded {
            let _ = recorded.send(());
        }
    });
    Box::pin(stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|item| (item, receiver))
    }))
}

pub(crate) fn estimate_cost(usage: &TokenUsage, pricing: &ModelPricing) -> u64 {
    let hour = usage
        .cache_creation_1h_tokens
        .min(usage.cache_creation_tokens);
    let total = usage
        .input_tokens
        .saturating_mul(pricing.input_micros_per_million)
        .saturating_add(
            usage
                .output_tokens
                .saturating_mul(pricing.output_micros_per_million),
        )
        .saturating_add(
            usage
                .cache_read_tokens
                .saturating_mul(pricing.cache_read_micros_per_million),
        )
        .saturating_add(
            usage
                .cache_creation_tokens
                .saturating_sub(hour)
                .saturating_mul(pricing.cache_creation_micros_per_million),
        );
    total.saturating_add(
        hour.saturating_mul(
            pricing
                .cache_creation_1h_micros_per_million
                .unwrap_or(pricing.cache_creation_micros_per_million),
        ),
    ) / 1_000_000
}

#[cfg(test)]
pub(crate) fn usage_from_wire_bytes(protocol: ProxyProtocol, bytes: &[u8]) -> TokenUsage {
    let mut usage = UsageAccumulator::default();
    observe_usage_bytes(&mut usage, protocol, bytes);
    usage.totals(protocol)
}

fn observe_usage_bytes(usage: &mut UsageAccumulator, protocol: ProxyProtocol, bytes: &[u8]) {
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) {
        usage.observe(protocol, &value);
        return;
    }
    let text = String::from_utf8_lossy(bytes);
    for line in text
        .lines()
        .filter_map(|line| line.strip_prefix("data:").map(str::trim))
    {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        usage.observe(protocol, &value);
    }
}

pub(crate) fn usage_from_wire_value(
    protocol: ProxyProtocol,
    value: &serde_json::Value,
) -> TokenUsage {
    let mut usage = UsageAccumulator::default();
    usage.observe(protocol, value);
    usage.totals(protocol)
}

#[derive(Default)]
pub(crate) struct SseSignals {
    pub(crate) response_id: Option<String>,
    pub(crate) completed: bool,
    pub(crate) terminal: bool,
    pub(crate) failed: bool,
}

#[cfg(test)]
pub(crate) fn observe_sse_usage(
    protocol: ProxyProtocol,
    buffer: &mut Vec<u8>,
    chunk: &[u8],
    usage: &mut UsageAccumulator,
) -> SseSignals {
    observe_sse_usage_traced(protocol, buffer, chunk, usage, None)
}

pub(crate) fn observe_sse_usage_traced(
    protocol: ProxyProtocol,
    buffer: &mut Vec<u8>,
    chunk: &[u8],
    usage: &mut UsageAccumulator,
    mut trace: Option<&mut diagnostics::protocol::ResponseTrace>,
) -> SseSignals {
    buffer.extend_from_slice(chunk);
    let mut consumed = 0;
    let mut signals = SseSignals::default();
    while let Some(relative_end) = sse_event_boundary_end(&buffer[consumed..]) {
        let end = consumed + relative_end;
        let event = &buffer[consumed..end];
        signals.failed |= sse_event_reports_error(event);
        signals.completed |= sse_event_reports_completion(protocol, event);
        signals.terminal |= sse_event_is_terminal(protocol, event);
        if let Some(data) = sse_event_data(event) {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&data) {
                if let Some(trace) = trace.as_deref_mut() {
                    trace.observe(&value);
                }
                if protocol == ProxyProtocol::OpenAiResponses {
                    if let Some(id) = value
                        .pointer("/response/id")
                        .and_then(serde_json::Value::as_str)
                        .and_then(normalize_session_affinity_key)
                    {
                        signals.response_id = Some(id);
                    }
                }
                usage.observe(protocol, &value);
            }
        }
        consumed = end;
    }
    if consumed > 0 {
        buffer.drain(..consumed);
    }
    const USAGE_EVENT_BUFFER_LIMIT: usize = 1024 * 1024;
    if buffer.len() > USAGE_EVENT_BUFFER_LIMIT {
        if let Some(trace) = trace {
            trace.limited();
        }
        buffer.clear();
    }
    signals
}
