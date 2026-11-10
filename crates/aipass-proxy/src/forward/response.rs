use super::*;
mod sniff;

pub(super) async fn deliver_upstream_response(
    ctx: &AttemptContext<'_>,
    response: SubscriptionResponse,
) -> AttemptDisposition {
    let state = ctx.state;
    let route = ctx.route;
    let target = ctx.target;
    let status = response.status;
    if matches!(
        target.upstream_kind,
        UpstreamKind::ClaudeSubscription | UpstreamKind::CommunitySubscription
    ) && response
        .headers
        .get("x-aipass-target-incompatible")
        .is_some_and(|v| v == "1")
    {
        return AttemptDisposition::Skip("subscription target cannot preserve this request".into());
    }
    if matches!(
        target.upstream_kind,
        UpstreamKind::ClaudeSubscription | UpstreamKind::CommunitySubscription
    ) && response
        .headers
        .get("x-aipass-local-rejection")
        .is_some_and(|v| v == "1")
    {
        let body = BodyExt::boxed_unsync(StreamBody::new(
            response.body.map(|chunk| chunk.map(Frame::data)),
        ));
        return AttemptDisposition::Abort(
            Response::builder()
                .status(status)
                .header(header::CONTENT_TYPE, "application/json")
                .body(body)
                .unwrap(),
        );
    }
    if let Ok(mut health) = state.health.lock() {
        health.entry(target.config.id).or_default().last_status = Some(status.as_u16());
    }
    if is_retryable_status(status) {
        // Explicit rejection before generation; the request never ran.
        let detail = diagnostics::upstream::read_error_stream(
            response.body,
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
        if status == StatusCode::TOO_MANY_REQUESTS {
            mark_rate_limited(
                state,
                target.config.id,
                &response.headers,
                &route.config.retry,
            );
        } else if status_affects_circuit(status) {
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
    let mut response_headers = response.headers;
    let content_type = response_headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let target_protocol = if target.upstream_kind == UpstreamKind::CommunitySubscription {
        route.config.inbound_protocol
    } else {
        target
            .config
            .effective_protocol(route.config.upstream_protocol)
    };
    let conversion =
        route.config.conversion_enabled && route.config.inbound_protocol != target_protocol;
    let codex_response =
        status.is_success() && target.upstream_kind == UpstreamKind::CodexSubscription;
    let mut upstream_stream: UpstreamBodyStream = response.body;
    let streaming_response = ctx.streaming_request
        && (is_event_stream(&content_type)
            || codex_response
            || (status.is_success()
                && match sniff::event_stream(&response_headers, &mut upstream_stream).await {
                    Ok(streaming) => streaming,
                    Err(err) => return ctx.fail(Some(status), None, true, err.to_string()),
                }));
    if streaming_response && !is_event_stream(&content_type) {
        response_headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream"),
        );
    }
    // A silent retry must not commit a response before the upstream stream
    // has completed. Only an explicit upstream error can permit a retry; an
    // incomplete stream must never replay generation.
    let buffer_streaming = streaming_response && route.config.retry.silent_retry && !ctx.websocket;
    if target.upstream_kind == UpstreamKind::GeminiNative && streaming_response {
        upstream_stream = gemini::stream(
            upstream_stream,
            target.config.id,
            state.gemini_signatures.clone(),
        );
    }
    if streaming_response || is_event_stream(&content_type) {
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
        cache_creation_1h_tokens: 0,
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
        let mut buffered = match collect_upstream_body(first_chunk, &mut upstream_stream).await {
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
                !stream_reports_error(&buffered)
                    || stream_reports_output(target_protocol, &buffered)
                    || stream_reports_completion(target_protocol, &buffered),
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
                buffered_response_has_output(target_protocol, &buffered),
                "upstream returned an error payload",
            );
        }
        if status.is_success() && buffered.is_empty() {
            return ctx.fail(
                Some(status),
                first_token_ms,
                true,
                "upstream returned an empty response",
            );
        }
        if codex_response && !ctx.streaming_request {
            buffered = match codex::collect_response(&buffered) {
                Ok(payload) => payload,
                Err((confirmed, message)) => {
                    return ctx.fail(Some(status), first_token_ms, !confirmed, message)
                }
            };
        }
        if target.upstream_kind == UpstreamKind::GeminiNative && !streaming_response {
            let normalized = serde_json::from_slice(&buffered)
                .map_err(|_| "invalid Gemini response".to_string())
                .and_then(|value| {
                    state
                        .gemini_signatures
                        .lock()
                        .map_err(|_| "Gemini session unavailable".to_string())
                        .and_then(|mut ledger| {
                            gemini::response(value, target.config.id, &mut ledger, false, &mut 0)
                        })
                })
                .and_then(|value| serde_json::to_vec(&value).map_err(|e| e.to_string()));
            buffered = match normalized {
                Ok(value) => Bytes::from(value),
                Err(error) => return ctx.fail(Some(status), first_token_ms, true, error),
            };
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
                // A malformed successful reply is not an explicit rejection.
                // Generation (including tools) may already have run upstream.
                return ctx.fail(
                    Some(status),
                    first_token_ms,
                    true,
                    "protocol conversion failed for upstream response",
                );
            }
        }
        Box::pin(stream::once(async move { Ok::<Bytes, BoxError>(buffered) }))
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
    let tracking = UsageTrackingContext {
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
    };
    let output_stream: Pin<Box<dyn Stream<Item = Result<Bytes, BoxError>> + Send>> =
        if conversion && streaming_response {
            track_converted_usage_stream(body_stream, tracking, inbound_protocol)
        } else if let Some(converted) = converted_payload {
            // Track usage from the original provider payload only after
            // conversion has succeeded, then emit the converted body.
            Box::pin(
                track_usage_stream(body_stream, tracking)
                    .map(move |result| result.map(|_| converted.clone())),
            )
        } else {
            track_usage_stream(body_stream, tracking)
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
            || (conversion || target.upstream_kind != UpstreamKind::Standard)
                && name == header::CONTENT_ENCODING
            || codex_response && name == header::CONTENT_TYPE)
        {
            builder = builder.header(name, value);
        }
    }
    if codex_response {
        builder = builder.header(
            header::CONTENT_TYPE,
            if ctx.streaming_request {
                "text/event-stream"
            } else {
                "application/json"
            },
        );
    }
    let mut response = builder.body(stream_body).unwrap_or_else(|_| {
        error_response(StatusCode::BAD_GATEWAY, "failed to build proxy response")
    });
    response.extensions_mut().insert(UpstreamIdentity {
        target_id: target.config.id,
    });
    AttemptDisposition::Response(response)
}
