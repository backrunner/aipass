use super::*;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm,
};
use sha2::{Digest, Sha256};
const ZCODE: &str = "https://zcode.z.ai";
fn root(site: &str) -> &'static str {
    if site == "bigmodel" {
        "https://bigmodel.cn"
    } else {
        "https://api.z.ai"
    }
}
fn base(site: &str) -> &'static str {
    if site == "bigmodel" {
        "https://open.bigmodel.cn/api/anthropic"
    } else {
        "https://api.z.ai/api/anthropic"
    }
}
fn platform() -> String {
    format!(
        "{}-{}",
        if cfg!(target_os = "macos") {
            "darwin"
        } else if cfg!(windows) {
            "win32"
        } else {
            "linux"
        },
        if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "x64"
        }
    )
}
fn request(
    c: &Context,
    method: reqwest::Method,
    url: &str,
    auth: &str,
    device: &str,
) -> RequestBuilder {
    let mut r = c
        .client
        .request(method, url)
        .header("User-Agent", "ZCode/3.14.3")
        .header("Accept", "application/json");
    if !auth.is_empty() {
        r = r.header("Authorization", auth);
    }
    if url.starts_with(ZCODE) {
        r = r.header("X-Device-Mid", device);
    }
    r
}
async fn data(r: RequestBuilder) -> Result<Value> {
    let v = json_request(r).await?;
    let code = v["code"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| v["code"].to_string());
    if !["null", "0", "200"].contains(&code.as_str()) {
        return Err(format!("ZCode API refused the operation (code {code})"));
    }
    Ok(v["data"].clone())
}
fn state(c: &Context) -> Result<Value> {
    if c.auth["type"] == "api" {
        Ok(
            json!({"site":c.auth["metadata"]["site"].as_str().unwrap_or("zai"),"key":c.auth["key"],"device":c.session}),
        )
    } else {
        serde_json::from_str(s(&c.auth, "refresh")).map_err(|_| "invalid ZCode account".into())
    }
}
fn team(req: RequestBuilder, st: &Value) -> RequestBuilder {
    if !s(st, "org").is_empty() && !s(st, "project").is_empty() {
        req.header("Bigmodel-Organization", s(st, "org"))
            .header("Bigmodel-Project", s(st, "project"))
            .header(
                "Set-Language",
                if st["site"] == "bigmodel" { "zh" } else { "en" },
            )
    } else {
        req
    }
}
async fn plan(c: &Context, st: &Value) -> Result<String> {
    let v = data(request(
        c,
        reqwest::Method::GET,
        &format!("{}/api/biz/subscription/list", root(s(st, "site"))),
        s(st, "key"),
        s(st, "device"),
    ))
    .await?;
    Ok(v.as_array()
        .into_iter()
        .flatten()
        .find(|x| s(x, "status").eq_ignore_ascii_case("VALID"))
        .map(|x| s(x, "productName").to_owned())
        .unwrap_or_default())
}
async fn start(c: &Context, st: &Value) -> Result<bool> {
    if s(st, "jwt").is_empty() || !s(st, "org").is_empty() {
        return Ok(false);
    }
    if s(st, "key").is_empty() {
        return Ok(true);
    }
    Ok(plan(c, st).await?.is_empty())
}
async fn key(c: &Context, st: &Value, auth: &str, team_key: bool) -> Result<String> {
    let mut url = url::Url::parse(root(s(st, "site"))).unwrap();
    url.path_segments_mut()
        .map_err(|_| "invalid ZCode API root")?
        .extend([
            "api",
            "biz",
            "v1",
            "organization",
            s(st, "org"),
            "projects",
            s(st, "project"),
            "api_keys",
        ]);
    let req = |method| team(request(c, method, url.as_str(), auth, s(st, "device")), st);
    let name = if team_key {
        "zcode-team-api-key"
    } else {
        "zcode-api-key"
    };
    let list = data(req(reqwest::Method::GET)).await?;
    let mut id = list
        .as_array()
        .into_iter()
        .flatten()
        .find(|k| k["name"] == name && (!team_key || k["keyType"] == 2 || k["keyType"] == "2"))
        .map(|k| s(k, "apiKey").to_owned())
        .unwrap_or_default();
    if id.is_empty() {
        let mut b = json!({"name":name});
        if team_key {
            b["keyType"] = json!(2);
        }
        id = s(&data(req(reqwest::Method::POST).json(&b)).await?, "apiKey").into();
    }
    if id.is_empty() {
        return Err("ZCode returned no project key".into());
    }
    url.path_segments_mut().unwrap().extend(["copy", &id]);
    let v = data(team(
        request(c, reqwest::Method::GET, url.as_str(), auth, s(st, "device")),
        st,
    ))
    .await?;
    if s(&v, "secretKey").is_empty() {
        if team_key {
            return Ok(id);
        }
        return Err("ZCode returned no key secret".into());
    }
    Ok(format!("{id}.{}", s(&v, "secretKey")))
}
async fn signed_in(c: &Context, site: &str, token: &str, jwt: &str, device: &str) -> Result<Value> {
    let auth = if site == "bigmodel" {
        token.to_owned()
    } else {
        let t = data(
            request(
                c,
                reqwest::Method::POST,
                "https://api.z.ai/api/auth/z/login",
                "",
                device,
            )
            .json(&json!({"token":token})),
        )
        .await?;
        if s(&t, "access_token").is_empty() {
            return Err("Z.ai returned no business token".into());
        }
        format!("Bearer {}", s(&t, "access_token"))
    };
    let info = data(request(
        c,
        reqwest::Method::GET,
        &format!("{}/api/biz/customer/getCustomerInfo", root(site)),
        &auth,
        device,
    ))
    .await?;
    let mut projects = Vec::new();
    for org in info["organizations"].as_array().into_iter().flatten() {
        for p in org["projects"].as_array().into_iter().flatten() {
            projects.push((org, p));
        }
    }
    projects.sort_by_key(|(o, p)| {
        (
            !s(o, "organizationName").contains("默认机构"),
            !s(p, "projectName").contains("默认项目"),
        )
    });
    for is_team in [false, true] {
        for (org, p) in &projects {
            let kind = p["projectType"] == 2 || p["projectType"] == "2";
            if kind != is_team {
                continue;
            }
            let mut st = json!({"site":site,"device":device,"jwt":jwt,"org":org["organizationId"],"project":p["projectId"],"token":auth});
            let name = if is_team {
                let d = data(team(
                    request(
                        c,
                        reqwest::Method::GET,
                        &format!(
                            "{}/api/biz/team/subscribe/product/querySubscribeDetail",
                            root(site)
                        ),
                        &auth,
                        device,
                    ),
                    &st,
                ))
                .await?;
                if d["hasSubscription"] == false
                    || !s(&d, "status").eq_ignore_ascii_case("EFFECTIVE")
                    || !s(&d, "memberGrantStatus").eq_ignore_ascii_case("VALID")
                {
                    continue;
                }
                s(&d, "productName").to_owned()
            } else {
                String::new()
            };
            st["key"] = json!(key(c, &st, &auth, is_team).await?);
            if is_team {
                st["plan"] = json!(name);
                return Ok(st);
            }
            let name = plan(c, &st).await?;
            if !name.is_empty() {
                st.as_object_mut().unwrap().remove("org");
                st.as_object_mut().unwrap().remove("project");
                st["plan"] = json!(name);
                return Ok(st);
            }
            break;
        }
    }
    if !jwt.is_empty() {
        let st = json!({"site":site,"device":device,"jwt":jwt});
        let b = start_balance(c, &st).await?;
        if b["plans"]
            .as_array()
            .into_iter()
            .flatten()
            .any(active_start_plan)
        {
            return Ok(st);
        }
    }
    Err("No active GLM Coding Plan, team seat, or ZCode Start Plan".into())
}
fn decrypt(key: &[u8], v: &Value) -> Option<String> {
    let p = v
        .as_str()?
        .strip_prefix("enc:v1:")?
        .split('.')
        .collect::<Vec<_>>();
    if p.len() != 3 {
        return None;
    }
    let iv = URL_SAFE_NO_PAD.decode(p[0].trim_end_matches('=')).ok()?;
    if iv.len() != 12 {
        return None;
    }
    let mut bytes = URL_SAFE_NO_PAD.decode(p[2].trim_end_matches('=')).ok()?;
    bytes.extend(URL_SAFE_NO_PAD.decode(p[1].trim_end_matches('=')).ok()?);
    let plain = Aes256Gcm::new_from_slice(key)
        .ok()?
        .decrypt(aes_gcm::Nonce::from_slice(&iv), bytes.as_slice())
        .ok()?;
    String::from_utf8(plain).ok()
}
fn native(source: &Value) -> Result<Value> {
    let home = home()?;
    let dir = native_import::root(source).unwrap_or_else(|| home.join(".zcode/v2"));
    let selected = native_import::selector(source);
    let store = read_json(&dir.join("credentials.json"))?;
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(windows) {
        "win32"
    } else {
        "linux"
    };
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default();
    let seed = std::env::var("ZCODE_CREDENTIAL_SECRET").unwrap_or_else(|_| {
        format!(
            "zcode-credential-fallback:{os}:{}:{}",
            home.display(),
            user.rsplit('\\').next().unwrap_or("")
        )
    });
    let key = Sha256::digest(seed.as_bytes());
    let mut key_owner = String::new();
    let mut st = json!({"site":"zai","device":uuid::Uuid::new_v4().to_string(),"source":"zcode","jwt":decrypt(&key,&store["zcodejwttoken"]).unwrap_or_default().trim_start_matches("Bearer "),"key":""});
    for (name, v) in store.as_object().ok_or("invalid ZCode credential store")? {
        if name.contains(":coding-plan:")
            && name.ends_with(":api-key")
            && (selected.is_empty() || selected == format!("key:{name}"))
        {
            if let Some(k) = decrypt(&key, v).filter(|k| k.contains('.')) {
                let site = if name.contains(":bigmodel-") {
                    "bigmodel"
                } else {
                    "zai"
                };
                if s(&st, "key").is_empty() || site == "zai" {
                    st["key"] = json!(k);
                    st["site"] = json!(site);
                    // The vendor credential key is scoped by stable user ID.
                    // Site-wide user_info describes only the latest login.
                    key_owner = name
                        .split_once(":coding-plan:")
                        .map(|(user, _)| user.to_owned())
                        .unwrap_or_default();
                }
            }
        }
    }
    if let Ok(settings) = read_json(&dir.join("setting.json")) {
        for site in ["zai", "bigmodel"] {
            let sel = &settings["providerFamilyConnectionSelections"][site];
            if sel["kind"] == "team-coding-plan"
                && (selected.is_empty() || selected == format!("team:{site}"))
            {
                if let Some(tok) = decrypt(&key, &store[format!("oauth:{site}:access_token")]) {
                    st["site"] = json!(site);
                    st["org"] = json!(crate::community::identity_value(&sel["organizationId"])
                        .unwrap_or_default());
                    st["project"] = json!(
                        crate::community::identity_value(&sel["projectId"]).unwrap_or_default()
                    );
                    st["token"] = json!(tok);
                    st["key"] = json!("");
                    key_owner.clear();
                    break;
                }
            }
        }
    }
    if (selected.starts_with("key:") && s(&st, "key").is_empty())
        || (selected.starts_with("team:") && s(&st, "token").is_empty())
    {
        return Err("Selected ZCode sign-in is unavailable".into());
    }
    let mut identity = key_owner;
    for site in ["zai", "bigmodel"] {
        if site != s(&st, "site") {
            continue;
        }
        if identity.is_empty() {
            if let Some(info) = decrypt(&key, &store[format!("oauth:{site}:user_info")])
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            {
                identity = crate::community::identity_value(&info["user_id"])
                    .or_else(|| crate::community::identity_value(&info["email"]))
                    .unwrap_or_default();
                if !identity.is_empty() {
                    break;
                }
            }
        }
    }
    if s(&st, "key").is_empty() && s(&st, "jwt").is_empty() && s(&st, "token").is_empty() {
        return Err("ZCode is not signed in".into());
    }
    if !selected.is_empty() && !identity.is_empty() {
        identity = format!(
            "{}::{}::{}::{}",
            identity,
            s(&st, "site"),
            s(&st, "org"),
            s(&st, "project")
        );
    }
    Ok(
        json!({"type":"oauth","access":if s(&st,"key").is_empty(){s(&st,"jwt")}else{s(&st,"key")},"refresh":st.to_string(),"expires":0,"accountId":identity}),
    )
}
pub(super) async fn login(c: &mut Context, method: usize) -> Result<Value> {
    if method == 2 {
        return native(&c.auth);
    }
    let site = if method == 1 { "bigmodel" } else { "zai" };
    let poll = format!("Bearer {}{}", hex_id(), hex_id());
    let device = uuid::Uuid::new_v4().to_string();
    let flow = data(
        request(
            c,
            reqwest::Method::POST,
            &format!("{ZCODE}/api/v1/oauth/cli/init"),
            &poll,
            &device,
        )
        .json(&json!({"provider":site})),
    )
    .await?;
    let mut url =
        url::Url::parse(s(&flow, "authorize_url")).map_err(|_| "invalid ZCode sign-in URL")?;
    url.query_pairs_mut().append_pair(
        if site == "bigmodel" {
            "redirect"
        } else {
            "redirect_uri"
        },
        &format!(
            "{ZCODE}/app/oauth/login?redirect=zcode%3A%2F%2Foauth%2Fcallback&app_version=3.14.3"
        ),
    );
    c.challenge(url.as_str(), "Sign in to ZCode in the browser.", false)
        .await?;
    let deadline = flow["expires_at"]
        .as_u64()
        .map(|n| n * 1000)
        .unwrap_or(now() + 300000);
    let interval = flow["poll_interval_sec"].as_u64().unwrap_or(1).max(1);
    while now() < deadline {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        let mut url = url::Url::parse(&format!("{ZCODE}/api/v1/oauth/cli/poll/")).unwrap();
        url.path_segments_mut()
            .unwrap()
            .pop_if_empty()
            .push(s(&flow, "flow_id"));
        let got = data(request(
            c,
            reqwest::Method::GET,
            url.as_str(),
            &poll,
            &device,
        ))
        .await?;
        match s(&got, "status") {
            "" | "pending" => continue,
            "ready" => {}
            _ => return Err("ZCode sign-in declined".into()),
        }
        let token = got[site]["access_token"]
            .as_str()
            .or(got[site]["accessToken"].as_str())
            .ok_or("ZCode gave no token")?;
        let st = signed_in(c, site, token, s(&got, "token"), &device).await?;
        return Ok(
            json!({"type":"oauth","access":st["key"].as_str().filter(|s|!s.is_empty()).unwrap_or(s(&st,"jwt")),"refresh":st.to_string(),"expires":0,"accountId":got["user"]["user_id"].as_str().or(got["user"]["email"].as_str()).unwrap_or("")}),
        );
    }
    Err("ZCode sign-in timed out".into())
}
pub(super) async fn fresh(c: &mut Context) -> Result<()> {
    let mut st = state(c)?;
    if st["source"] == "zcode" {
        let next = native(&c.auth)?;
        if s(&next, "accountId").is_empty() || next["accountId"] != c.auth["accountId"] {
            return Err("ZCode native account changed; reconnect explicitly".into());
        }
        let new: Value = serde_json::from_str(s(&next, "refresh"))
            .map_err(|_| "invalid ZCode native account")?;
        if new["key"] != st["key"] || new["jwt"] != st["jwt"] {
            c.save(next).await?;
            st = state(c)?;
        }
    }
    if !s(&st, "org").is_empty() && s(&st, "key").is_empty() {
        st["key"] = json!(key(c, &st, s(&st, "token"), true).await?);
        let mut next = c.auth.clone();
        next["access"] = st["key"].clone();
        next["refresh"] = json!(st.to_string());
        c.save(next).await?;
    }
    if start(c, &st).await? {
        let expiry = expires(s(&st, "jwt"));
        if expiry > 0 && expiry <= now() {
            return Err("ZCode Start Plan sign-in expired; reconnect".into());
        }
    }
    Ok(())
}
pub(super) async fn models(c: &mut Context) -> Result<Value> {
    let st = state(c)?;
    let start = start(c, &st).await?;
    let v = data(
        request(
            c,
            reqwest::Method::GET,
            &format!("{ZCODE}/api/v1/client/configs"),
            "",
            s(&st, "device"),
        )
        .query(&[("app_version", "3.14.3"), ("platform", &platform())]),
    )
    .await?;
    let url = s(&v["configs"], "builtin_provider_config_json");
    let u = url::Url::parse(url).map_err(|_| "ZCode gave no model config")?;
    if u.scheme() != "https" || !u.username().is_empty() || u.password().is_some() {
        return Err("invalid ZCode config URL".into());
    }
    let cfg = json_request(c.client.get(u)).await?;
    let plan = if start {
        "account:zai-start-plan"
    } else if st["site"] == "bigmodel" {
        "account:bigmodel-individual-coding-plan"
    } else {
        "account:zai-individual-coding-plan"
    };
    let cfg = &cfg["config"];
    let mut ids = std::collections::BTreeSet::new();
    for rule in cfg["providerConfigRules"]["providerRules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["providerId"] == plan)
    {
        for id in rule["builtinModelIds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            ids.insert(id.to_owned());
        }
    }
    for rule in cfg["modelConfigRules"]["builtinProviderModelRules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["providerId"] == plan)
    {
        let id = s(rule, "modelId");
        if rule["config"]["enabled"] == false {
            ids.retain(|k| !k.eq_ignore_ascii_case(id));
        } else {
            ids.insert(id.into());
        }
    }
    let mut out = json!({});
    for id in ids {
        let mut m = model(&id, &id, "anthropic", base(s(&st, "site")), 0, 0);
        for rule in cfg["modelConfigRules"]["modelRules"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if regex::RegexBuilder::new(&format!("^(?:{})$", s(rule, "modelMatch")))
                .case_insensitive(true)
                .size_limit(1024 * 1024)
                .build()
                .is_ok_and(|r| r.is_match(&id))
            {
                let x = &rule["config"];
                if x["properties"]["contextWindow"].is_number() {
                    m["limit"]["context"] = x["properties"]["contextWindow"].clone();
                }
                if x["optionSpecs"]["maxOutputTokens"]["max"].is_number() {
                    m["limit"]["output"] = x["optionSpecs"]["maxOutputTokens"]["max"].clone();
                }
                m["attachment"] = json!(x["properties"]["inputFormat"]["supportsImage"] == true);
                m["variants"] = json!({});
                for effort in x["optionSpecs"]["reasoningLevel"]["values"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    let e = match effort {
                        "disabled" => "none",
                        "enabled" => "high",
                        _ => effort,
                    };
                    m["variants"][e] = json!({"reasoningEffort":e});
                }
            }
        }
        out[&id] = m;
    }
    Ok(out)
}
fn number(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}
async fn start_balance(c: &Context, st: &Value) -> Result<Value> {
    data(request(
        c,
        reqwest::Method::GET,
        &format!("{ZCODE}/api/v1/zcode-plan/billing/balance?app_version=3.14.3"),
        &format!("Bearer {}", s(st, "jwt")),
        s(st, "device"),
    ))
    .await
}
pub(super) async fn usage(c: &mut Context) -> Result<Value> {
    let st = state(c)?;
    let mut windows = vec![];
    if start(c, &st).await? {
        let b = start_balance(c, &st).await?;
        for x in b["balances"].as_array().into_iter().flatten() {
            let total = number(&x["total_units"]);
            let used =
                number(&x["used_units"]).or_else(|| Some(total? - number(&x["remaining_units"])?));
            let expired = b["plans"].as_array().into_iter().flatten().any(|p| {
                p["user_plan_id"] == x["user_plan_id"]
                    && (s(p, "status") == "expired"
                        || number(&p["ends_at"]).is_some_and(|t| t * 1000.0 <= now() as f64))
            });
            if expired {
                continue;
            }
            if let (Some(t), Some(u)) = (total.filter(|v| *v > 0.0), used) {
                let models: Vec<_> = x["capabilities"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(|s| s.trim_start_matches("model:"))
                    .collect();
                windows.push(json!({"name":x["show_name"].as_str().unwrap_or("Credits"),"used":100.0*u/t,"models":models,"resetsAt":iso_ms(number(&x["expires_at"]).unwrap_or(0.0)*1000.0)}));
            }
        }
        return Ok(json!({"plan":"Start Plan","windows":windows}));
    }
    let suffix = if s(&st, "org").is_empty() {
        ""
    } else {
        "?type=2"
    };
    let d = data(team(
        request(
            c,
            reqwest::Method::GET,
            &format!(
                "{}/api/monitor/usage/quota/limit{suffix}",
                root(s(&st, "site"))
            ),
            s(&st, "key"),
            s(&st, "device"),
        ),
        &st,
    ))
    .await?;
    for x in d["limits"].as_array().into_iter().flatten() {
        let unit = x["unit"].as_u64().unwrap_or(0);
        let n = x["number"]
            .as_u64()
            .unwrap_or(if unit == 3 { 5 } else { 1 })
            .max(1);
        let span = n * match unit {
            1 => 60,
            3 => 3600,
            4 => 86400,
            5 => 2592000,
            6 => 604800,
            _ => 0,
        };
        let total = number(&x["usage"]);
        let used = if let Some(t) = total.filter(|t| *t > 0.0) {
            number(&x["remaining"])
                .map(|r| 100.0 * (t - r) / t)
                .or(number(&x["percentage"]))
                .or_else(|| number(&x["currentValue"]).map(|u| 100.0 * u / t))
        } else {
            number(&x["percentage"])
        };
        windows.push(json!({"name":if x["type"]=="TIME_LIMIT"{"MCP · Month".into()}else{format!("{} hours",span/3600)},"used":used,"span":span,"aside":x["type"]=="TIME_LIMIT"||total==Some(0.0),"resetsAt":iso_ms(number(&x["nextResetTime"]).unwrap_or(0.0))}));
    }
    Ok(json!({"plan":format!("GLM Coding {}",s(&d,"level")),"windows":windows}))
}
fn iso_ms(ms: f64) -> Value {
    if ms <= 0.0 {
        return Value::Null;
    }
    time::OffsetDateTime::from_unix_timestamp((ms / 1000.0) as i64)
        .ok()
        .and_then(|t| {
            t.format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        .map(Value::String)
        .unwrap_or(Value::Null)
}
pub(super) async fn generate(c: &mut Context, _: &Value, mut body: Value) -> Result<()> {
    let st = state(c)?;
    let start = start(c, &st).await?;
    let endpoint = if start {
        format!("{ZCODE}/api/v1/zcode-plan/anthropic/v1/messages")
    } else {
        format!("{}/v1/messages", base(s(&st, "site")))
    };
    let mut req = c
        .client
        .post(endpoint)
        .header("anthropic-version", "2023-06-01");
    if start {
        let date = time::OffsetDateTime::now_utc().date().to_string();
        let os_version = os_version();
        let context = aipass_proxy_conversion::providers::zcode::Environment {
            cwd: "/tmp",
            platform: vendor_platform(),
            shell: if cfg!(windows) { "cmd" } else { "sh" },
            os_version: &os_version,
            date: &date,
        };
        aipass_proxy_conversion::providers::zcode::dress(
            &mut body,
            s(&st, "site"),
            s(&st, "device"),
            &context,
        )?;
        req = req
            .bearer_auth(s(&st, "jwt"))
            .header("HTTP-Referer", ZCODE)
            .header("User-Agent", "ZCode/3.14.3 ai-sdk/anthropic/3.0.81")
            .header("X-ZCode-App-Version", "3.14.3")
            .header("X-Title", "Z Code@cli")
            .header("X-Release-Channel", "production")
            .header("X-Client-Language", "en-US")
            .header("X-ZCode-Agent", "glm")
            .header("X-Client-Timezone", "UTC")
            .header("X-Platform", vendor_platform())
            .header(
                "X-Os-Category",
                if cfg!(target_os = "macos") {
                    "macos"
                } else {
                    std::env::consts::OS
                },
            )
            .header("X-Os-Version", &os_version)
            .header("x-request-id", uuid::Uuid::new_v4().to_string())
            .header("x-zcode-session-type", "main")
            .header("x-zcode-trace-id", uuid::Uuid::new_v4().to_string());
    } else {
        req = req
            .bearer_auth(s(&st, "key"))
            .header("x-api-key", s(&st, "key"));
    }
    c.pipe(send(req.json(&body)).await?).await
}

fn active_start_plan(p: &Value) -> bool {
    let id = s(p, "plan_id").trim().to_lowercase();
    let name = s(p, "name").trim().to_lowercase();
    s(p, "status").trim().eq_ignore_ascii_case("active")
        && number(&p["ends_at"]).is_none_or(|t| t <= 0.0 || t * 1000.0 > now() as f64)
        && (id.is_empty() && name.is_empty()
            || [id, name]
                .iter()
                .any(|s| s.contains("start-plan") || s.contains("start plan")))
}

fn os_version() -> String {
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86_64") {
        "x64"
    } else {
        std::env::consts::ARCH
    };
    #[cfg(unix)]
    let release = rustix::system::uname()
        .release()
        .to_string_lossy()
        .into_owned();
    #[cfg(not(unix))]
    let release = String::from("unknown");
    format!("{} {release} {arch}", vendor_platform())
}
