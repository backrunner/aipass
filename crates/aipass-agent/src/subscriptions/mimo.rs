use super::*;
use reqwest::cookie::{CookieStore, Jar};
const ACCOUNT: &str = "https://account.xiaomi.com";
const UA: &str = "miNative PC/Normal Windows_NT/10.0.26100 SDKV/1.0.0 DEVT/PC DEVS/Windows APP/miaccount_desktop APPV/0.1.0";
fn base(region: &str) -> &'static str {
    match region {
        "RU" => "https://mimo-server-ru.xiaomimimo.com/api",
        "IN" => "https://mimo-server-in.xiaomimimo.com/api",
        _ => "https://mimo-server-sgp.xiaomimimo.com/api",
    }
}
fn app(r: RequestBuilder) -> RequestBuilder {
    r.header("User-Agent", UA)
        .header("X-Client-Version", "26.929.292248")
}
fn allowed(url: &url::Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(
            url.host_str(),
            Some(
                "account.xiaomi.com"
                    | "mimo-server-sgp.xiaomimimo.com"
                    | "mimo-server-ru.xiaomimimo.com"
                    | "mimo-server-in.xiaomimimo.com"
            )
        )
}
async fn account_get(c: &Context, url: &str, device: &str) -> Result<Value> {
    let u = url::Url::parse(url).map_err(|_| "invalid Xiaomi sign-in URL")?;
    if !allowed(&u) || u.host_str() != Some("account.xiaomi.com") {
        return Err("unexpected Xiaomi sign-in host".into());
    }
    let response = send(
        c.client
            .get(u)
            .header("User-Agent", UA)
            .header(
                "Cookie",
                format!("deviceId={device}; pass_ua=pc; uLocale=zh_CN"),
            )
            .timeout(Duration::from_secs(70)),
    )
    .await?;
    let bytes = read_bytes(response, LIMIT).await?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| "invalid Xiaomi response")?
        .trim()
        .trim_start_matches("&&&START&&&");
    serde_json::from_str(text).map_err(|_| "invalid Xiaomi account response".into())
}
async fn session(c: &Context, creds: &Value, root: &str) -> Result<(Value, Value)> {
    let jar = Jar::default();
    let account = url::Url::parse(ACCOUNT).unwrap();
    for (name, value) in [
        ("userId", s(creds, "userId")),
        ("passToken", s(creds, "passToken")),
        ("cUserId", s(creds, "cUserId")),
        ("deviceId", s(creds, "deviceId")),
        ("pass_ua", "pc"),
        ("uLocale", "zh_CN"),
    ] {
        if value.contains([';', '\r', '\n']) {
            return Err("invalid Xiaomi credential".into());
        }
        if !value.is_empty() {
            jar.add_cookie_str(&format!("{name}={value}; Path=/; Secure"), &account);
        }
    }
    let mut url =
        url::Url::parse(&format!("{root}/user/xiaomi/me")).map_err(|_| "invalid MiMo endpoint")?;
    for _ in 0..11 {
        if !allowed(&url) {
            return Err("unexpected Xiaomi session redirect".into());
        }
        let mut req = app(c.client.get(url.clone()));
        if let Some(cookies) = jar.cookies(&url) {
            req = req.header("Cookie", cookies);
        }
        let response = send(req.timeout(Duration::from_secs(30))).await?;
        jar.set_cookies(&mut response.headers().get_all("set-cookie").iter(), &url);
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or("MiMo redirect missing destination")?;
            let mut next = url.join(location).map_err(|_| "invalid MiMo redirect")?;
            if next.scheme() == "http"
                && next.host_str()
                    == url::Url::parse(root)
                        .ok()
                        .as_ref()
                        .and_then(|u| u.host_str())
            {
                let _ = next.set_scheme("https");
            }
            url = next;
            continue;
        }
        let status = response.status();
        let me: Value = serde_json::from_slice(&read_bytes(response, LIMIT).await?)
            .map_err(|_| "Xiaomi session expired; reconnect the account")?;
        if !status.is_success() || me["code"] != 0 {
            return Err("Xiaomi did not accept the MiMo session or account region".into());
        }
        let mut cookies = json!({});
        let target = url::Url::parse(&format!("{root}/route/chat/completions")).unwrap();
        if let Some(header) = jar.cookies(&target) {
            for pair in header
                .to_str()
                .map_err(|_| "invalid MiMo cookies")?
                .split(';')
            {
                if let Some((k, v)) = pair.trim().split_once('=') {
                    cookies[k] = json!(v);
                }
            }
        }
        if cookies.as_object().is_none_or(|m| m.is_empty()) {
            return Err("MiMo returned no session cookies".into());
        }
        return Ok((cookies, me["data"].clone()));
    }
    Err("too many Xiaomi session redirects".into())
}
fn auth(creds: Value, cookies: Value) -> Value {
    json!({"type":"oauth","accountId":creds["userId"],"refresh":creds.to_string(),"access":cookies.to_string(),"expires":now()+86400000})
}
fn creds(c: &Context) -> Result<Value> {
    serde_json::from_str(s(&c.auth, "refresh"))
        .map_err(|_| "invalid Xiaomi account credentials".into())
}
pub(super) async fn login(c: &mut Context, _: usize) -> Result<Value> {
    let root = base("SGP");
    let device = format!("pc_{}", hex_id());
    let mut sid = "mimosgp".to_owned();
    let mut callback = format!("{root}/sts");
    let res = send(app(c.client.get(format!("{root}/user/xiaomi/me")))).await?;
    if let Some(location) = res
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| url::Url::parse(v).ok())
    {
        for (k, v) in location.query_pairs() {
            if k == "sid" {
                sid = v.into_owned();
            } else if k == "callback" {
                callback = v.into_owned();
            }
        }
    }
    if !url::Url::parse(&callback)
        .ok()
        .as_ref()
        .is_some_and(allowed)
    {
        return Err("unexpected Xiaomi callback host".into());
    }
    let mut u = url::Url::parse(&format!("{ACCOUNT}/longPolling/loginUrl")).unwrap();
    u.query_pairs_mut().extend_pairs([
        ("_group", "DEFAULT"),
        ("_qrsize", "240"),
        ("qs", &format!("%3Fsid%3D{sid}%26_json%3Dtrue")),
        ("callback", &callback),
        ("_hasLogo", "false"),
        ("sid", &sid),
        ("serviceParam", ""),
        ("_locale", "en_US"),
    ]);
    let ticket = account_get(c, u.as_str(), &device).await?;
    if ticket["code"] != 0 {
        return Err("Xiaomi sign-in failed".into());
    }
    c.challenge(
        s(&ticket, "loginUrl"),
        "Sign in with your Xiaomi account.",
        false,
    )
    .await?;
    let deadline = now() + ticket["timeout"].as_u64().unwrap_or(600).min(600) * 1000;
    while now() < deadline {
        if let Ok(p) = account_get(c, s(&ticket, "lp"), &device).await {
            if !s(&p, "passToken").is_empty() && !p["userId"].is_null() {
                let user = p["userId"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| p["userId"].to_string());
                let mut cr = json!({"userId":user,"cUserId":p["cUserId"],"passToken":p["passToken"],"deviceId":device,"region":"SGP","base":root});
                let (mut cookies, me) = session(c, &cr, root).await?;
                let region = s(&me, "region").to_uppercase();
                if ["RU", "IN"].contains(&region.as_str()) {
                    cr["region"] = json!(region);
                    cr["base"] = json!(base(&region));
                    cookies = session(c, &cr, base(&region)).await?.0;
                }
                return Ok(auth(cr, cookies));
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Err("Xiaomi sign-in timed out".into())
}
pub(super) async fn fresh(c: &mut Context) -> Result<()> {
    if c.auth["expires"].as_u64().unwrap_or(0) > now() {
        return Ok(());
    }
    let cr = creds(c)?;
    let (cookies, me) = session(c, &cr, base(s(&cr, "region"))).await?;
    if me["userId"]
        .as_str()
        .is_some_and(|id| id != s(&cr, "userId"))
    {
        return Err("Xiaomi account changed".into());
    }
    c.save(auth(cr, cookies)).await
}
fn signed(c: &Context, req: RequestBuilder) -> Result<RequestBuilder> {
    let cookies: Value = serde_json::from_str(c.token()?).map_err(|_| "invalid MiMo cookies")?;
    let mut parts = Vec::new();
    for (k, v) in cookies.as_object().ok_or("invalid MiMo cookies")? {
        let v = v.as_str().ok_or("invalid MiMo cookie")?;
        if k.contains([';', '\r', '\n', '=']) || v.contains([';', '\r', '\n']) {
            return Err("invalid MiMo cookie".into());
        }
        parts.push(format!("{k}={v}"));
    }
    Ok(app(req).header("Cookie", parts.join("; ")))
}
fn server_time(v: &Value) -> Value {
    let Some(s) = v.as_str() else {
        return Value::Null;
    };
    let s = s.replace(' ', "T");
    let iso = if s.len() == 10 {
        format!("{s}T00:00:00+08:00")
    } else if s.len() >= 19 && s.get(19..).is_some_and(|s| !s.contains(['Z', '+', '-'])) {
        format!("{s}+08:00")
    } else {
        s
    };
    time::OffsetDateTime::parse(&iso, &time::format_description::well_known::Rfc3339)
        .ok()
        .and_then(|t| {
            t.format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        .map(Value::String)
        .unwrap_or(Value::Null)
}
pub(super) async fn usage(c: &mut Context) -> Result<Value> {
    let cr = creds(c)?;
    let root = base(s(&cr, "region"));
    let plan = json_request(signed(
        c,
        c.client
            .get(format!("{root}/user/xiaomi/subscription/self")),
    )?)
    .await?;
    let current = &plan["data"]["current"];
    let tier = match current["planTier"].as_u64() {
        Some(1) => "Starter",
        Some(2) => "Plus",
        Some(3) => "Pro",
        Some(4) => "Ultra",
        _ => "Free",
    };
    let mut out = json!({"plan":current["title"].as_str().filter(|s|!s.is_empty()).unwrap_or(tier),"until":server_time(&current["endTime"])});
    if let Ok(v) = json_request(signed(c, c.client.get(format!("{root}/user/usage")))?).await {
        if let (Some(percent), Some(_)) = (
            v["data"]["percent"].as_f64(),
            v["data"]["resetDate"].as_str(),
        ) {
            out["windows"] = json!([{"name":"7 days","used":(100.0-percent).clamp(0.0,100.0),"span":604800,"resetsAt":server_time(&v["data"]["resetDate"])}]);
        }
    }
    Ok(out)
}
pub(super) async fn generate(c: &mut Context, _: &Value, mut body: Value) -> Result<()> {
    let cr = creds(c)?;
    if body["model"] == "mimo-auto" {
        body["model"] = json!("mimo-pro");
    }
    for attempt in 0..2 {
        let req = signed(
            c,
            c.client
                .post(format!("{}/route/chat/completions", base(s(&cr, "region")))),
        )?
        .header("X-Mimo-Source", "mimocode-cli-free")
        .json(&body);
        let response = send(req).await?;
        if attempt == 0 && response.status() == 401 {
            c.auth["expires"] = json!(0);
            fresh(c).await?;
            continue;
        }
        return c.pipe(response).await;
    }
    Err("MiMo session refused".into())
}
