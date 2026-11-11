use super::*;

pub(super) struct Prepared {
    pub(super) request: reqwest::RequestBuilder,
    pub(super) grok: Option<aipass_proxy_conversion::providers::grok::Adapter>,
}
mod grok;

/// Send the prepared request and turn the upstream response into the client
/// response: classify the status, stream or buffer the body, convert and
/// track usage. Retry dispositions carry whether generation may already have
/// been submitted upstream.
pub(super) async fn run_http_attempt(ctx: &AttemptContext<'_>) -> AttemptDisposition {
    if matches!(
        ctx.target.upstream_kind,
        UpstreamKind::ClaudeSubscription | UpstreamKind::CommunitySubscription
    ) {
        let backend = ctx
            .state
            .subscription_backend
            .read()
            .ok()
            .and_then(|b| b.clone());
        let Some(backend) = backend else {
            return ctx.fail(None, None, false, "subscription bridge is unavailable");
        };
        let Some(payload) = ctx.request_json.cloned() else {
            return AttemptDisposition::Abort(error_response(
                StatusCode::BAD_REQUEST,
                "subscription bridge requires JSON",
            ));
        };
        let mut payload = if ctx.target.upstream_kind == UpstreamKind::CommunitySubscription {
            payload
        } else {
            match BuiltinConversionPlugin.convert_request(
                ctx.route.config.inbound_protocol,
                if ctx.target.upstream_kind == UpstreamKind::ClaudeSubscription {
                    ProxyProtocol::AnthropicMessages
                } else {
                    ProxyProtocol::OpenAiChatCompletions
                },
                payload,
            ) {
                Ok(value) => value,
                Err(error) => return ctx.conversion_error(error),
            }
        };
        if let Some(model) = &ctx.target.model_override {
            payload["model"] = serde_json::json!(model);
        }
        if ctx.target.upstream_kind == UpstreamKind::ClaudeSubscription {
            let Some(object) = payload.as_object_mut() else {
                return AttemptDisposition::Abort(error_response(
                    StatusCode::BAD_REQUEST,
                    "Claude requires a JSON object",
                ));
            };
            object.remove("_aipass_client_cache");
            object.remove("_aipass_claude_session");
            if let Some(options) = ctx.incoming_headers.get("x-aipass-claude-session") {
                let options = serde_json::from_slice::<serde_json::Value>(options.as_bytes()).ok();
                let Some(options) = options.filter(serde_json::Value::is_object) else {
                    return AttemptDisposition::Abort(error_response(
                        StatusCode::BAD_REQUEST,
                        "x-aipass-claude-session must be a JSON object",
                    ));
                };
                object.insert("_aipass_claude_session".into(), options);
            }
            if ctx
                .incoming_headers
                .get("x-aipass-cache-marks")
                .is_some_and(|v| v == "client")
            {
                if ctx.route.config.inbound_protocol != ProxyProtocol::AnthropicMessages {
                    return AttemptDisposition::Abort(error_response(
                        StatusCode::BAD_REQUEST,
                        "client cache marks require the Messages protocol",
                    ));
                }
                payload["_aipass_client_cache"] = serde_json::json!(true);
            }
        }
        let mut target = ctx.target.clone();
        if target.upstream_proxy.is_none() {
            target.upstream_proxy = ctx
                .state
                .config
                .read()
                .ok()
                .map(|c| c.upstream_proxy.clone());
        }
        return match backend
            .request_protocol(
                target,
                payload,
                ctx.route.config.inbound_protocol,
                ctx.session_key.map(str::to_owned),
            )
            .await
        {
            Ok(response) => deliver_upstream_response(ctx, response).await,
            // A child may already have started generation. Never retry an
            // ambiguous CLI failure on another billable account.
            Err(error) => ctx.fail(None, None, true, error),
        };
    }
    let upstream = match prepare_upstream_request(ctx).await {
        AttemptDisposition::Dispatch(upstream) => upstream,
        disposition => return disposition,
    };
    let response = match upstream.request.send().await {
        Ok(response) => response,
        // A connect failure proves no generation was submitted. A lost
        // response or partial write does not: never replay it.
        Err(err) => return ctx.fail(None, None, !err.is_connect(), err.to_string()),
    };
    let response = if let Some(adapter) = upstream.grok {
        grok::normalize(response.into(), adapter)
    } else {
        response.into()
    };
    deliver_upstream_response(ctx, response).await
}
