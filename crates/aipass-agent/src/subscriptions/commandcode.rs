use super::*;
const API: &str = "https://api.commandcode.ai";
const BASE: &str = "https://api.commandcode.ai/provider/v1";
fn plan_info(id: &str) -> (&'static str, f64) {
    let id = id.to_lowercase().replace('_', "-");
    for (p, name, amount) in [
        ("individual-provider", "Provider", 15.0),
        ("individual-pro-v1", "Pro", 80.0),
        ("individual-goat", "GOAT", 70.0),
        ("individual-ultra", "Ultra", 300.0),
        ("individual-max", "Max", 150.0),
        ("individual-pro", "Pro", 30.0),
        ("individual-go", "Go", 10.0),
        ("teams-pro", "Teams Pro", 40.0),
    ] {
        if id.starts_with(p) {
            return (name, amount);
        }
    }
    ("", 0.0)
}
pub(super) async fn login(c: &mut Context, method: usize) -> Result<Value> {
    let a = if method == 1 {
        read_json(&home()?.join(".commandcode/auth.json"))?
    } else {
        let callback = loopback::Loopback::bind(&[0]).await?;
        let state = format!("{}{}", hex_id(), hex_id());
        let mut url = url::Url::parse("https://commandcode.ai/studio/auth/cli").unwrap();
        url.query_pairs_mut()
            .append_pair(
                "callback",
                &format!("http://127.0.0.1:{}/callback", callback.port),
            )
            .append_pair("state", &state)
            .append_pair("mode", "redirect");
        c.challenge(
            url.as_str(),
            "Approve an API key in Command Code Studio.",
            false,
        )
        .await?;
        callback
            .wait(
                &["https://commandcode.ai", "https://staging.commandcode.ai"],
                |cb| cb.method == "POST" && cb.path == "/callback" && cb.fields["state"] == state,
            )
            .await?
            .fields
    };
    if s(&a, "apiKey").is_empty() {
        return Err("Command Code returned no API key".into());
    }
    let me = json_request(
        c.client
            .get(format!("{API}/alpha/whoami"))
            .bearer_auth(s(&a, "apiKey")),
    )
    .await?;
    Ok(
        json!({"type":"api","key":a["apiKey"],"metadata":{"userId":me["user"]["id"].as_str().or(a["userId"].as_str()),"email":me["user"]["email"],"cli":method==1}}),
    )
}
pub(super) async fn fresh(c: &mut Context) -> Result<()> {
    if now().saturating_sub(c.auth["metadata"]["planAt"].as_u64().unwrap_or(0)) < 600000 {
        return Ok(());
    }
    let v = match json_request(
        c.client
            .get(format!("{API}/alpha/billing/subscriptions"))
            .bearer_auth(c.token()?),
    )
    .await
    {
        Ok(v) => v,
        Err(_) if c.auth["metadata"]["planAt"].as_u64().unwrap_or(0) > 0 => return Ok(()),
        Err(e) => return Err(e),
    };
    if v["success"] == false {
        return Err("Command Code could not read the subscription".into());
    }
    let d = &v["data"];
    let active =
        !s(d, "planId").is_empty() && !matches!(s(d, "status"), "canceled" | "incomplete_expired");
    let id = if active { s(d, "planId") } else { "" };
    let (name, _) = plan_info(id);
    let mut next = c.auth.clone();
    if !next["metadata"].is_object() {
        next["metadata"] = json!({});
    }
    next["metadata"]["planId"] = json!(id);
    next["metadata"]["plan"] = json!(if active {
        if name.is_empty() {
            id
        } else {
            name
        }
    } else {
        "No plan"
    });
    next["metadata"]["planAt"] = json!(now());
    c.save(next).await
}
fn go(c: &Context) -> bool {
    c.auth["metadata"]["plan"] == "Go"
}
pub(super) async fn models(c: &mut Context) -> Result<Value> {
    let mut req = c.client.get(format!("{BASE}/models"));
    if !go(c) {
        req = req.bearer_auth(c.token()?).header("x-api-key", c.token()?);
    }
    let v = json_request(req).await?;
    let mut out = json!({});
    let table: Value = serde_json::from_str(include_str!("commandcode_go.json"))
        .map_err(|_| "invalid Go model table")?;
    for m in v["data"]
        .as_array()
        .or(v["models"].as_array())
        .into_iter()
        .flatten()
    {
        let id = m["id"].as_str().or(m["name"].as_str()).unwrap_or("");
        if id.is_empty() {
            continue;
        }
        if go(c)
            && table["refused"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|v| v == id)
        {
            continue;
        }
        let lower = id.to_lowercase();
        if go(c)
            && [
                "image",
                "embedding",
                "whisper",
                "tts",
                "dall-e",
                "flux",
                "imagen",
                "speech",
            ]
            .iter()
            .any(|x| lower.contains(x))
        {
            continue;
        }
        let eps = m["supported_endpoints"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(|s| s.trim_end_matches('/').trim_start_matches("/v1"))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let wire = if go(c) || eps.contains(&"/chat/completions") {
            "chat"
        } else if eps.contains(&"/responses") {
            "responses"
        } else if eps.contains(&"/messages") || id.contains("claude") {
            "anthropic"
        } else {
            "chat"
        };
        let known = table["models"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|m| m["id"] == id);
        let mut item = model(
            id,
            m["display_name"]
                .as_str()
                .or(m["name"].as_str())
                .unwrap_or(id),
            wire,
            BASE,
            m["context_length"]
                .as_u64()
                .or_else(|| known.and_then(|k| k["context"].as_u64()))
                .unwrap_or(0),
            0,
        );
        item["variants"] = json!({});
        if let Some(k) = known {
            for e in k["efforts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                item["variants"][e] = json!({"reasoningEffort":e});
            }
        }
        out[id] = item;
    }
    Ok(out)
}
fn number(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}
pub(super) async fn usage(c: &mut Context) -> Result<Value> {
    let v = json_request(
        c.client
            .get(format!("{API}/alpha/billing/credits"))
            .bearer_auth(c.token()?),
    )
    .await?;
    let mut windows = vec![];
    for (name, span, key) in [("5 hours", 18000, "fiveHour"), ("Weekly", 604800, "weekly")] {
        let x = &v["windowLimits"][key];
        if let (Some(u), Some(cap)) = (number(&x["used"]), number(&x["cap"]).filter(|n| *n > 0.0)) {
            windows.push(json!({"name":name,"used":(100.0*u.max(0.0)/cap).min(100.0),"span":span,"resetsAt":x["resetAt"]}));
        }
    }
    let mut left = 0.0;
    let mut known = false;
    for k in ["monthlyCredits", "purchasedCredits", "freeCredits"] {
        if let Some(n) = number(&v["credits"][k]) {
            left += n.max(0.0);
            known = true;
        }
    }
    let (_, monthly) = plan_info(s(&c.auth["metadata"], "planId"));
    let mut out = json!({"plan":c.auth["metadata"]["plan"]});
    if known && monthly > 0.0 {
        let current = number(&v["credits"]["monthlyCredits"])
            .unwrap_or(0.0)
            .max(0.0);
        let pool = monthly.max(current) + left - current;
        windows.push(json!({"name":"Credits","used":100.0*(pool-left)/pool,"display":format!("${:.2} / ${pool:.2}",pool-left)}));
    } else if known && windows.is_empty() {
        out["balance"] = json!(format!("${left:.2}"));
    }
    out["windows"] = json!(windows);
    Ok(out)
}
pub(super) async fn generate(c: &mut Context, m: &Value, body: Value) -> Result<()> {
    if !go(c) {
        let mut req = c
            .client
            .post(format!("{BASE}{}", path(s(m, "wire"))))
            .bearer_auth(c.token()?)
            .header("x-api-key", c.token()?);
        if m["wire"] == "anthropic" {
            req = req.header("anthropic-version", "2023-06-01");
        }
        return c.pipe(send(req.json(&body)).await?).await;
    }
    use aipass_proxy_conversion::providers::commandcode::{generate_body, GoStream};
    let date = time::OffsetDateTime::now_utc().date().to_string();
    let mut adjusted = body.clone();
    let levels = m["variants"]
        .as_object()
        .map(|v| v.keys().map(String::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    if let Some(e) = body["reasoning_effort"].as_str() {
        if e == "none" {
            adjusted.as_object_mut().unwrap().remove("reasoning_effort");
        } else {
            adjusted["reasoning_effort"] = json!(
                aipass_proxy_conversion::providers::commandcode::fit_effort(e, &levels)
            );
        }
    }
    let req = generate_body(&adjusted, &date, vendor_platform())?;
    let response = send(
        c.client
            .post(format!("{API}/alpha/generate"))
            .bearer_auth(c.token()?)
            .header("User-Agent", "cli")
            .header("x-command-code-version", "1.72.2")
            .header("x-cli-environment", "production")
            .header("x-project-slug", "aipass")
            .header("x-taste-learning", "false")
            .header("x-session-id", &c.session)
            .json(&req),
    )
    .await?;
    let mut convert = GoStream::new(s(&body, "model"), &format!("chatcmpl-{}", hex_id()));
    c.converted(response, &mut convert).await
}
