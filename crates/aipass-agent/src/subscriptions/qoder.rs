use super::*;
use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
use rsa::{pkcs8::DecodePublicKey, Pkcs1v15Encrypt, RsaPublicKey};
use sha2::{Digest, Sha256};
const CHAT_PATH:&str="/algo/api/v2/service/pro/sse/agent_chat_generation?FetchKeys=llm_model_result&AgentId=agent_common&Encode=1";
fn site(c: &Context) -> (&'static str, &'static str, &'static str, &'static str) {
    if c.provider == "qoder-cn" {
        (
            "https://qoder.cn",
            "https://openapi.qoder.com.cn",
            "https://gateway.qoder.com.cn",
            "e883ade2-e6e3-4d6d-adf7-f92ceff5fdcb",
        )
    } else {
        (
            "https://qoder.com",
            "https://openapi.qoder.sh",
            "https://api3.qoder.sh",
            "732aef47-9cf2-46a2-95fe-4cebb5d0d1fa",
        )
    }
}
const RSA_KEY:&str="-----BEGIN PUBLIC KEY-----\nMIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQDA8iMH5c02LilrsERw9t6Pv5Nc\n4k6Pz1EaDicBMpdpxKduSZu5OANqUq8er4GM95omAGIOPOh+Nx0spthYA2BqGz+l\n6HRkPJ7S236FZz73In/KVuLnwI8JJ2CbuJap8kvheCCZpmAWpb/cPx/3Vr/J6I17\nXcW+ML9FoCI6AOvOzwIDAQAB\n-----END PUBLIC KEY-----";
fn cosy(c: &Context, url: &str, body: &str, req: RequestBuilder) -> Result<RequestBuilder> {
    let machine = s(&c.auth, "machineId");
    if machine.is_empty() {
        return Err("Qoder account has no machine ID; reconnect".into());
    }
    let id = hex_id();
    let key = &id.as_bytes()[..16];
    let raw=json!({"uid":c.auth["uid"],"aid":"","name":c.auth["name"].as_str().unwrap_or(""),"email":c.auth["email"].as_str().unwrap_or(""),"security_oauth_token":c.token()?}).to_string();
    let mut buf = raw.as_bytes().to_vec();
    buf.resize(buf.len() + 16, 0);
    let encrypted = cbc::Encryptor::<aes::Aes128>::new_from_slices(key, key)
        .map_err(|_| "Qoder encryption failed")?
        .encrypt_padded_mut::<Pkcs7>(&mut buf, raw.len())
        .map_err(|_| "Qoder encryption failed")?;
    let info = STANDARD.encode(encrypted);
    let rsa =
        RsaPublicKey::from_public_key_pem(RSA_KEY).map_err(|_| "invalid Qoder signing key")?;
    let wrapped = STANDARD.encode(
        rsa.encrypt(&mut rand_core::OsRng, Pkcs1v15Encrypt, key)
            .map_err(|_| "Qoder key wrapping failed")?,
    );
    let payload=STANDARD.encode(json!({"version":"v1","requestId":hex_id(),"info":info,"cosyVersion":"1.1.49","ideVersion":""}).to_string());
    let path = url::Url::parse(url)
        .map_err(|_| "invalid Qoder URL")?
        .path()
        .trim_start_matches("/algo")
        .to_owned();
    let ts = now() / 1000;
    let signature = format!(
        "{:x}",
        md5::compute(format!("{payload}\n{wrapped}\n{ts}\n{body}\n{path}"))
    );
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(windows) {
        "win32"
    } else {
        "linux"
    };
    Ok(req
        .header("Accept", "application/json")
        .header("Accept-Encoding", "identity")
        .header("Content-Type", "application/json")
        .header(
            "Authorization",
            format!("Bearer COSY.{payload}.{signature}"),
        )
        .header("Cosy-Business-Product", "app")
        .header("Cosy-Business-Type", "agent")
        .header("Cosy-ClientIp", machine)
        .header("Cosy-ClientType", "10")
        .header("Cosy-Data-Policy", "disagree")
        .header("Cosy-Date", ts.to_string())
        .header("Cosy-Key", wrapped)
        .header("Cosy-MachineId", machine)
        .header("Cosy-MachineToken", machine)
        .header("Cosy-MachineType", "5")
        .header("Cosy-MachineOS", format!("{}_{os}", std::env::consts::ARCH))
        .header("Cosy-Scene", "app")
        .header(
            "Cosy-User",
            c.auth["uid"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| c.auth["uid"].to_string()),
        )
        .header("Cosy-Version", "1.1.49")
        .header("Login-Version", "v2"))
}
fn device_expiry(v: &Value) -> u64 {
    v["expires_at"]
        .as_str()
        .and_then(|s| {
            time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
        })
        .map(|t| t.unix_timestamp().max(0) as u64 * 1000)
        .unwrap_or_else(|| expires_after(&v["expires_in"], 86400, 1000))
}
pub(super) async fn login(c: &mut Context, _: usize) -> Result<Value> {
    let (web, api, _, client) = site(c);
    let verifier = format!("{}{}", hex_id(), hex_id());
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let nonce = uuid::Uuid::new_v4().to_string();
    let machine = uuid::Uuid::new_v4().to_string();
    let mut url = url::Url::parse(&format!("{web}/device/selectAccounts")).unwrap();
    url.query_pairs_mut().extend_pairs([
        ("challenge", challenge.as_str()),
        ("challenge_method", "S256"),
        ("nonce", &nonce),
        ("machine_id", &machine),
        ("client_id", client),
    ]);
    if c.provider == "qoder" {
        url.query_pairs_mut()
            .append_pair("redirect_uri", "qoder-app://");
    }
    c.challenge(
        url.as_str(),
        "Sign in to Qoder and authorize this device.",
        false,
    )
    .await?;
    let deadline = now() + 900000;
    while now() < deadline {
        let result = raw_json(
            c.client
                .get(format!("{api}/api/v1/deviceToken/poll"))
                .query(&[
                    ("nonce", nonce.as_str()),
                    ("verifier", &verifier),
                    ("challenge_method", "S256"),
                ]),
        )
        .await;
        if let Ok((200, dt)) = result {
            if !s(&dt, "token").is_empty() {
                let (status, jt) = raw_json(
                    c.client
                        .post(format!("{api}/api/v1/me/jobToken"))
                        .bearer_auth(s(&dt, "token"))
                        .json(&json!({"clientId":client})),
                )
                .await?;
                let device =
                    status != 200 && c.provider == "qoder-cn" && (400..500).contains(&status);
                if status != 200 && !device {
                    return Err(format!("Qoder job token returned HTTP {status}"));
                }
                let chat = if device { &dt } else { &jt };
                if s(chat, "token").is_empty() {
                    return Err("Qoder returned no chat token".into());
                }
                let who = json_request(
                    c.client
                        .get(format!("{api}/api/v1/userinfo"))
                        .bearer_auth(s(&dt, "token")),
                )
                .await
                .unwrap_or(json!({}));
                return Ok(
                    json!({"type":"oauth","access":chat["token"],"refresh":chat["refresh_token"],"expires":if device{device_expiry(chat)}else{expires_after(&chat["expires_in"],86400000,1)},"deviceChat":device,"accountId":dt["user_id"],"uid":dt["user_id"],"email":who["email"],"name":who["name"].as_str().or(dt["user_name"].as_str()).unwrap_or(""),"machineId":machine,"deviceToken":dt["token"],"deviceRefresh":dt["refresh_token"]}),
                );
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    Err("Qoder sign-in timed out".into())
}
pub(super) async fn fresh(c: &mut Context) -> Result<()> {
    if c.auth["expires"].as_u64().unwrap_or(0) > now() + 300000 {
        return Ok(());
    }
    let device = c.auth["deviceChat"] == true;
    let _refresh = c.refresh_guard()?;
    let v = json_request(
        c.client
            .post(format!(
                "{}/api/v1/{}/refresh",
                site(c).1,
                if device { "deviceToken" } else { "jobToken" }
            ))
            .json(&json!({"refresh_token":c.auth["refresh"]})),
    )
    .await?;
    let token = v["token"]
        .as_str()
        .or(v["device_token"].as_str())
        .ok_or("Qoder refresh returned no access token")?;
    if s(&v, "refresh_token").is_empty() {
        return Err("Qoder refresh returned no refresh token".into());
    }
    let mut next = c.auth.clone();
    next["access"] = json!(token);
    next["refresh"] = v["refresh_token"].clone();
    next["expires"] = json!(if device {
        device_expiry(&v)
    } else {
        expires_after(&v["expires_in"], 86400000, 1)
    });
    if device {
        next["deviceToken"] = next["access"].clone();
        next["deviceRefresh"] = next["refresh"].clone();
    }
    c.save(next).await
}
pub(super) async fn models(c: &mut Context) -> Result<Value> {
    let url = format!("{}/algo/api/v2/model/list?Encode=1", site(c).2);
    let list = json_request(cosy(c, &url, "", c.client.get(&url))?).await?;
    let mut out = json!({});
    for raw in list["chat"].as_array().into_iter().flatten() {
        let id = s(raw, "key");
        if raw["enable"] != true || matches!(id, "" | "auto" | "default") {
            continue;
        }
        let mut item = model(
            id,
            raw["display_name"].as_str().unwrap_or(id),
            "chat",
            site(c).2,
            raw["max_input_tokens"].as_u64().unwrap_or(0),
            0,
        );
        item["qoder_config"] = raw.clone();
        item["attachment"] = json!(raw["is_vl"] == true);
        item["reasoning"] =
            json!(raw["is_reasoning"] == true || !raw["thinking_config"]["enabled"].is_null());
        item["variants"] = json!({});
        for (e, _) in raw["thinking_config"]["enabled"]["efforts"]
            .as_object()
            .into_iter()
            .flatten()
        {
            item["variants"][e] = json!({"reasoningEffort":e});
        }
        let price = raw["price_factor"].as_f64().or(raw["priceFactor"].as_f64());
        let promotion = raw
            .get("promotion")
            .or(raw.get("prommotion"))
            .unwrap_or(&Value::Null);
        item["free"] = json!(if let Some(price) = price {
            price == 0.0
                && !(promotion["active"] == true
                    && promotion["before_promotion_price_factor"]
                        .as_f64()
                        .or(promotion["beforePromotionPriceFactor"].as_f64())
                        .is_some_and(|v| v > 0.0))
        } else {
            raw["is_free"] == true || raw["isFree"] == true
        });
        out[id] = item;
    }
    Ok(out)
}
pub(super) async fn usage(c: &mut Context) -> Result<Value> {
    let api = site(c).1;
    let mut env = None;
    for attempt in 0..2 {
        let (status, v) = raw_json(
            c.client
                .get(format!("{api}/sash/api/v2/me/usage"))
                .bearer_auth(s(&c.auth, "deviceToken"))
                .header("Cosy-ClientType", "10")
                .header("User-Agent", "Qoder"),
        )
        .await?;
        if attempt == 0 && [401, 403].contains(&status) {
            let _refresh = c.refresh_guard()?;
            let dt = json_request(
                c.client
                    .post(format!("{api}/api/v1/deviceToken/refresh"))
                    .json(&json!({"refresh_token":c.auth["deviceRefresh"]})),
            )
            .await?;
            let mut next = c.auth.clone();
            next["deviceToken"] = dt["token"]
                .as_str()
                .or(dt["device_token"].as_str())
                .map(|s| json!(s))
                .ok_or("Qoder device refresh returned no token")?;
            if s(&dt, "refresh_token").is_empty() {
                return Err("Qoder device refresh returned no refresh token".into());
            }
            next["deviceRefresh"] = dt["refresh_token"].clone();
            if next["deviceChat"] == true {
                next["access"] = next["deviceToken"].clone();
                next["refresh"] = next["deviceRefresh"].clone();
                next["expires"] = json!(device_expiry(&dt));
            }
            c.save(next).await?;
            continue;
        }
        if status != 200 {
            return Err(format!("Qoder usage returned HTTP {status}"));
        }
        env = Some(v);
        break;
    }
    let env = env.ok_or("Qoder account-page credentials expired")?;
    if env["displayMode"] == "enterprise" {
        return Ok(json!({"plan":"Enterprise","windows":[]}));
    }
    let u = &env["qoderUsage"];
    if env["displayMode"] != "qoder" || !u.is_object() {
        return Err("Qoder returned no quota data".into());
    }
    let mut windows = vec![];
    let mut add = |b: &Value, name: &str| {
        let total = b["total"].as_f64().or(b["cap"].as_f64());
        if let Some(t) = total.filter(|n| *n > 0.0) {
            if let Some(used) = b["used"]
                .as_f64()
                .or_else(|| b["remaining"].as_f64().map(|n| t - n))
                .filter(|n| *n >= 0.0)
            {
                windows.push(json!({"name":b["name"].as_str().unwrap_or(name),"used":(100.0*used/t).min(100.0),"display":format!("{used} / {t} credits")}));
            }
        }
    };
    for (a, b, name) in [
        ("userQuota", "user_quota", "Credits"),
        ("addOnQuota", "add_on_quota", "Add-on credits"),
        (
            "orgResourcePackage",
            "org_resource_package",
            "Shared credits",
        ),
    ] {
        add(u.get(a).or(u.get(b)).unwrap_or(&Value::Null), name);
    }
    for b in u["dedicatedResourcePackages"]
        .as_array()
        .or(u["dedicated_resource_packages"].as_array())
        .into_iter()
        .flatten()
    {
        add(b, "Dedicated credits");
    }
    Ok(json!({"plan":u["userType"].as_str().or(u["user_type"].as_str()),"windows":windows}))
}
pub(super) async fn generate(c: &mut Context, m: &Value, body: Value) -> Result<()> {
    use aipass_proxy_conversion::providers::qoder::{encode_body, request, QoderStream};
    let mut config = m["qoder_config"].clone();
    if !config.is_object() {
        let live = models(c).await?;
        config = live[s(&body, "model")]["qoder_config"].clone();
    }
    if !config.is_object() {
        return Err("Qoder model is unknown or disabled".into());
    }
    let body_text = request(&body, &config, &c.session, now())?.to_string();
    let wire = encode_body(body_text.as_bytes());
    let url = format!("{}{CHAT_PATH}", site(c).2);
    let response = send(
        cosy(c, &url, &wire, c.client.post(&url))?
            .header("Accept", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .header("X-Model-Key", s(&config, "key"))
            .header("X-Model-Source", s(&config, "source"))
            .body(wire),
    )
    .await?;
    let mut converter = QoderStream::new(s(&body, "model"), &format!("chatcmpl-{}", hex_id()));
    c.converted(response, &mut converter).await
}
