use super::*;

/// Build the outbound request for one attempt: pick the client, run request
/// conversion, and assemble URL, headers and body. Any disposition other
/// than `Dispatch` replaces the send.
pub(super) async fn prepare_upstream_request(ctx: &AttemptContext<'_>) -> AttemptDisposition {
    let state = ctx.state;
    let route = ctx.route;
    let target = ctx.target;
    let client = match target_client(state, target, route.config.retry.connect_timeout_ms) {
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
    let mut rewritten_payload = if (target.profile != ProviderProfile::Generic
        || target.upstream_kind != UpstreamKind::Standard
        || target.model_override.is_some())
        && ctx.body.bytes().is_none()
    {
        match ctx.request_json {
            Some(value) => Some(Bytes::from(value.to_string())),
            None => {
                return AttemptDisposition::Abort(error_response(
                    StatusCode::BAD_REQUEST,
                    "provider adaptation requires a JSON request",
                ))
            }
        }
    } else {
        None
    };
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
        } else if conversion || ctx.body.bytes().is_none() {
            rewritten_payload = Some(payload);
        }
    }
    if let Some(model) = &target.model_override {
        if let Some(source) = rewritten_payload
            .take()
            .or_else(|| ctx.body.bytes().cloned())
        {
            let mut body: serde_json::Value = match serde_json::from_slice(&source) {
                Ok(body) => body,
                Err(_) => {
                    return AttemptDisposition::Abort(error_response(
                        StatusCode::BAD_REQUEST,
                        "model adaptation requires JSON",
                    ))
                }
            };
            body["model"] = serde_json::json!(model);
            rewritten_payload = Some(Bytes::from(body.to_string()));
        }
    }
    if target.profile != ProviderProfile::Generic {
        if let Some(source) = rewritten_payload
            .take()
            .or_else(|| ctx.body.bytes().cloned())
        {
            rewritten_payload = Some(
                match provider::prepare(target.profile, target_protocol, source) {
                    Ok(body) => body,
                    Err(error) => {
                        return AttemptDisposition::Abort(error_response(
                            StatusCode::BAD_REQUEST,
                            &error,
                        ))
                    }
                },
            );
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
    let mut url =
        match upstream_url_with_query(&target.config.base_url, upstream_path, ctx.request_query) {
            Ok(url) => url,
            Err(err) => return ctx.fail(None, None, false, err.to_string()),
        };
    let mut upstream_headers =
        match build_upstream_headers(ctx.incoming_headers, target, target_protocol) {
            Ok(headers) => headers,
            Err(err) => return ctx.fail(None, None, false, err),
        };
    if target.upstream_kind == UpstreamKind::CodexSubscription {
        // Use the same normalized body for HTTP and adapted downstream WS.
        let Some(source) = rewritten_payload
            .take()
            .or_else(|| ctx.body.bytes().cloned())
        else {
            return AttemptDisposition::Abort(error_response(
                StatusCode::BAD_REQUEST,
                "Codex subscription requires a buffered JSON request",
            ));
        };
        let body = match codex::prepare(source) {
            Ok(body) => body,
            Err(error) => {
                return AttemptDisposition::Abort(error_response(StatusCode::BAD_REQUEST, &error))
            }
        };
        codex::headers(&mut upstream_headers, &body);
        rewritten_payload = Some(body);
    }
    if target.upstream_kind == UpstreamKind::GeminiNative {
        let Some(source) = rewritten_payload
            .take()
            .or_else(|| ctx.body.bytes().cloned())
        else {
            return AttemptDisposition::Abort(error_response(
                StatusCode::BAD_REQUEST,
                "Gemini requires a buffered JSON request",
            ));
        };
        let prepared = state
            .gemini_signatures
            .lock()
            .map_err(|_| "Gemini session unavailable".to_string())
            .and_then(|ledger| {
                gemini::prepare(&target.config.base_url, source, target.config.id, &ledger)
            });
        match prepared {
            Ok((endpoint, body)) => {
                url = endpoint;
                rewritten_payload = Some(body);
            }
            Err(error) => {
                return AttemptDisposition::Abort(error_response(StatusCode::BAD_REQUEST, &error))
            }
        }
    }
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
