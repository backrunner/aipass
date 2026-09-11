use super::*;

pub(crate) async fn handle_models_request(
    request: Request<Incoming>,
    state: RuntimeState,
) -> Response<BoxBody> {
    if request.method() != http::Method::GET {
        return error_response(
            StatusCode::METHOD_NOT_ALLOWED,
            "model discovery requires GET",
        );
    }
    let request_id = request
        .extensions()
        .get::<Uuid>()
        .copied()
        .unwrap_or_else(Uuid::new_v4);
    let incoming_headers = request.headers().clone();
    let request_query = request.uri().query().map(str::to_owned);
    let session_key = session_affinity_key(&incoming_headers, None);
    let (bearer_token, api_key_token) = local_proxy_tokens(&incoming_headers);
    if bearer_token.is_none() && api_key_token.is_none() {
        return error_response(StatusCode::UNAUTHORIZED, "missing local proxy token");
    }
    let Some((mut route, _)) = select_route(&state, bearer_token, api_key_token, None) else {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "invalid local proxy token or route",
        );
    };
    route.local_token.zeroize();

    let mut last_error = None;
    let mut saw_not_found = false;
    let mut saw_other_failure = false;
    let mut capacity_skipped = false;
    for _round in 0..silent_retry_rounds(&route.config.retry) {
        let targets = ordered_route_targets(&state, &route, session_key.as_deref());
        let mut attempts = 0;
        for target in targets {
            if attempts >= route.config.retry.max_attempts.max(1) {
                break;
            }
            let Some(_recovery) = RecoveryPermit::acquire(&state, target.config.id) else {
                continue;
            };
            let Some(_permit) = ProviderPermit::acquire(&state, &target) else {
                capacity_skipped = true;
                continue;
            };
            attempts += 1;
            let client = match upstream_client(&state, route.config.retry.connect_timeout_ms) {
                Ok(client) => client,
                Err(err) => {
                    last_error = Some(err);
                    saw_other_failure = true;
                    continue;
                }
            };
            let upstream_path = if target.config.auth_scheme == "azure_api_key" {
                "/models"
            } else {
                "/v1/models"
            };
            let url = match upstream_url_with_query(
                &target.config.base_url,
                upstream_path,
                request_query.as_deref(),
            ) {
                Ok(url) => url,
                Err(err) => {
                    last_error = Some(err.to_string());
                    saw_other_failure = true;
                    mark_failure(&state, target.config.id, &route.config.retry);
                    continue;
                }
            };
            let headers = match build_upstream_headers(
                &incoming_headers,
                &target,
                target
                    .config
                    .effective_protocol(route.config.upstream_protocol),
            ) {
                Ok(headers) => headers,
                Err(err) => {
                    last_error = Some(err);
                    saw_other_failure = true;
                    mark_failure(&state, target.config.id, &route.config.retry);
                    continue;
                }
            };
            let response = match client.get(url).headers(headers).send().await {
                Ok(response) => response,
                Err(err) => {
                    last_error = Some(err.to_string());
                    saw_other_failure = true;
                    mark_failure(&state, target.config.id, &route.config.retry);
                    continue;
                }
            };
            let status = response.status();
            if !status.is_success() {
                let detail = diagnostics::upstream::read_error(
                    response,
                    &target,
                    &local_token_redactions(&incoming_headers),
                )
                .await;
                state.usage.log_upstream_error(
                    request_id,
                    route.config.id,
                    &target.config,
                    status,
                    "http_models",
                    &detail,
                );
                last_error = Some(format!("upstream returned {status}"));
                if status == StatusCode::NOT_FOUND {
                    saw_not_found = true;
                } else {
                    saw_other_failure = true;
                }
                if status_affects_circuit(status) {
                    mark_failure(&state, target.config.id, &route.config.retry);
                }
                continue;
            }
            let response_headers = response.headers().clone();
            let mut source: UpstreamBodyStream = Box::pin(response.bytes_stream());
            let payload = match collect_upstream_body(None, &mut source).await {
                Ok(payload) => payload,
                Err(err) => {
                    last_error = Some(err);
                    saw_other_failure = true;
                    mark_failure(&state, target.config.id, &route.config.retry);
                    continue;
                }
            };
            if payload.is_empty() || is_upstream_error_payload(&payload) {
                last_error = Some("upstream returned an empty or error model list".into());
                saw_other_failure = true;
                mark_failure(&state, target.config.id, &route.config.retry);
                continue;
            }
            let payload = enrich_models_payload(payload, route.config.inbound_protocol);
            let body = BodyExt::boxed_unsync(
                Full::new(payload).map_err(|never| -> BoxError { match never {} }),
            );
            let mut builder = Response::builder().status(status);
            let response_hop_headers = connection_header_names(&response_headers);
            for (name, value) in response_headers.iter() {
                if !is_hop_header(name)
                    && !response_hop_headers.contains(name)
                    && name != header::CONTENT_LENGTH
                    && name != header::CONTENT_ENCODING
                {
                    builder = builder.header(name, value);
                }
            }
            return builder.body(body).unwrap_or_else(|_| {
                error_response(
                    StatusCode::BAD_GATEWAY,
                    "failed to build model list response",
                )
            });
        }
    }

    if capacity_skipped && !saw_not_found && !saw_other_failure {
        return capacity_response();
    }
    // Model discovery is optional and several upstreams (notably Anthropic)
    // legitimately do not expose a /v1/models endpoint. Keep that client
    // response visible without treating it as a proxy health failure.
    if saw_not_found && !saw_other_failure {
        return error_response(
            StatusCode::NOT_FOUND,
            "upstream model discovery endpoint not found",
        );
    }

    set_error(
        &state,
        last_error.unwrap_or_else(|| "all model discovery targets failed".into()),
    );
    error_response(
        StatusCode::BAD_GATEWAY,
        "all model discovery targets failed",
    )
}

