use super::*;
fn base(c: &Context) -> &'static str {
    if c.provider == "workbuddy-ai" {
        "https://www.workbuddy.ai"
    } else {
        "https://copilot.tencent.com"
    }
}
fn domain(c: &Context) -> &str {
    c.auth["domain"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or(if c.provider == "workbuddy-ai" {
            "www.workbuddy.ai"
        } else {
            "copilot.tencent.com"
        })
}
fn sign(c: &Context, req: RequestBuilder) -> Result<RequestBuilder> {
    let mut r = req
        .bearer_auth(c.token()?)
        .header("X-User-Id", s(&c.auth, "uid"))
        .header("X-Domain", domain(c))
        .header("X-Product", "SaaS")
        .header("X-IDE-Type", "WorkBuddy")
        .header("User-Agent", "WorkBuddy/5.5.6");
    if c.provider == "workbuddy-ai" {
        r = r
            .header("X-Requested-With", "XMLHttpRequest")
            .header("X-Agent-Intent", "craft")
            .header("X-Agent-Type", "main")
            .header("X-IDE-Name", "WorkBuddy")
            .header("X-IDE-Version", "5.5.6")
            .header("X-Conversation-ID", &c.session)
            .header("X-Conversation-Request-ID", &c.session)
            .header("X-Conversation-Message-ID", hex_id())
            .header("X-Request-ID", hex_id());
    }
    Ok(r)
}
async fn data(req: RequestBuilder) -> Result<Value> {
    let v = json_request(req.header("User-Agent", "WorkBuddy/5.5.6")).await?;
    if v["code"].as_i64().unwrap_or(0) != 0 {
        return Err(format!(
            "WorkBuddy API refused the operation (code {})",
            v["code"]
        ));
    }
    Ok(v["data"].clone())
}
fn merge(mut a: Value, token: &Value) -> Result<Value> {
    if s(token, "accessToken").is_empty() {
        return Err("WorkBuddy returned no access token".into());
    }
    a["access"] = token["accessToken"].clone();
    for (from, to) in [
        ("refreshToken", "refresh"),
        ("domain", "domain"),
        ("tokenType", "tokenType"),
    ] {
        if !s(token, from).is_empty() {
            a[to] = token[from].clone();
        }
    }
    for (at, within, to) in [
        ("expiresAt", "expiresIn", "expires"),
        ("refreshExpiresAt", "refreshExpiresIn", "refreshExpiresAt"),
    ] {
        if let Some(n) = token[at].as_u64().filter(|n| *n > 0) {
            a[to] = json!(n);
        } else if let Some(n) = token[within].as_u64().filter(|n| *n > 0) {
            a[to] = json!(expires_after(&json!(n), 3600, 1000));
        }
    }
    Ok(a)
}
pub(super) async fn login(c: &mut Context, method: usize) -> Result<Value> {
    if method == 1 {
        let root = if cfg!(target_os = "macos") {
            "Library/Application Support/CodeBuddyExtension"
        } else if cfg!(windows) {
            "AppData/Local/CodeBuddyExtension"
        } else {
            ".local/share/CodeBuddyExtension"
        };
        let file =
            home()?
                .join(root)
                .join("Data/Public/auth")
                .join(if c.provider == "workbuddy-ai" {
                    "workbuddy-desktop-ai.info"
                } else {
                    "workbuddy-desktop.info"
                });
        let v = read_json(&file)?;
        let mut a = merge(json!({"type":"oauth"}), &v["auth"])?;
        a["uid"] = json!(identifier(&v["account"]["uid"]));
        a["accountId"] = a["uid"].clone();
        if s(&a, "uid").is_empty() {
            return Err("WorkBuddy native account identity missing".into());
        }
        return Ok(a);
    }
    let state = data(
        c.client
            .post(format!("{}/v2/plugin/auth/state", base(c)))
            .query(&[("platform", &c.provider)])
            .header("X-No-Authorization", "true")
            .header("X-No-User-Id", "true")
            .header("X-No-Enterprise-Id", "true")
            .header("X-No-Department-Info", "true")
            .json(&json!({})),
    )
    .await?;
    let mut url =
        url::Url::parse(s(&state, "authUrl")).map_err(|_| "WorkBuddy returned no sign-in URL")?;
    url.query_pairs_mut()
        .append_pair("version", "2.0.0")
        .append_pair("loginSessionId", &hex_id());
    c.challenge(url.as_str(), "Sign in to WorkBuddy in the browser.", false)
        .await?;
    let deadline = now() + 300000;
    let token = poll(
        c,
        "/v2/plugin/auth/token",
        s(&state, "state"),
        11217,
        deadline,
        false,
    )
    .await?;
    let mut a = merge(json!({"type":"oauth"}), &token)?;
    c.auth = a.clone();
    let who = poll(
        c,
        "/v2/plugin/login/account",
        s(&state, "state"),
        12151,
        deadline,
        true,
    )
    .await?;
    a["uid"] = json!(identifier(&who["uid"]));
    a["accountId"] = a["uid"].clone();
    if s(&a, "uid").is_empty() {
        return Err("WorkBuddy returned no account identity".into());
    }
    Ok(a)
}
async fn poll(
    c: &Context,
    path: &str,
    state: &str,
    retry: i64,
    deadline: u64,
    auth: bool,
) -> Result<Value> {
    while now() < deadline {
        let mut r = c
            .client
            .get(format!("{}{path}", base(c)))
            .query(&[("state", state)])
            .header("User-Agent", "WorkBuddy/5.5.6");
        if auth {
            r = r
                .bearer_auth(c.token()?)
                .header("X-Domain", domain(c))
                .header("X-No-User-Id", "true")
                .header("X-No-Enterprise-Id", "true");
        }
        let (status, v) = raw_json(r).await?;
        let code = v["code"].as_i64().unwrap_or(0);
        if status == 200 && code == 0 && v["data"].as_object().is_some_and(|m| !m.is_empty()) {
            return Ok(v["data"].clone());
        }
        if code != retry && code != 0 && ![408, 429].contains(&status) {
            return Err("WorkBuddy sign-in refused".into());
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Err("WorkBuddy sign-in timed out".into())
}
pub(super) async fn fresh(c: &mut Context) -> Result<()> {
    let expiry = c.auth["expires"].as_u64().unwrap_or(0);
    if expiry == 0 || expiry > now() + 60000 {
        return Ok(());
    }
    if s(&c.auth, "refresh").is_empty()
        || c.auth["refreshExpiresAt"]
            .as_u64()
            .is_some_and(|at| at > 0 && at <= now())
    {
        return if expiry > now() {
            Ok(())
        } else {
            Err("WorkBuddy refresh grant expired; reconnect the account".into())
        };
    }
    let _refresh = c.refresh_guard()?;
    let token = data(
        c.client
            .post(format!("{}/v2/plugin/auth/token/refresh", base(c)))
            .header("X-Refresh-Token", s(&c.auth, "refresh"))
            .header("X-Auth-Refresh-Source", "plugin")
            .header("X-Domain", domain(c))
            .json(&json!({})),
    )
    .await?;
    c.save(merge(c.auth.clone(), &token)?).await
}
pub(super) async fn models(c: &mut Context) -> Result<Value> {
    let v = json_request(
        sign(c, c.client.get(format!("{}/v3/config", base(c))))?
            .header("User-Agent", "CLI/2.0.0 WorkBuddy/5.5.6")
            .header("X-Requested-With", "XMLHttpRequest"),
    )
    .await?;
    if v["code"].as_i64().unwrap_or(0) != 0 {
        return Err("WorkBuddy model discovery refused".into());
    }
    let cfg = &v["data"];
    let mut out = json!({});
    for agent in cfg["agents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| a["name"] == "cli")
    {
        for id in agent["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let d = cfg["models"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|m| m["id"] == id)
                .cloned()
                .unwrap_or(Value::Null);
            let mut m = model(
                id,
                d["name"].as_str().unwrap_or(id),
                "chat",
                &format!("{}/v2", base(c)),
                d["maxInputTokens"].as_u64().unwrap_or(0),
                d["maxOutputTokens"].as_u64().unwrap_or(0),
            );
            m["attachment"] = json!(d["supportsImages"] == true);
            m["variants"] = json!({});
            if let Some(efforts) = d
                .pointer("/reasoning/supportedEfforts")
                .and_then(Value::as_array)
            {
                for e in efforts.iter().filter_map(Value::as_str) {
                    m["variants"][e] = json!({"reasoningEffort":e});
                }
                if !efforts.is_empty() {
                    m["reasoning"] = json!(true);
                    if d["onlyReasoning"] != true
                        && d.pointer("/reasoning/canDisableThinking") != Some(&json!(false))
                    {
                        m["variants"]["none"] = json!({"reasoningEffort":"none"});
                    }
                }
            }
            let credits = d["credits"].as_f64().or_else(|| {
                d["credits"]
                    .as_str()
                    .and_then(|s| s.trim().trim_start_matches('x').parse().ok())
            });
            m["free"] = json!(credits == Some(0.0));
            out[id] = m;
        }
    }
    Ok(out)
}
pub(super) async fn usage(c: &mut Context) -> Result<Value> {
    let sum = data(
        sign(
            c,
            c.client.post(format!(
                "{}/billing/meter/get-user-resource-summary",
                base(c)
            )),
        )?
        .json(&json!({})),
    )
    .await?;
    let number = |v: &Value| {
        v.as_f64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            .unwrap_or(0.0)
    };
    let (mut used, mut total) = (0.0, 0.0);
    for p in sum["Packages"].as_array().into_iter().flatten() {
        used += number(&p["CycleUsedCapacity"]);
        total += number(&p["CycleTotalCapacity"]);
    }
    Ok(
        json!({"plan":if sum["IsPaidUser"]==true {"Pro"} else {"Free"},"windows":if total>0.0 {vec![json!({"name":"Credits","used":100.0*used/total,"display":format!("{used:.2} / {total:.2}")})]} else {vec![]}}),
    )
}
pub(super) async fn generate(c: &mut Context, _: &Value, mut body: Value) -> Result<()> {
    aipass_proxy_conversion::providers::workbuddy_request(&mut body)?;
    let r = send(sign(c, c.client.post(format!("{}/v2/chat/completions", base(c))))?.json(&body))
        .await?;
    c.pipe(r).await
}
