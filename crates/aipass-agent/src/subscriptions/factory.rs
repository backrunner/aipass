use super::*;
const API: &str = "https://api.factory.ai";
const WORKOS: &str = "https://api.workos.com/user_management";
const CLIENT: &str = "client_01HNM792M5G5G1A2THWPXKFMXB";
fn base(auth: &Value) -> &'static str {
    if auth["region"] == "eu" {
        "https://api.eu.factory.ai"
    } else {
        API
    }
}
fn signed(c: &Context, request: RequestBuilder) -> Result<RequestBuilder> {
    let mut r = request
        .bearer_auth(c.token()?)
        .header("X-Factory-Client", "cli")
        .header("X-Client-Version", "0.231.0")
        .header("User-Agent", "factory-cli/0.231.0");
    if c.auth["type"] != "api" && !s(&c.auth, "activeOrganizationId").is_empty() {
        r = r.header("X-Factory-Org-Id", s(&c.auth, "activeOrganizationId"));
    }
    Ok(r)
}
struct RefreshFailure {
    message: String,
    refused: bool,
}
impl RefreshFailure {
    fn can_use(&self, expires: u64) -> bool {
        !self.refused && expires > now()
    }
}
async fn renew(c: &Context, auth: &Value, org: &str) -> std::result::Result<Value, RefreshFailure> {
    let mut form = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", s(auth, "refresh")),
        ("client_id", CLIENT),
    ];
    if !org.is_empty() {
        form.push(("organization_id", org));
    }
    let (status, t) = raw_json(c.client.post(format!("{WORKOS}/authenticate")).form(&form))
        .await
        .map_err(|message| RefreshFailure {
            message,
            refused: false,
        })?;
    if !(200..300).contains(&status) {
        return Err(RefreshFailure {
            message: format!("Factory token refresh returned HTTP {status}"),
            refused: (400..500).contains(&status) && status != 429,
        });
    }
    renewed_auth(auth, &t).map_err(|message| RefreshFailure {
        message,
        refused: false,
    })
}
fn renewed_auth(auth: &Value, t: &Value) -> Result<Value> {
    if s(t, "access_token").is_empty() {
        return Err("Factory sign-in expired; reconnect the account".into());
    }
    let mut next = auth.clone();
    next["access"] = t["access_token"].clone();
    next["expires"] = json!(expires(s(t, "access_token")));
    if !s(t, "refresh_token").is_empty() {
        next["refresh"] = t["refresh_token"].clone();
    }
    Ok(next)
}
async fn who(c: &Context, auth: &mut Value) -> Result<()> {
    let token = auth["access"]
        .as_str()
        .or(auth["key"].as_str())
        .ok_or("Factory credential missing")?;
    let req = || {
        c.client
            .get(format!("{}/api/cli/whoami", base(auth)))
            .bearer_auth(token)
            .header("X-Factory-Whoami-Extended", "true")
    };
    let mut r = req();
    if !s(auth, "activeOrganizationId").is_empty() {
        r = r.header("X-Factory-Org-Id", s(auth, "activeOrganizationId"));
    }
    let (status, mut v) = raw_json(r).await?;
    if status == 403 {
        v = json_request(req()).await?;
    } else if status != 200 {
        return Err(format!("Factory account lookup returned HTTP {status}"));
    }
    if auth["type"] != "api" && !s(&v, "orgId").is_empty() {
        auth["activeOrganizationId"] = v["orgId"].clone();
    }
    auth["region"] = v["region"].clone();
    auth["premBaseHost"] = v["premBaseHostV2"].clone();
    if auth["accountId"].is_null() {
        auth["accountId"] = v["email"].clone();
    }
    auth["whoAt"] = json!(now());
    Ok(())
}
pub(super) async fn login(c: &mut Context, _: usize) -> Result<Value> {
    let dc = json_request(
        c.client
            .post(format!("{WORKOS}/authorize/device"))
            .form(&[("client_id", CLIENT)]),
    )
    .await?;
    let url = dc["verification_uri_complete"]
        .as_str()
        .or(dc["verification_uri"].as_str())
        .ok_or("Factory gave no sign-in URL")?;
    c.challenge(
        url,
        &format!("Sign in to Factory. Device code: {}", s(&dc, "user_code")),
        false,
    )
    .await?;
    let deadline = now() + dc["expires_in"].as_u64().unwrap_or(300).max(300) * 1000 + 30000;
    let mut interval = dc["interval"].as_u64().unwrap_or(5).max(1);
    while now() < deadline {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        let (status, t) = raw_json(c.client.post(format!("{WORKOS}/authenticate")).form(&[
            ("client_id", CLIENT),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", s(&dc, "device_code")),
        ]))
        .await?;
        match s(&t, "error") {
            "authorization_pending" => continue,
            "slow_down" => {
                interval += 1;
                continue;
            }
            "" if status == 200 => {}
            _ => return Err("Factory device sign-in refused or expired".into()),
        }
        let access = s(&t, "access_token");
        if access.is_empty() {
            return Err("Factory gave no access token".into());
        }
        let cl = claims(access);
        let mut a = json!({"type":"oauth","access":access,"refresh":t["refresh_token"],"expires":expires(access),"accountId":cl["email"].as_str().or(cl["sub"].as_str()).unwrap_or(""),"orgId":cl["org_id"]});
        if s(&cl, "org_id").is_empty() && !s(&a, "refresh").is_empty() {
            let orgs = json_request(
                c.client
                    .get(format!("{API}/api/cli/org"))
                    .bearer_auth(access),
            )
            .await?;
            if let Some(org) = orgs["workosOrgIds"][0].as_str() {
                a = renew(c, &a, org).await.map_err(|e| e.message)?;
            }
        }
        let _ = who(c, &mut a).await;
        return Ok(a);
    }
    Err("Factory sign-in timed out".into())
}
pub(super) async fn fresh(c: &mut Context) -> Result<()> {
    let mut next = c.auth.clone();
    let expiry = next["expires"].as_u64().unwrap_or(0);
    if next["type"] != "api" && !s(&next, "refresh").is_empty() && expiry <= now() + 120000 {
        let _refresh = c.refresh_guard()?;
        match renew(c, &next, "").await {
            Ok(rotated) => {
                next = rotated;
                // Persistence failure must never be treated as a retryable
                // issuer failure: this grant has already been spent.
                c.save(next.clone()).await?;
            }
            Err(error) if error.can_use(expiry) => {}
            Err(error) => return Err(error.message),
        }
    }
    if now().saturating_sub(next["whoAt"].as_u64().unwrap_or(0)) > 600000 {
        let _ = who(c, &mut next).await;
    }
    if next != c.auth {
        c.save(next).await?;
    }
    Ok(())
}
pub(super) async fn usage(c: &mut Context) -> Result<Value> {
    let mut value = None;
    for attempt in 0..2 {
        let (status, v) = raw_json(signed(
            c,
            c.client
                .get(format!("{}/api/billing/limits", base(&c.auth))),
        )?)
        .await?;
        if attempt == 0 && status == 403 && mend_org(c, &v.to_string()).await? {
            continue;
        }
        if status != 200 {
            return Err(format!("Factory usage returned HTTP {status}"));
        }
        value = Some(v);
        break;
    }
    let v = value.ok_or("Factory usage unavailable")?;
    if !v["limits"]["standard"].is_object() {
        return Err("Factory account reported no limits".into());
    }
    let mut windows = Vec::new();
    let core: Vec<_> = c
        .models
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(id, _)| {
            !id.starts_with("claude")
                && !id.starts_with("gpt")
                && !id.starts_with("grok")
                && !id.starts_with("o3")
                && !id.starts_with("o4")
        })
        .map(|(id, _)| id.clone())
        .collect();
    for pool in ["standard", "core"] {
        for (key, span, label) in [
            ("fiveHour", 18000, "5 hours"),
            ("weekly", 604800, "Week"),
            ("monthly", 2592000, "Month"),
        ] {
            let w = &v["limits"][pool][key];
            if let Some(used) = w["usedPercent"].as_f64() {
                let mut out = json!({"name":format!("{pool} {label}"),"used":used,"span":span,"resetsAt":timestamp(&w["windowEnd"]),"aside":v["extraUsageAllowed"] == true && v["extraUsageBalanceCents"].as_f64().unwrap_or(0.0)>0.0});
                out[if pool == "core" {
                    "models"
                } else {
                    "notModels"
                }] = json!(core);
                windows.push(out);
            }
        }
    }
    if let Some(cents) = v["extraUsageBalanceCents"].as_f64() {
        windows.push(
            json!({"name":"Extra usage","display":format!("${:.2}",cents/100.0),"aside":true}),
        );
    }
    Ok(json!({"windows":windows}))
}
pub(super) async fn generate(c: &mut Context, model: &Value, mut body: Value) -> Result<()> {
    let wire = s(model, "wire");
    aipass_proxy_conversion::providers::factory_request(wire, &mut body)?;
    for attempt in 0..2 {
        let response = send(generation(c, wire, &body)?).await?;
        if attempt == 0 && response.status() == 403 && c.auth["type"] != "api" {
            let content_type = response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/json")
                .to_owned();
            let data = read_bytes(response, LIMIT).await?;
            if mend_org(c, &String::from_utf8_lossy(&data)).await? {
                continue;
            }
            c.headers(403, json!([["content-type", content_type]]))
                .await?;
            c.data(&data).await?;
            return c.emit(json!({"type":"end"})).await;
        }
        return c.pipe(response).await;
    }
    Err("Factory organization repair failed".into())
}
async fn mend_org(c: &mut Context, message: &str) -> Result<bool> {
    if c.auth["type"] == "api" {
        return Ok(false);
    }
    let refused = message
        .to_lowercase()
        .contains("active organization is not accessible");
    let mut next = c.auth.clone();
    let old = s(&next, "activeOrganizationId").to_owned();
    if !old.is_empty() {
        if !refused {
            return Ok(false);
        }
        next["activeOrganizationId"] = json!("");
        let _ = who(c, &mut next).await;
        if s(&next, "activeOrganizationId") == old {
            next["activeOrganizationId"] = json!("");
        }
        c.save(next).await?;
        return Ok(true);
    }
    if !refused {
        if who(c, &mut next).await.is_ok() && !s(&next, "activeOrganizationId").is_empty() {
            c.save(next).await?;
            return Ok(true);
        }
        return Ok(false);
    }
    if s(&next, "refresh").is_empty() {
        return Ok(false);
    }
    let orgs = json_request(
        c.client
            .get(format!("{}/api/cli/org", base(&next)))
            .bearer_auth(c.token()?),
    )
    .await?;
    let Some(org) = orgs["workosOrgIds"][0].as_str() else {
        return Ok(false);
    };
    let _refresh = c.refresh_guard()?;
    next = renew(c, &next, org).await.map_err(|e| e.message)?;
    next["orgId"] = json!(org);
    c.save(next).await?;
    Ok(true)
}
fn generation(c: &Context, wire: &str, body: &Value) -> Result<RequestBuilder> {
    let prem = s(&c.auth, "premBaseHost");
    let root = if prem.is_empty() {
        base(&c.auth).to_owned()
    } else if prem.contains("://") {
        prem.to_owned()
    } else {
        format!("https://{prem}")
    };
    let u = url::Url::parse(&root).map_err(|_| "invalid Factory organization host")?;
    if u.scheme() != "https" || !u.username().is_empty() || u.password().is_some() {
        return Err("invalid Factory organization host".into());
    }
    let upstream = match s(body, "model") {
        id if id.starts_with("claude") => "anthropic",
        id if id.starts_with("grok") => "xai",
        id if id.starts_with("gpt") || id.starts_with("o3") || id.starts_with("o4") => "openai",
        "glm-5.2" | "nemotron-3-ultra" => "baseten",
        "mistral-medium-3.5" => "mistral",
        _ => "fireworks",
    };
    let endpoint = format!(
        "{}/api/llm/{}/v1{}",
        root.trim_end_matches('/'),
        if wire == "anthropic" { "a" } else { "o" },
        path(wire)
    );
    let mut req = signed(c, c.client.post(endpoint))?
        .header("x-api-provider", upstream)
        .header("x-session-id", &c.session)
        .header("x-assistant-message-id", uuid::Uuid::new_v4().to_string())
        .header("x-provider-routing-source", "registry_default");
    if upstream == "openai" {
        req = req.header("OpenAI-Platform", "org-bHuLtG1fGmYk5YaOihAAXFBw");
    }
    if wire == "anthropic" {
        req = req
            .header("x-api-key", "placeholder")
            .header("anthropic-version", "2023-06-01");
    }
    Ok(req.json(body))
}
fn timestamp(v: &Value) -> Value {
    let time = if let Some(n) = v
        .as_i64()
        .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
    {
        time::OffsetDateTime::from_unix_timestamp(n / if n > 100000000000 { 1000 } else { 1 }).ok()
    } else {
        v.as_str().and_then(|s| {
            time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
        })
    };
    time.and_then(|t| {
        t.format(&time::format_description::well_known::Rfc3339)
            .ok()
    })
    .map(Value::String)
    .unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rotation_preserves_omitted_refresh_and_transient_failures_keep_only_unexpired_access() {
        let auth =
            json!({"type":"oauth","access":"old","refresh":"old-refresh","accountId":"alice"});
        let kept = renewed_auth(&auth, &json!({"access_token":"new"})).unwrap();
        assert_eq!(kept["refresh"], "old-refresh");
        let rotated = renewed_auth(
            &auth,
            &json!({"access_token":"new","refresh_token":"new-refresh"}),
        )
        .unwrap();
        assert_eq!(rotated["refresh"], "new-refresh");
        let transient = RefreshFailure {
            message: "network".into(),
            refused: false,
        };
        let refused = RefreshFailure {
            message: "invalid grant".into(),
            refused: true,
        };
        assert!(transient.can_use(now() + 60000));
        assert!(!transient.can_use(1));
        assert!(!refused.can_use(now() + 60000));
    }
    #[test]
    fn reset_accepts_numeric_milliseconds_and_iso_dates() {
        assert_eq!(
            timestamp(&json!(1793000000000i64)),
            timestamp(&json!("1793000000000"))
        );
        assert_eq!(
            timestamp(&json!(1793000000000i64)),
            timestamp(&json!(1793000000i64))
        );
        assert!(timestamp(&json!("soon")).is_null());
    }
}
