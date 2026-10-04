use super::*;
use rsa::{pkcs1::EncodeRsaPublicKey, Oaep, Pkcs1v15Encrypt, RsaPrivateKey, RsaPublicKey};
const CLOUD: &str = "https://cloud.zed.dev";
fn ua() -> String {
    format!(
        "Zed/1.23.0 ({}; {})",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}
fn state(c: &Context) -> Result<Value> {
    serde_json::from_str(s(&c.auth, "refresh")).map_err(|_| "invalid Zed account".into())
}
fn cloud(c: &Context, method: reqwest::Method, path: &str) -> Result<RequestBuilder> {
    let st = state(c)?;
    Ok(c.client
        .request(method, format!("{CLOUD}{path}"))
        .header(
            "Authorization",
            format!("{} {}", s(&st, "userId"), c.token()?),
        )
        .header("x-zed-system-id", s(&st, "systemId"))
        .header("User-Agent", ua()))
}
fn org(me: &Value) -> Value {
    me["organizations"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|o| o["id"] == me["default_organization_id"])
        .or(me["organizations"].get(0))
        .and_then(|o| o.get("id"))
        .or(me.get("default_organization_id"))
        .cloned()
        .unwrap_or(json!(""))
}
pub(super) async fn login(c: &mut Context, _: usize) -> Result<Value> {
    let key = RsaPrivateKey::new(&mut rand_core::OsRng, 2048)
        .map_err(|_| "cannot create Zed sign-in key")?;
    let der = RsaPublicKey::from(&key)
        .to_pkcs1_der()
        .map_err(|_| "cannot encode Zed sign-in key")?;
    let public = base64::engine::general_purpose::URL_SAFE.encode(der.as_bytes());
    let system = uuid::Uuid::new_v4().to_string();
    let callback = loopback::Loopback::bind(&[0]).await?;
    let mut url = url::Url::parse("https://zed.dev/native_app_signin").unwrap();
    url.query_pairs_mut()
        .append_pair("native_app_port", &callback.port.to_string())
        .append_pair("native_app_public_key", &public)
        .append_pair("system_id", &system);
    c.challenge(url.as_str(), "Sign in to Zed in the browser.", false)
        .await?;
    let got = callback
        .wait(&["https://zed.dev"], |cb| {
            cb.method == "GET"
                && !s(&cb.fields, "user_id").is_empty()
                && !s(&cb.fields, "access_token").is_empty()
        })
        .await?
        .fields;
    let ciphertext = URL_SAFE_NO_PAD
        .decode(s(&got, "access_token").trim_end_matches('='))
        .map_err(|_| "invalid encrypted Zed token")?;
    let plaintext = key
        .decrypt(Oaep::new::<sha2::Sha256>(), &ciphertext)
        .or_else(|_| key.decrypt(Pkcs1v15Encrypt, &ciphertext))
        .map_err(|_| "Zed sign-in token could not be decrypted")?;
    let access = String::from_utf8(plaintext).map_err(|_| "invalid Zed token")?;
    let mut st = json!({"userId":got["user_id"],"systemId":system});
    c.auth = json!({"type":"oauth","access":access,"refresh":st.to_string(),"expires":0});
    let me = json_request(cloud(c, reqwest::Method::GET, "/client/users/me")?).await?;
    let org = org(&me);
    if me["configuration_by_organization"][identifier(&org)]["is_zed_model_provider_enabled"]
        == false
    {
        return Err("Zed hosted models are disabled by this organization".into());
    }
    st["org"] = json!(org);
    let mut auth = c.auth.clone();
    auth["refresh"] = json!(st.to_string());
    auth["accountId"] = got["user_id"].clone();
    Ok(auth)
}
pub(super) async fn fresh(_: &mut Context) -> Result<()> {
    Ok(())
}
async fn token(c: &Context) -> Result<String> {
    let st = state(c)?;
    let v = json_request(
        cloud(c, reqwest::Method::POST, "/client/llm_tokens")?
            .json(&json!({"organization_id":st["org"]})),
    )
    .await?;
    v["token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "Zed returned no model token".into())
}
pub(super) async fn models(c: &mut Context) -> Result<Value> {
    let token = token(c).await?;
    let v = json_request(
        c.client
            .get(format!("{CLOUD}/models"))
            .bearer_auth(token)
            .header("x-zed-client-supports-x-ai", "true")
            .header("User-Agent", ua()),
    )
    .await?;
    let mut out = json!({});
    for m in v["models"].as_array().into_iter().flatten() {
        if m["is_disabled"] == true {
            continue;
        }
        let wire = match s(m, "provider") {
            "anthropic" => "anthropic",
            "open_ai" => "responses",
            "x_ai" => "chat",
            "google" => "gemini",
            _ => continue,
        };
        let id = s(m, "id");
        if id.is_empty() {
            continue;
        }
        let mut item = model(
            id,
            m["display_name"].as_str().unwrap_or(id),
            wire,
            CLOUD,
            m["max_token_count"].as_u64().unwrap_or(0),
            m["max_output_tokens"].as_u64().unwrap_or(0),
        );
        item["attachment"] = json!(m["supports_images"] == true);
        item["reasoning"] = json!(m["supports_thinking"] == true);
        out[id] = item;
    }
    Ok(out)
}
pub(super) async fn usage(c: &mut Context) -> Result<Value> {
    let me = json_request(cloud(c, reqwest::Method::GET, "/client/users/me")?).await?;
    let st = state(c)?;
    let org = if identifier(&st["org"]).is_empty() {
        org(&me)
    } else {
        st["org"].clone()
    };
    let plan = me["plans_by_organization"][identifier(&org)]
        .as_str()
        .or(me["plan"]["plan_v3"].as_str())
        .unwrap_or("zed_free");
    let mut out = json!({"plan":plan.trim_start_matches("zed_"),"until":me["plan"]["subscription_period"]["ended_at"],"windows":[]});
    if me["plan"]["has_overdue_invoices"] == true {
        out["error"] =
            json!("Zed has paused hosted models because this account has an overdue invoice");
    }
    Ok(out)
}
pub(super) async fn generate(c: &mut Context, m: &Value, body: Value) -> Result<()> {
    use aipass_proxy_conversion::providers::zed::{request, ZedStream};
    let wire = s(m, "wire");
    let model = body["model"]
        .as_str()
        .or(m["api"]["id"].as_str())
        .ok_or("Zed model missing")?;
    let request = request(wire, model, body.clone())?;
    let mut response = None;
    for attempt in 0..2 {
        let token = token(c).await?;
        let res = send(
            c.client
                .post(format!("{CLOUD}/completions"))
                .bearer_auth(token)
                .header("x-zed-version", "1.23.0")
                .header("x-zed-client-supports-status-messages", "true")
                .header(
                    "x-zed-client-supports-stream-ended-request-completion-status",
                    "true",
                )
                .header("User-Agent", ua())
                .json(&request),
        )
        .await?;
        if attempt == 0
            && (res.status() == 401
                || res.headers().contains_key("x-zed-expired-token")
                || res.headers().contains_key("x-zed-outdated-token"))
        {
            continue;
        }
        response = Some(res);
        break;
    }
    let response = response.ok_or("Zed model token refused")?;
    let mut converter = ZedStream::new(
        wire,
        response
            .headers()
            .contains_key("x-zed-server-supports-status-messages"),
    );
    c.converted(response, &mut converter).await
}