pub(crate) fn enrich_models_payload(payload: Bytes, protocol: ProxyProtocol) -> Bytes {
    let Ok(mut root) = serde_json::from_slice::<serde_json::Value>(&payload) else {
        return payload;
    };
    let Some(models) = root
        .get_mut("data")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return payload;
    };
    let api_type = match protocol {
        ProxyProtocol::OpenAiResponses => "responses",
        ProxyProtocol::OpenAiChatCompletions => "chat_completions",
        ProxyProtocol::AnthropicMessages => "anthropic_messages",
    };
    for model in models {
        let Some(model) = model.as_object_mut() else {
            continue;
        };
        model
            .entry("api_types")
            .or_insert_with(|| serde_json::json!([api_type]));
        model.entry("capabilities").or_insert_with(|| {
            serde_json::json!({
                "output_modalities": ["text"],
                "supports_tool_use": true
            })
        });
    }
    serde_json::to_vec(&root).map_or(payload, Bytes::from)
}

pub(crate) async fn handle_local_health_request(
    request: Request<Incoming>,
    state: &RuntimeState,
) -> Response<BoxBody> {
    if !matches!(request.method(), &http::Method::GET | &http::Method::HEAD) {
        return error_response(StatusCode::METHOD_NOT_ALLOWED, "health check requires GET");
    }
    let (enabled, active_routes) = state
        .config
        .read()
        .map(|config| {
            (
                config.enabled,
                config
                    .routes
                    .iter()
                    .filter(|route| route.config.enabled)
                    .count(),
            )
        })
        .unwrap_or((false, 0));
    let (requests, failures) = state
        .stats
        .lock()
        .map(|stats| (stats.requests, stats.failures))
        .unwrap_or((0, 0));
    let body = serde_json::json!({
        "status": "ok",
        "service": "aipass-proxy",
        "enabled": enabled,
        "activeRoutes": active_routes,
        "requests": requests,
        "failures": failures,
    });
    let body = if request.method() == http::Method::HEAD {
        Bytes::new()
    } else {
        Bytes::from(body.to_string())
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(BodyExt::boxed_unsync(
            Full::new(body).map_err(|never| -> BoxError { match never {} }),
        ))
        .unwrap_or_else(|_| {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "health response failed")
        })
}
