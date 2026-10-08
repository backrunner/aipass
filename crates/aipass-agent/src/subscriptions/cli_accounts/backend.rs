//! Subscription HTTP contracts using immediate CLI access.
use super::*;

pub(crate) async fn generate(c: &mut Context, _: &Value, mut body: Value) -> Result<()> {
    let req = match c.provider.as_str() {
        "codex" => {
            body = aipass_proxy::prepare_codex_subscription(body)?;
            signed(c, c.client.post(format!("{CODEX}/responses")))?
                .header("session_id", &c.session)
                .header("Accept", "text/event-stream")
                .json(&body)
        }
        "copilot" => signed(
            c,
            c.client
                .post(format!("{}/chat/completions", copilot_api(c).await?)),
        )?
        .header("x-request-id", uuid::Uuid::new_v4().to_string())
        .header("x-initiator", "user")
        .json(&body),
        "gemini-cli" => {
            let model = s(&body, "model").to_owned();
            body.as_object_mut()
                .ok_or("invalid Gemini request")?
                .remove("model");
            signed(c, c.client.post(format!("{GEMINI}:streamGenerateContent?alt=sse")))?
                .json(&json!({"model":model,"project":project(c).await?,"user_prompt_id":c.session,"request":body}))
        }
        _ => return Err("unknown CLI subscription".into()),
    };
    c.pipe(send(req).await?).await
}

const CODEX: &str = "https://chatgpt.com/backend-api/codex";
const COPILOT: &str = "https://api.githubcopilot.com";
const GEMINI: &str = "https://cloudcode-pa.googleapis.com/v1internal";

pub(super) fn signed(c: &Context, r: RequestBuilder) -> Result<RequestBuilder> {
    let mut r = r.bearer_auth(c.token()?);
    if c.provider == "codex" {
        r = r
            .header("chatgpt-account-id", &c.native_workspace)
            .header("originator", "codex_cli_rs")
            .header("User-Agent", "codex_cli_rs/0.116.0");
    }
    if c.provider == "copilot" {
        r = r
            .header("copilot-integration-id", "copilot-developer-cli")
            .header("editor-version", "copilot/1.0.88")
            .header("user-agent", "copilot/1.0.88")
            .header("x-github-api-version", "2026-07-01");
    }
    Ok(r)
}

pub(crate) async fn models(c: &mut Context) -> Result<Value> {
    if c.provider == "gemini-cli" {
        let quota = google(c, "retrieveUserQuota", json!({"project":project(c).await?})).await?;
        let mut models = json!({});
        for b in quota["buckets"].as_array().into_iter().flatten() {
            let id = s(b, "modelId");
            if !id.is_empty() {
                models[id] = model(id, id, "gemini", GEMINI, 1000000, 65536);
            }
        }
        return Ok(models);
    }
    let url = if c.provider == "codex" {
        format!("{CODEX}/models?client_version=0.116.0")
    } else {
        format!("{}/models", copilot_api(c).await?)
    };
    let v = json_request(signed(c, c.client.get(url))?).await?;
    let mut out = json!({});
    let list = v["models"]
        .as_array()
        .or(v["data"].as_array())
        .ok_or("CLI backend returned no models")?;
    for m in list {
        let id = m["slug"].as_str().or(m["id"].as_str()).unwrap_or("");
        if id.is_empty() {
            continue;
        }
        out[id] = model(
            id,
            m["display_name"]
                .as_str()
                .or(m["name"].as_str())
                .unwrap_or(id),
            if c.provider == "codex" {
                "responses"
            } else {
                "chat"
            },
            if c.provider == "codex" {
                CODEX
            } else {
                COPILOT
            },
            m["context_window"].as_u64().unwrap_or(0),
            0,
        );
        if c.provider == "copilot" {
            if let Some(multiplier) = m
                .pointer("/billing/multiplier")
                .and_then(Value::as_f64)
                .filter(|n| n.is_finite() && *n >= 0.0)
            {
                out[id]["premiumMultiplier"] = json!(multiplier);
            }
        }
    }
    Ok(out)
}

pub(super) fn copilot_user_request(c: &Context) -> Result<RequestBuilder> {
    let request = signed(
        c,
        c.client.get("https://api.github.com/copilot_internal/user"),
    )?;
    // Replace, rather than append a second Authorization header.
    let mut headers = reqwest::header::HeaderMap::new();
    let mut authorization =
        reqwest::header::HeaderValue::from_str(&format!("token {}", c.token()?))
            .map_err(|_| "invalid CLI access token")?;
    authorization.set_sensitive(true);
    headers.insert(reqwest::header::AUTHORIZATION, authorization);
    Ok(request.headers(headers))
}

pub(super) async fn copilot_api(c: &Context) -> Result<String> {
    let v = json_request(copilot_user_request(c)?).await?;
    let api = v
        .pointer("/endpoints/api")
        .and_then(Value::as_str)
        .unwrap_or(COPILOT);
    let u = url::Url::parse(api).map_err(|_| "invalid Copilot endpoint")?;
    if u.scheme() != "https"
        || !u.username().is_empty()
        || u.password().is_some()
        || u.port().is_some_and(|p| p != 443)
        || !u
            .host_str()
            .is_some_and(|h| h == "githubcopilot.com" || h.ends_with(".githubcopilot.com"))
    {
        return Err("Copilot returned an untrusted endpoint".into());
    }
    Ok(api.trim_end_matches('/').to_owned())
}

pub(super) async fn google(c: &Context, method: &str, body: Value) -> Result<Value> {
    json_request(
        signed(c, c.client.post(format!("{GEMINI}:{method}")))?
            .header("User-Agent", "GeminiCLI/0.34.0")
            .json(&body),
    )
    .await
}
pub(super) async fn project(c: &Context) -> Result<String> {
    let v = google(c,"loadCodeAssist",json!({"metadata":{"ideType":"IDE_UNSPECIFIED","platform":"PLATFORM_UNSPECIFIED","pluginType":"GEMINI"}})).await?;
    let id = v["cloudaicompanionProject"].as_str().or(v
        .pointer("/cloudaicompanionProject/id")
        .and_then(Value::as_str));
    if let Some(id) = id.filter(|s| !s.is_empty()) {
        return Ok(id.into());
    }
    // Let the official CLI perform entitlement onboarding. Never invent a project.
    Err("Gemini Code Assist has no project yet; finish setup in Gemini CLI and reconnect".into())
}

pub(crate) async fn usage(c: &mut Context) -> Result<Value> {
    if c.provider == "gemini-cli" {
        let v = google(c, "retrieveUserQuota", json!({"project":project(c).await?})).await?;
        let windows = v["buckets"].as_array().into_iter().flatten().filter_map(|b|b["remainingFraction"].as_f64().map(|f|json!({"name":b["modelId"],"models":[b["modelId"]],"used":100.0*(1.0-f),"resetsAt":b["resetTime"]}))).collect::<Vec<_>>();
        if windows.is_empty() {
            return Err("Gemini returned no allowance data".into());
        }
        return Ok(json!({"windows":windows}));
    }
    if c.provider == "copilot" {
        let v = json_request(copilot_user_request(c)?).await?;
        return copilot_usage(&v, &c.models);
    }

    let path = root(&c.auth)?;
    let mut cmd = command(c, "codex", &path)?;
    cmd.arg("app-server");
    let v = rpc(cmd, c, "account/rateLimits/read", json!(null), false, false).await?;
    if v["accountId"]
        .as_str()
        .is_some_and(|id| id != c.native_workspace)
    {
        return Err("Codex quota belongs to a different workspace".into());
    }
    codex_usage(&v)
}
