use super::*;
use sha2::{Digest, Sha256};
const SERVER: &str = "https://server.codeium.com";
fn server(c: &Context) -> Result<String> {
    let s = c.auth["metadata"]["server"].as_str().unwrap_or(SERVER);
    let u = url::Url::parse(s).map_err(|_| "invalid Devin server")?;
    if u.scheme() != "https" || !u.username().is_empty() || u.password().is_some() {
        return Err("invalid Devin server".into());
    }
    Ok(s.trim_end_matches('/').into())
}
fn native(source: &Value) -> Result<Value> {
    let root = if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or(home()?.join("AppData/Roaming"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or(home()?.join(".local/share"))
    };
    let path = native_import::root(source)
        .unwrap_or_else(|| root.join("devin"))
        .join("credentials.toml");
    let raw = zeroize::Zeroizing::new(
        std::fs::read_to_string(path).map_err(|_| "Devin CLI is not signed in")?,
    );
    if raw.len() > LIMIT {
        return Err("Devin credentials exceed limit".into());
    }
    let v: toml::Value = toml::from_str(&raw).map_err(|_| "invalid Devin CLI credentials")?;
    let key = v
        .get("windsurf_api_key")
        .and_then(toml::Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or("Devin CLI has no session token")?;
    Ok(
        json!({"type":"api","key":key,"metadata":{"server":v.get("api_server_url").and_then(toml::Value::as_str).unwrap_or(SERVER),"cli":true}}),
    )
}
pub(super) async fn login(c: &mut Context, method: usize) -> Result<Value> {
    if method == 1 {
        return native(&c.auth);
    }
    let cb = loopback::Loopback::bind(&[0]).await?;
    let redirect = format!("http://127.0.0.1:{}/callback", cb.port);
    let verifier = format!("{}{}", hex_id(), hex_id());
    let state = format!("{}{}", hex_id(), hex_id());
    let mut url = url::Url::parse("https://app.devin.ai/auth/cli/continue").unwrap();
    url.query_pairs_mut()
        .append_pair("redirect_uri", &redirect)
        .append_pair("state", &state)
        .append_pair("prompt", "select_account")
        .append_pair(
            "code_challenge",
            &URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
        )
        .append_pair("code_challenge_method", "S256")
        .append_pair("cli_pkce_marker", "1");
    c.challenge(url.as_str(), "Sign in to Devin in the browser.", false)
        .await?;
    let p = cb
        .wait(&["https://app.devin.ai"], |r| {
            r.method == "GET" && r.path == "/callback" && r.fields["state"] == state
        })
        .await?
        .fields;
    let v = json_request(
        c.client
            .post(format!(
                "{SERVER}/exa.seat_management_pb.SeatManagementService/ExchangeDevinCLIPKCECode"
            ))
            .json(&json!({"code":p["code"],"code_verifier":verifier,"redirect_uri":redirect})),
    )
    .await?;
    let key = v["sessionToken"]
        .as_str()
        .or(v["session_token"].as_str())
        .filter(|s| !s.is_empty())
        .ok_or("Devin returned no session token")?;
    Ok(json!({"type":"api","key":key,"metadata":{"server":SERVER}}))
}
pub(super) async fn fresh(c: &mut Context) -> Result<()> {
    if let Ok(source) = serde_json::from_value::<aipass_agent_protocol::SubscriptionImportSource>(
        c.auth["nativeSource"].clone(),
    ) {
        let mut local = native(&c.auth)?;
        let changed = local["key"] != c.auth["key"];
        native_import::erase(&mut local);
        if changed {
            let secret = native_import::read(
                &source,
                c.outbound
                    .as_ref()
                    .unwrap_or(&UpstreamProxyConfig::default()),
            )
            .await?;
            c.save(serde_json::from_str(secret.expose()).map_err(|_| "invalid Devin account")?)
                .await?;
        }
    }
    Ok(())
}
fn executable() -> Result<std::path::PathBuf> {
    let exe = if cfg!(windows) { "devin.exe" } else { "devin" };
    let mut dirs =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect::<Vec<_>>();
    dirs.push(home()?.join(".local/bin"));
    dirs.extend(["/opt/homebrew/bin".into(), "/usr/local/bin".into()]);
    dirs.into_iter()
        .map(|p| p.join(exe))
        .find(|p| p.is_absolute() && p.is_file())
        .ok_or_else(|| "Devin CLI is not installed".into())
}
async fn families(c: &Context) -> Result<Value> {
    let exe = executable()?;
    let temp = tempfile::Builder::new()
        .prefix("aipass-devin-")
        .tempdir()
        .map_err(|_| "cannot create Devin workspace")?;
    let dir = temp.path().join("devin");
    std::fs::create_dir(&dir).map_err(|_| "cannot create Devin workspace")?;
    let value = toml::to_string(&std::collections::BTreeMap::from([
        ("windsurf_api_key", c.token()?.to_owned()),
        ("api_server_url", server(c)?),
        ("devin_webapp_host", "app.devin.ai".into()),
        ("devin_api_url", "https://api.devin.ai".into()),
    ]))
    .map_err(|_| "cannot encode Devin credentials")?;
    let mut file = std::fs::OpenOptions::new();
    file.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        file.mode(0o600);
    }
    use std::io::Write;
    file.open(dir.join("credentials.toml"))
        .and_then(|mut f| f.write_all(value.to_string().as_bytes()))
        .map_err(|_| "cannot stage Devin credentials")?;
    let mut cmd = native_cli::command(&exe, c);
    cmd.args(["models", "list", "--format", "json"])
        .env("XDG_DATA_HOME", temp.path())
        .env("APPDATA", temp.path())
        .current_dir(temp.path());
    let output = native_cli::run(cmd, 30).await?;
    let v: Value = serde_json::from_str(&output).map_err(|_| "invalid Devin model list")?;
    let families:Vec<_>=v["families"].as_array().into_iter().flatten().filter(|f|!matches!(s(f,"family_uid"),""|"adaptive"|"fusion")).map(|f|json!({"uid":f["family_uid"],"label":f["family_label"],"aliases":f["aliases"],"models":f["variants"].as_array().into_iter().flatten().map(|v|json!({"id":v["model_uid"],"name":v["label"],"context":v["max_context_tokens"],"output":v["max_output_tokens"]})).collect::<Vec<_>>()})).collect();
    if families.is_empty() {
        return Err("Devin returned no model families".into());
    }
    Ok(json!(families))
}
pub(super) async fn models(c: &mut Context) -> Result<Value> {
    let families = match families(c).await {
        Ok(v) => v,
        Err(_) => serde_json::from_str(include_str!("devin_families.json"))
            .map_err(|_| "invalid Devin model metadata")?,
    };
    let mut out = json!({});
    for f in families.as_array().into_iter().flatten() {
        let id = s(f, "uid");
        let variants = f["models"].as_array().cloned().unwrap_or_default();
        let context = variants
            .iter()
            .filter_map(|m| m["context"].as_u64())
            .max()
            .unwrap_or(0);
        let output = variants
            .iter()
            .filter_map(|m| m["output"].as_u64())
            .max()
            .unwrap_or(0);
        let mut ids = vec![(id.to_owned(), s(f, "label").to_owned())];
        for tier in ["fast", "priority"] {
            if variants
                .iter()
                .any(|m| s(m, "id").ends_with(&format!("-{tier}")))
            {
                ids.push((format!("{id}-{tier}"), format!("{} {tier}", s(f, "label"))));
            }
        }
        for m in &variants {
            ids.push((s(m, "id").into(), s(m, "name").into()));
        }
        for (id, name) in ids {
            if id.is_empty() {
                continue;
            }
            let mut item = model(&id, &name, "chat", SERVER, context, output);
            item["devin_families"] = json!([f]);
            out[&id] = item;
        }
    }
    Ok(out)
}
pub(super) async fn usage(c: &mut Context) -> Result<Value> {
    let v=json_request(c.client.post(format!("{}/exa.seat_management_pb.SeatManagementService/GetUserStatus",server(c)?)).header("Connect-Protocol-Version","1").json(&json!({"metadata":{"ideName":"devin-cli","ideVersion":"3000.11.3","extensionName":"devin-cli","extensionVersion":"3000.11.3","apiKey":c.token()?,"locale":"en","os":vendor_platform()}}))).await?;
    let st = &v["userStatus"]["planStatus"];
    let num = |v: &Value| {
        v.as_f64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    };
    let mut windows = vec![];
    for (name, span, left, reset, hide) in [
        (
            "1 day",
            86400,
            "dailyQuotaRemainingPercent",
            "dailyQuotaResetAtUnix",
            "hideDailyQuota",
        ),
        (
            "7 days",
            604800,
            "weeklyQuotaRemainingPercent",
            "weeklyQuotaResetAtUnix",
            "hideWeeklyQuota",
        ),
    ] {
        if st["planInfo"][hide] == true {
            continue;
        }
        if let Some(n) = num(&st[left]) {
            let at = num(&st[reset])
                .and_then(|n| time::OffsetDateTime::from_unix_timestamp(n as i64).ok())
                .and_then(|t| {
                    t.format(&time::format_description::well_known::Rfc3339)
                        .ok()
                });
            windows.push(
                json!({"name":name,"used":(100.0-n).clamp(0.0,100.0),"span":span,"resetsAt":at}),
            );
        }
    }
    if let Some(limit) = num(&st["acuLimit"]).filter(|n| *n > 0.0) {
        windows.push(json!({"name":"ACUs","used":100.0*num(&st["acuConsumed"]).unwrap_or(0.0)/limit,"aside":true,"resetsAt":st["planEnd"]}));
    }
    Ok(json!({"plan":st["planInfo"]["planName"],"until":st["planEnd"],"windows":windows}))
}
pub(super) async fn generate(c: &mut Context, m: &Value, body: Value) -> Result<()> {
    use aipass_proxy_conversion::providers::devin::{request, variant, DevinStream};
    let fallback: Value = serde_json::from_str(include_str!("devin_families.json"))
        .map_err(|_| "invalid Devin model metadata")?;
    let families = m.get("devin_families").unwrap_or(&fallback);
    let uid = variant(families, s(&body, "model"), s(&body, "reasoning_effort"));
    let req = request(&body, &uid, c.token()?, vendor_platform())?;
    let response = send(
        c.client
            .post(format!(
                "{}/exa.api_server_pb.ApiServerService/GetChatMessage",
                server(c)?
            ))
            .header("Content-Type", "application/connect+proto")
            .header("Connect-Protocol-Version", "1")
            .header("Authorization", format!("Basic {}", c.token()?))
            .body(req),
    )
    .await?;
    let mut converter = DevinStream::new(s(&body, "model"), &format!("chatcmpl-{}", hex_id()));
    c.converted(response, &mut converter).await
}
