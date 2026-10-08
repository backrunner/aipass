use super::*;
const BASE: &str = "https://cli-chat-proxy.grok.com/v1";
pub(super) async fn login(c: &mut Context, method: usize) -> Result<Value> {
    cli_accounts::login(c, method).await
}
pub(super) async fn fresh(c: &mut Context) -> Result<()> {
    if s(&c.auth, "nativeHome").is_empty() {
        // One-time upgrade of existing complete grants into a CLI-owned home.
        let bundle = s(&c.auth, "aipassNativeBundle");
        if bundle.is_empty() {
            return Err("Reconnect Grok with the official Grok Build CLI".into());
        }
        let ambient = cli_accounts::default_home("grok")?;
        if let Ok(reference) = cli_accounts::reference("grok", &ambient) {
            if reference["accountId"] == c.auth["accountId"] {
                c.save(reference).await?;
                return cli_accounts::fresh(c).await;
            }
        }
        let path = cli_accounts::new_home("grok")?;
        crate::claude_bridge::write_private(&path.join("auth.json"), bundle.as_bytes())?;
        let reference = cli_accounts::reference("grok", &path)?;
        c.save(reference).await?;
    }
    cli_accounts::fresh(c).await
}
fn sign(c: &Context, r: RequestBuilder) -> Result<RequestBuilder> {
    Ok(r.bearer_auth(c.token()?)
        .header("x-grok-client-version", "1.0.41")
        .header("x-xai-token-auth", "xai-grok-cli")
        .header("x-grok-client-identifier", "grok-shell")
        .header("x-grok-client-mode", "headless")
        .header(
            "User-Agent",
            format!(
                "grok-shell/1.0.41 ({}; {})",
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
        ))
}
pub(super) async fn models(c: &mut Context) -> Result<Value> {
    let v = json_request(sign(c, c.client.get(format!("{BASE}/models")))?).await?;
    let mut out = json!({});
    for m in v["data"].as_array().into_iter().flatten() {
        let id = s(m, "id");
        if id.is_empty() || (!s(m, "api_backend").is_empty() && m["api_backend"] != "responses") {
            continue;
        }
        let mut item = model(
            id,
            m["name"].as_str().unwrap_or(id),
            "responses",
            BASE,
            m["context_window"].as_u64().unwrap_or(0),
            0,
        );
        item["variants"] = json!({});
        for e in m["reasoning_efforts"].as_array().into_iter().flatten() {
            let e = s(e, "value");
            if !e.is_empty() {
                item["variants"][e] = json!({"reasoningEffort":e});
            }
        }
        item["attachment"] = json!(true);
        out[id] = item;
    }
    Ok(out)
}
pub(super) async fn usage(c: &mut Context) -> Result<Value> {
    let v = json_request(
        c.client
            .get(format!("{BASE}/billing?format=credits"))
            .bearer_auth(c.token()?),
    )
    .await?;
    let cfg = &v["config"];
    let (name, span) =
        match s(&cfg["currentPeriod"], "type").trim_start_matches("USAGE_PERIOD_TYPE_") {
            "DAILY" => ("1 day", 86400),
            "WEEKLY" => ("7 days", 604800),
            "MONTHLY" => ("Month", 2592000),
            _ => ("Allowance", 0),
        };
    let reset = cfg["currentPeriod"]["end"]
        .as_str()
        .or(cfg["billingPeriodEnd"].as_str());
    let mut windows = vec![];
    if let Some(used) = cfg["creditUsagePercent"].as_f64() {
        windows.push(json!({"name":name,"span":span,"used":used,"resetsAt":reset}));
    }
    if let Some(cap) = cfg["onDemandCap"]["val"].as_f64().filter(|n| *n > 0.0) {
        windows.push(json!({"name":"On-demand","used":100.0*cfg["onDemandUsed"]["val"].as_f64().unwrap_or(0.0)/cap,"aside":true,"resetsAt":reset}));
    }
    Ok(json!({"windows":windows}))
}
pub(super) async fn generate(c: &mut Context, _: &Value, mut body: Value) -> Result<()> {
    let allowed = [
        "function",
        "web_search",
        "x_search",
        "image_generation",
        "collections_search",
        "file_search",
        "code_execution",
        "code_interpreter",
        "mcp",
        "shell",
        "tool_search",
    ];
    for t in body["tools"].as_array_mut().into_iter().flatten() {
        if !allowed.contains(&s(t, "type")) {
            return Err("Grok does not support this tool type; use a function tool".into());
        }
        if let Some(o) = t.as_object_mut() {
            o.remove("external_web_access");
        }
    }
    for item in body["input"].as_array_mut().into_iter().flatten() {
        if item["type"] == "reasoning" && item["content"].is_null() {
            if let Some(o) = item.as_object_mut() {
                o.remove("content");
            }
        }
    }
    let req = sign(c, c.client.post(format!("{BASE}/responses")))?
        .header("x-grok-model-override", s(&body, "model"))
        .header(
            "x-grok-conv-id",
            body["prompt_cache_key"].as_str().unwrap_or(&c.session),
        )
        .json(&body);
    c.pipe(send(req).await?).await
}
