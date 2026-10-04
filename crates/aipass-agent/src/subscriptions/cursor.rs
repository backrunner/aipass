use super::*;
use aipass_proxy_conversion::providers::cursor as codec;
use sha2::{Digest, Sha256};
const API: &str = "https://api2.cursor.sh";
const AGENT: &str = "https://agentn.global.api5.cursor.sh";
const VERSION: &str = "2026.09.23-86fc751";

fn executable() -> Result<std::path::PathBuf> {
    let mut dirs =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect::<Vec<_>>();
    dirs.extend([
        home()?.join(".local/bin"),
        "/opt/homebrew/bin".into(),
        "/usr/local/bin".into(),
    ]);
    for name in ["cursor-agent", "agent"] {
        for dir in &dirs {
            let p = dir.join(if cfg!(windows) {
                format!("{name}.exe")
            } else {
                name.into()
            });
            if p.is_absolute()
                && p.is_file()
                && (name == "cursor-agent"
                    || p.canonicalize()
                        .is_ok_and(|p| p.to_string_lossy().contains("cursor-agent")))
            {
                return Ok(p);
            }
        }
    }
    Err("Cursor CLI is not installed".into())
}
fn version() -> String {
    let re = regex::Regex::new(r"^\d{4}\.\d{2}\.\d{2}-[0-9a-f]+$").unwrap();
    let mut v = VERSION.to_owned();
    if let Ok(p) = executable().and_then(|p| {
        p.canonicalize()
            .map_err(|_| "cannot resolve Cursor CLI".into())
    }) {
        if let Some(name) = p
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .filter(|s| re.is_match(s))
        {
            if name > v.as_str() {
                v = name.into();
            }
        }
    }
    if let Ok(home) = home() {
        if let Ok(entries) = std::fs::read_dir(home.join(".local/share/cursor-agent/versions")) {
            for e in entries.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if re.is_match(&n) && n > v {
                    v = n;
                }
            }
        }
    }
    format!("cli-{v}")
}
fn headers(r: RequestBuilder, token: &str) -> RequestBuilder {
    r.bearer_auth(token)
        .header("Connect-Protocol-Version", "1")
        .header("x-cursor-client-version", version())
        .header("x-cursor-client-type", "cli")
        .header("x-ghost-mode", "true")
}
async fn unary(c: &Context, path: &str, token: &str) -> Result<Value> {
    json_request(headers(c.client.post(format!("{API}/{path}")), token).json(&json!({}))).await
}
async fn who(c: &Context, token: &str) -> Result<(String, String)> {
    let me = unary(c, "aiserver.v1.DashboardService/GetMe", token).await?;
    let email = s(&me, "email").trim().to_lowercase();
    if email.is_empty() {
        return Err("Cursor account identity missing".into());
    }
    let plan = unary(c, "aiserver.v1.DashboardService/GetPlanInfo", token)
        .await
        .unwrap_or(Value::Null);
    Ok((email, s(&plan["planInfo"], "planName").into()))
}
async fn native_token(_c: &Context) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = native_cli::command(std::path::Path::new("/usr/bin/security"), _c);
        cmd.args([
            "find-generic-password",
            "-s",
            "cursor-access-token",
            "-a",
            "cursor-user",
            "-w",
        ]);
        if let Ok(token) = native_cli::run(cmd, 10).await {
            let token = token.trim();
            if !token.is_empty() {
                return Ok(token.into());
            }
        }
    }
    let p = if cfg!(target_os = "macos") {
        home()?.join(".cursor/auth.json")
    } else if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or(home()?.join("AppData/Roaming"))
            .join("Cursor/auth.json")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or(home()?.join(".config"))
            .join("cursor/auth.json")
    };
    let v = read_json(&p)?;
    v["accessToken"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "Cursor CLI has no access token".into())
}
pub(super) async fn login(c: &mut Context, method: usize) -> Result<Value> {
    if method == 1 {
        let token = native_token(c).await?;
        let (email, plan) = who(c, &token).await?;
        return Ok(
            json!({"type":"oauth","access":token,"refresh":"cursor-agent","expires":expires(&token),"accountId":email,"plan":plan}),
        );
    }
    let verifier = format!("{}{}", hex_id(), hex_id());
    let state = uuid::Uuid::new_v4().to_string();
    let mut url = url::Url::parse("https://cursor.com/loginDeepControl").unwrap();
    url.query_pairs_mut()
        .append_pair(
            "challenge",
            &URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
        )
        .append_pair("uuid", &state)
        .append_pair("mode", "login")
        .append_pair("redirectTarget", "cli");
    c.challenge(url.as_str(), "Sign in to Cursor in your browser.", false)
        .await?;
    let mut errors = 0;
    for attempt in 0..150 {
        tokio::time::sleep(Duration::from_millis(
            (1000.0 * 1.2f64.powi(attempt)).min(10000.0) as u64,
        ))
        .await;
        let result = send(
            c.client
                .get(format!("{API}/auth/poll"))
                .query(&[("uuid", state.as_str()), ("verifier", verifier.as_str())])
                .header("x-cursor-client-version", version())
                .header("x-cursor-client-type", "cli")
                .timeout(Duration::from_secs(15)),
        )
        .await;
        let response = match result {
            Ok(r) => {
                errors = 0;
                r
            }
            Err(e) => {
                errors += 1;
                if errors >= 3 {
                    return Err(e);
                }
                continue;
            }
        };
        if response.status() == 404 {
            continue;
        }
        if !response.status().is_success() {
            return Err(format!(
                "Cursor sign-in returned HTTP {}",
                response.status().as_u16()
            ));
        }
        let v: Value = serde_json::from_slice(&read_bytes(response, LIMIT).await?)
            .map_err(|_| "invalid Cursor sign-in response")?;
        let token = s(&v, "accessToken");
        if token.is_empty() || v["refreshToken"].is_null() {
            continue;
        }
        let (email, plan) = who(c, token).await?;
        return Ok(
            json!({"type":"oauth","access":token,"refresh":v["refreshToken"],"expires":expires(token),"accountId":email,"plan":plan}),
        );
    }
    Err("Cursor sign-in timed out".into())
}
pub(super) async fn fresh(c: &mut Context) -> Result<()> {
    let native = c.auth["refresh"] == "cursor-agent";
    let api = c.auth["type"] == "api";
    if api && (s(&c.auth, "access").is_empty() || expires(s(&c.auth, "access")) < now() + 300000) {
        let v = json_request(
            c.client
                .post(format!("{API}/auth/exchange_user_api_key"))
                .bearer_auth(s(&c.auth, "key"))
                .json(&json!({})),
        )
        .await?;
        let token = s(&v, "accessToken");
        if token.is_empty() {
            return Err("Cursor returned no access token".into());
        }
        let (email, plan) = who(c, token).await?;
        let mut a = c.auth.clone();
        a["access"] = json!(token);
        a["accountId"] = json!(email);
        a["plan"] = json!(plan);
        a["expires"] = json!(expires(token));
        return c.save(a).await;
    }
    if native {
        let mut token = native_token(c).await?;
        // Never refresh a CLI account different from the one saved in the vault.
        let (email, plan) = if s(&c.auth, "access") == token && !s(&c.auth, "accountId").is_empty()
        {
            (
                s(&c.auth, "accountId").to_owned(),
                s(&c.auth, "plan").to_owned(),
            )
        } else {
            who(c, &token).await?
        };
        if s(&c.auth, "accountId") != email {
            return Err("Cursor CLI account changed; reconnect explicitly".into());
        }
        if expires(&token) > 0 && expires(&token) < now() + 300000 {
            let mut cmd = native_cli::command(&executable()?, c);
            cmd.arg("status");
            native_cli::run(cmd, 30).await?;
            token = native_token(c).await?;
            let (after, _) = who(c, &token).await?;
            if after != email {
                return Err("Cursor CLI account changed during refresh".into());
            }
        }
        if s(&c.auth, "access") != token {
            let mut a = c.auth.clone();
            a["access"] = json!(token);
            a["expires"] = json!(expires(&token));
            a["plan"] = json!(plan);
            c.save(a).await?;
        }
    }
    let expiry = expires(c.token()?);
    if expiry > 0 && expiry <= now() {
        return Err("Cursor session expired; sign in again".into());
    }
    Ok(())
}
pub(super) async fn models(c: &mut Context) -> Result<Value> {
    let v = unary(c, "agent.v1.AgentService/GetUsableModels", c.token()?).await?;
    let mut raw = vec![];
    let re = regex::Regex::new(r"\b(\d+)M\b").unwrap();
    for m in v["models"].as_array().into_iter().flatten() {
        let id = m["displayModelId"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(s(m, "modelId"));
        if id.is_empty() {
            continue;
        }
        let name = m["displayName"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(id)
            .replace('\u{200b}', "")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let name = name
            .trim_end_matches("(default)")
            .trim_end_matches("(current)")
            .trim();
        let context = re
            .captures(name)
            .and_then(|c| c[1].parse::<u64>().ok())
            .unwrap_or(0)
            * 1_000_000;
        raw.push(json!({"id":id,"name":name,"context":if context>0{context}else{200000},"run":m["modelId"].as_str().filter(|s|!s.is_empty()).unwrap_or(id)}));
    }
    if raw.is_empty() {
        return Err("Cursor returned no models".into());
    }
    let mut out = json!({});
    for r in &raw {
        let id = s(r, "id");
        let mut item = model(
            id,
            s(r, "name"),
            "chat",
            AGENT,
            r["context"].as_u64().unwrap_or(200000),
            0,
        );
        item["cursor_models"] = json!(raw);
        out[id] = item;
    }
    let families: std::collections::BTreeSet<_> = raw
        .iter()
        .map(|r| codec::split_model(s(r, "id")).0)
        .collect();
    for family in families {
        if !out[&family].is_null() {
            continue;
        }
        let vs = codec::variants(&raw, &family);
        if vs.len() < 2 {
            continue;
        }
        let default = vs
            .get("")
            .and_then(|id| raw.iter().find(|r| r["id"] == *id));
        let context = raw
            .iter()
            .filter(|r| codec::split_model(s(r, "id")).0 == family)
            .filter_map(|r| r["context"].as_u64())
            .min()
            .unwrap_or(200000);
        let mut item = model(
            &family,
            default.map(|r| s(r, "name")).unwrap_or(&family),
            "chat",
            AGENT,
            context,
            0,
        );
        item["variants"] = json!({});
        for e in vs.keys().filter(|s| !s.is_empty()) {
            item["variants"][e] = json!({"reasoningEffort":e});
        }
        item["cursor_models"] = json!(raw);
        out[family] = item;
    }
    Ok(out)
}
fn pool_base(id: &str) -> String {
    let mut id = id.to_lowercase();
    if let Some(s) = id.strip_prefix("cursor-") {
        id = s.into();
    }
    loop {
        let next = [
            "fast",
            "none",
            "low",
            "medium",
            "extra-high",
            "xhigh",
            "high",
            "max",
            "thinking",
        ]
        .into_iter()
        .find_map(|s| id.strip_suffix(&format!("-{s}")).map(str::to_owned));
        if let Some(s) = next {
            id = s;
        } else {
            break;
        }
    }
    id
}
pub(super) async fn usage(c: &mut Context) -> Result<Value> {
    let v = unary(
        c,
        "aiserver.v1.DashboardService/GetCurrentPeriodUsage",
        c.token()?,
    )
    .await?;
    let u = &v["planUsage"];
    if !u.is_object() {
        return Ok(json!({"plan":c.auth["plan"],"windows":[]}));
    }
    let bucket: std::collections::BTreeSet<_> = v["autoBucketModels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(pool_base)
        .collect();
    let mut candidates = vec!["auto".to_owned()];
    candidates.extend(
        c.models
            .as_object()
            .into_iter()
            .flat_map(|m| m.keys().cloned()),
    );
    candidates.extend(
        v["autoBucketModels"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned),
    );
    let pool: std::collections::BTreeSet<_> = candidates
        .into_iter()
        .filter(|id| {
            let p = pool_base(id);
            p == "auto"
                || p == "default"
                || p.starts_with("composer")
                || ["grok-4.5", "grok-4.6", "grok-4.7"].contains(&p.as_str())
                || bucket.contains(&p)
        })
        .collect();
    let ms = v["billingCycleEnd"]
        .as_str()
        .and_then(|s| s.parse::<i64>().ok())
        .or(v["billingCycleEnd"].as_i64());
    let reset = ms
        .and_then(|ms| time::OffsetDateTime::from_unix_timestamp(ms / 1000).ok())
        .and_then(|t| {
            t.format(&time::format_description::well_known::Rfc3339)
                .ok()
        });
    let mut windows = vec![];
    for (name, key, aside) in [
        ("Cursor Models", "autoPercentUsed", false),
        ("Other Models", "apiPercentUsed", false),
        ("Total", "totalPercentUsed", true),
    ] {
        if let Some(n) = u[key].as_f64() {
            let mut w = json!({"name":name,"used":n,"resetsAt":reset,"aside":aside});
            if name == "Cursor Models" {
                w["models"] = json!(pool);
            }
            if name == "Other Models" {
                w["notModels"] = json!(pool);
            }
            windows.push(w);
        }
    }
    Ok(json!({"plan":c.auth["plan"],"windows":windows}))
}
async fn agent_url(c: &Context) -> String {
    if let Ok(v) = unary(
        c,
        "aiserver.v1.ServerConfigService/GetServerConfig",
        c.token().unwrap_or(""),
    )
    .await
    {
        for key in ["agentUrl", "agentnUrl"] {
            if let Some(s) = v["agentUrlConfig"][key].as_str() {
                if let Ok(u) = url::Url::parse(s) {
                    if u.scheme() == "https"
                        && u.username().is_empty()
                        && u.password().is_none()
                        && u.host_str().is_some_and(|h| {
                            h.ends_with(".cursor.sh") || h.ends_with(".cursor.com")
                        })
                    {
                        return s.trim_end_matches('/').into();
                    }
                }
            }
        }
    }
    AGENT.into()
}
async fn error(c: &mut Context, status: u16, message: &str) -> Result<()> {
    c.headers(status, json!([["content-type", "application/json"]]))
        .await?;
    c.data(
        &serde_json::to_vec(&json!({"error":{"message":message,"type":"cursor_error"}}))
            .map_err(|_| "invalid Cursor error")?,
    )
    .await?;
    c.emit(json!({"type":"end"})).await
}
pub(super) async fn generate(c: &mut Context, m: &Value, body: Value) -> Result<()> {
    let raw = m["cursor_models"].as_array().cloned().unwrap_or_default();
    let id = codec::model_id(
        &raw,
        s(&body, "model"),
        s(&body, "reasoning_effort"),
        matches!(s(&body, "service_tier"), "priority" | "fast"),
    );
    let mut base = agent_url(c).await;
    for attempt in 0..2 {
        let run = codec::request(&body, &id, &c.session)?;
        let (tx, response) = open(c, &base, run.first.clone()).await?;
        let mut converter =
            codec::CursorStream::new(s(&body, "model"), &format!("chatcmpl-{}", hex_id()), run);
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let data = read_bytes(response, LIMIT).await?;
            let v =
                serde_json::from_slice(&data).unwrap_or(json!({"message":"Cursor request failed"}));
            let (status, message) = codec::failure(status, &v);
            if attempt == 0 && message.to_lowercase().contains("region") {
                let new = agent_url(c).await;
                if new != base {
                    base = new;
                    continue;
                }
            }
            return error(c, status, &message).await;
        }
        if response.version() != reqwest::Version::HTTP_2 {
            return Err("Cursor requires bidirectional HTTP/2".into());
        }
        let mut stream = response.bytes_stream();
        let mut heartbeat = tokio::time::interval(Duration::from_secs(5));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut sent = false;
        let mut count = 0usize;
        let mut retry = false;
        loop {
            let data = tokio::select! {_ = heartbeat.tick()=>{tx.try_send(Ok(codec::CursorStream::heartbeat())).map_err(|_|"Cursor request stalled")?;continue;},v=stream.next()=>v};
            let Some(data) = data else { break };
            let data = data.map_err(|_| "Cursor stream interrupted")?;
            count += data.len();
            if count > 128 * 1024 * 1024 {
                return Err("Cursor reply exceeds limit".into());
            }
            let batch = converter.push(&data)?;
            if let Some((status, message)) = batch.error {
                if sent || converter.has_output() {
                    return Err(format!("Cursor stream failed (HTTP {status})"));
                }
                if attempt == 0 && message.to_lowercase().contains("region") {
                    let new = agent_url(c).await;
                    if new != base {
                        base = new;
                        retry = true;
                        break;
                    }
                }
                return error(c, status, &message).await;
            }
            for msg in batch.outgoing {
                tokio::time::timeout(Duration::from_secs(30), tx.send(Ok(msg)))
                    .await
                    .map_err(|_| "Cursor upload stalled")?
                    .map_err(|_| "Cursor upload closed")?;
            }
            if !batch.chunks.is_empty() && !sent {
                c.headers(200, json!([["content-type", "text/event-stream"]]))
                    .await?;
                sent = true;
            }
            for chunk in batch.chunks {
                c.data(&chunk).await?;
            }
            if converter.done() {
                break;
            }
        }
        if retry {
            continue;
        }
        converter.finish()?;
        return c.emit(json!({"type":"end"})).await;
    }
    Err("Cursor regional endpoint unavailable".into())
}

type Upload = mpsc::Sender<std::result::Result<Vec<u8>, std::io::Error>>;
async fn open(c: &Context, base: &str, first: Vec<u8>) -> Result<(Upload, Response)> {
    let (tx, rx) = mpsc::channel(8);
    tx.send(Ok(first))
        .await
        .map_err(|_| "Cursor request cancelled")?;
    let input =
        futures_util::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|v| (v, rx)) });
    let response = send(
        headers(
            c.client.post(format!("{base}/agent.v1.AgentService/Run")),
            c.token()?,
        )
        .version(reqwest::Version::HTTP_2)
        .header("content-type", "application/connect+proto")
        .header("x-request-id", uuid::Uuid::new_v4().to_string())
        .header(
            "x-cursor-agent-allowed-tools",
            "mcp_tool_call,get_mcp_tools_tool_call",
        )
        .body(reqwest::Body::wrap_stream(input)),
    )
    .await?;
    Ok((tx, response))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http_body_util::{BodyExt, StreamBody};
    use hyper::{
        body::{Frame, Incoming},
        service::service_fn,
    };
    use hyper_util::rt::{TokioExecutor, TokioIo};
    #[tokio::test]
    async fn http2_upload_stays_open_while_downloading_and_sending_blob_answers() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            hyper::server::conn::http2::Builder::new(TokioExecutor::new())
                .serve_connection(
                    TokioIo::new(socket),
                    service_fn(|request: hyper::Request<Incoming>| async move {
                        assert_eq!(request.uri().path(), "/agent.v1.AgentService/Run");
                        assert_eq!(
                            request.headers()["x-cursor-agent-allowed-tools"],
                            "mcp_tool_call,get_mcp_tools_tool_call"
                        );
                        assert_eq!(request.headers()["x-ghost-mode"], "true");
                        let mut body = request.into_body();
                        let first = body.frame().await.unwrap().unwrap().into_data().unwrap();
                        assert_eq!(first, b"first"[..]);
                        let (tx, rx) =
                            mpsc::channel::<std::result::Result<Frame<Bytes>, std::io::Error>>(2);
                        tx.send(Ok(Frame::data(Bytes::from_static(b"need-blob"))))
                            .await
                            .unwrap();
                        tokio::spawn(async move {
                            let second = body.frame().await.unwrap().unwrap().into_data().unwrap();
                            assert_eq!(second, b"blob-answer"[..]);
                            tx.send(Ok(Frame::data(Bytes::from_static(b"complete"))))
                                .await
                                .unwrap();
                            drop(tx);
                            while body.frame().await.is_some() {}
                        });
                        let output = futures_util::stream::unfold(rx, |mut rx| async move {
                            rx.recv().await.map(|v| (v, rx))
                        });
                        Ok::<_, std::convert::Infallible>(hyper::Response::new(StreamBody::new(
                            output,
                        )))
                    }),
                )
                .await
                .unwrap();
        });
        let (c, _input, _output) = super::super::tests::context(
            Client::builder()
                .no_proxy()
                .http2_prior_knowledge()
                .build()
                .unwrap(),
        );
        tokio::time::timeout(Duration::from_secs(5), async {
            let (tx, response) = open(&c, &format!("http://{address}"), b"first".to_vec())
                .await
                .unwrap();
            assert_eq!(response.version(), reqwest::Version::HTTP_2);
            let mut response = response.bytes_stream();
            assert_eq!(response.next().await.unwrap().unwrap(), b"need-blob"[..]);
            tx.send(Ok(b"blob-answer".to_vec())).await.unwrap();
            assert_eq!(response.next().await.unwrap().unwrap(), b"complete"[..]);
            drop(tx);
            while let Some(rest) = response.next().await {
                assert!(rest.unwrap().is_empty());
            }
        })
        .await
        .unwrap();
        server.abort();
    }
}
