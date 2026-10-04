use super::*;
const BASE: &str = "https://cli-chat-proxy.grok.com/v1";
fn cli_home() -> Result<std::path::PathBuf> {
    Ok(std::env::var_os("GROK_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or(home()?.join(".grok")))
}
fn executable() -> Result<std::path::PathBuf> {
    let name = if cfg!(windows) { "grok.exe" } else { "grok" };
    let mut dirs = vec![cli_home()?.join("bin"), home()?.join(".grok/bin")];
    if let Some(p) = std::env::var_os("GROK_BIN_DIR") {
        dirs.insert(0, p.into());
    }
    for dir in dirs {
        let p = dir.join(name);
        if p.is_absolute() && p.is_file() {
            return Ok(p);
        }
    }
    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        let p = dir.join(name);
        if std::fs::canonicalize(&p)
            .is_ok_and(|p| p.to_string_lossy().replace('\\', "/").contains("/.grok/"))
        {
            return Ok(p);
        }
    }
    Err("Install Grok Build and sign in before connecting this account".into())
}
fn credential(bundle: &Value) -> Result<Value> {
    let item = bundle
        .as_object()
        .into_iter()
        .flatten()
        .find_map(|(_, v)| (!s(v, "key").is_empty()).then_some(v))
        .ok_or("Grok Build is not signed in")?;
    let expiry = item["expires_at"]
        .as_str()
        .and_then(|s| {
            time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
        })
        .map(|t| t.unix_timestamp().max(0) as u64 * 1000)
        .unwrap_or(0);
    Ok(
        json!({"type":"oauth","refresh":"native-owned","access":item["key"],"expires":expiry,"accountId":item["email"],"aipassNativeBundle":bundle.to_string()}),
    )
}
pub(super) async fn login(c: &mut Context, method: usize) -> Result<Value> {
    if method == 1 {
        return credential(&read_json(&cli_home()?.join("auth.json"))?);
    }
    let temp = tempfile::Builder::new()
        .prefix("aipass-grok-auth-")
        .tempdir()
        .map_err(|_| "cannot create private Grok workspace")?;
    let mut cmd = native_cli::command(&executable()?, c);
    cmd.args(["login", "--device-auth"])
        .env("GROK_HOME", temp.path())
        .current_dir(temp.path());
    native_cli::login(cmd, c).await?;
    credential(&read_json(&temp.path().join("auth.json"))?)
}
pub(super) async fn fresh(c: &mut Context) -> Result<()> {
    if c.auth["expires"].as_u64().unwrap_or(0) > now() + 300000 {
        return Ok(());
    }
    let _refresh = c.refresh_guard()?;
    let bundle = s(&c.auth, "aipassNativeBundle");
    if bundle.is_empty() {
        return Err("Reconnect Grok to import its account-owned refresh grant".into());
    }
    let temp = tempfile::Builder::new()
        .prefix("aipass-grok-auth-")
        .tempdir()
        .map_err(|_| "cannot create private Grok workspace")?;
    let path = temp.path().join("auth.json");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    options
        .open(&path)
        .and_then(|mut f| f.write_all(bundle.as_bytes()))
        .map_err(|_| "cannot stage Grok credentials")?;
    let mut cmd = native_cli::command(&executable()?, c);
    cmd.arg("models")
        .env("GROK_HOME", temp.path())
        .current_dir(temp.path());
    native_cli::run(cmd, 30).await?;
    let next = credential(&read_json(&path)?)?;
    if s(&next, "accountId").is_empty() || next["accountId"] != c.auth["accountId"] {
        return Err("Grok account ownership changed".into());
    }
    if next["expires"]
        .as_u64()
        .is_some_and(|at| at > 0 && at <= now())
    {
        return Err("Grok Build token remains expired; reconnect the account".into());
    }
    c.save(next).await
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
