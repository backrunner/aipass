use super::*;
use sha2::{Digest, Sha256};
const AUTH: &str = "https://prod.us-east-1.auth.desktop.kiro.dev";
fn region(c: &Context) -> Result<String> {
    let profile = s(&c.auth, "profileArn");
    let r = profile
        .split(':')
        .nth(3)
        .filter(|s| !s.is_empty())
        .unwrap_or(if s(&c.auth, "region").starts_with("eu-") {
            "eu-central-1"
        } else {
            "us-east-1"
        });
    if r.len() > 32
        || !r
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err("invalid Kiro region".into());
    }
    Ok(r.into())
}
fn signed(c: &Context, r: RequestBuilder) -> Result<RequestBuilder> {
    let mut r = r.bearer_auth(c.token()?);
    if c.auth["type"] == "api" {
        r = r.header("tokentype", "API_KEY");
    } else if c.auth["method"] == "external-idp" {
        r = r.header("tokentype", "EXTERNAL_IDP");
    }
    Ok(r)
}
async fn management(
    c: &Context,
    name: &str,
    body: Option<Value>,
    region_override: Option<&str>,
    query: &[(&str, &str)],
) -> Result<Value> {
    let region = region_override.map(str::to_owned).unwrap_or(region(c)?);
    let url = format!("https://management.{region}.kiro.dev/{name}");
    let r = if let Some(b) = body {
        c.client.post(url).json(&b)
    } else {
        c.client.get(url).query(query)
    };
    json_request(signed(c, r)?).await
}
fn cli_db() -> Result<std::path::PathBuf> {
    Ok(home()?.join(if cfg!(target_os = "macos") {
        "Library/Application Support/kiro-cli/data.sqlite3"
    } else if cfg!(windows) {
        "AppData/Roaming/kiro-cli/data.sqlite3"
    } else {
        ".local/share/kiro-cli/data.sqlite3"
    }))
}
fn native_db(source: &Value) -> Result<std::path::PathBuf> {
    Ok(native_import::root(source)
        .map(|p| p.join("data.sqlite3"))
        .unwrap_or(cli_db()?))
}
fn cli_executable() -> Option<std::path::PathBuf> {
    let name = if cfg!(windows) {
        "kiro-cli.exe"
    } else {
        "kiro-cli"
    };
    let mut dirs: Vec<_> =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
    if let Ok(home) = home() {
        dirs.extend([home.join(".local/bin"), home.join(".kiro/bin")]);
    }
    dirs.extend(["/opt/homebrew/bin".into(), "/usr/local/bin".into()]);
    dirs.into_iter()
        .map(|d| d.join(name))
        .find(|p| p.is_absolute() && p.is_file())
}
fn expiry(v: &Value) -> u64 {
    v.as_str()
        .and_then(|s| {
            time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
        })
        .map(|t| t.unix_timestamp().max(0) as u64 * 1000)
        .unwrap_or(0)
}
fn native(source: &Value) -> Result<Value> {
    let selected = native_import::selector(source);
    if selected != "ide" {
        if let Ok(db) = rusqlite::Connection::open_with_flags(
            native_db(source)?,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        ) {
            if selected.starts_with("cli:") {
                db.prepare("SELECT value FROM auth_kv WHERE key=?1")
                    .map_err(|_| "invalid Kiro CLI credential database")?;
            }
            let get = |key: &str| {
                db.query_row("SELECT value FROM auth_kv WHERE key=?1", [key], |r| {
                    r.get::<_, String>(0)
                })
                .ok()
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            };
            for kind in ["social", "odic", "external-idp"] {
                if !selected.is_empty() && selected != format!("cli:{kind}") {
                    continue;
                }
                let key = format!("kirocli:{kind}:token");
                if let Some(m) = get(&key).filter(|v| !s(v, "access_token").is_empty()) {
                    let mut a = json!({"type":"oauth","access":m["access_token"],"refresh":m["refresh_token"],"expires":expiry(&m["expires_at"]),"region":m["region"].as_str().unwrap_or("us-east-1"),"profileArn":m["profile_arn"],"method":if kind=="odic"{"idc"}else{kind},"source":"kiro-cli","dbKey":key});
                    if kind == "odic" {
                        if let Some(reg) = get("kirocli:odic:device-registration") {
                            a["clientId"] = reg["client_id"].clone();
                            a["clientSecret"] = reg["client_secret"].clone();
                        }
                    } else if kind == "external-idp" {
                        a["clientId"] = m["client_id"].clone();
                        a["tokenURL"] = json!(m["token_endpoint"]
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| format!(
                                "{}/v1/token",
                                s(&m, "issuer_url").trim_end_matches('/')
                            )));
                    }
                    return Ok(a);
                }
            }
        }
    }
    if selected.starts_with("cli:") {
        return Err("Kiro CLI is not signed in".into());
    }
    let dir = native_import::root(source).unwrap_or(home()?.join(".aws/sso/cache"));
    let m = read_json(&dir.join("kiro-auth-token.json"))?;
    if s(&m, "accessToken").is_empty() {
        return Err("Kiro is not signed in".into());
    }
    let mut a = json!({"type":"oauth","source":"kiro-ide","access":m["accessToken"],"refresh":m["refreshToken"],"expires":expiry(&m["expiresAt"]),"region":m["region"].as_str().unwrap_or("us-east-1"),"profileArn":m["profileArn"],"method":"social"});
    let hash = s(&m, "clientIdHash");
    if !hash.is_empty()
        && hash
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        if let Ok(reg) = read_json(&dir.join(format!("{hash}.json"))) {
            a["clientId"] = reg["clientId"].clone();
            a["clientSecret"] = reg["clientSecret"].clone();
            if !s(&m, "authMethod").eq_ignore_ascii_case("social") {
                a["method"] = json!("idc");
            }
        }
    }
    Ok(a)
}
pub(super) async fn import_profile(c: &Context) -> Result<String> {
    let arn = if c.auth["type"] == "api" {
        let v = json_request(signed(
            c,
            c.client
                .post("https://management.us-east-1.kiro.dev/")
                .header("Content-Type", "application/x-amz-json-1.0")
                .header("X-Amz-Target", "AmazonCodeWhispererService.GetProfile")
                .body("{}"),
        )?)
        .await?;
        s(&v["profile"], "arn").to_owned()
    } else {
        let mut arn = String::new();
        for r in ["us-east-1", "eu-central-1"] {
            if let Ok(v) =
                management(c, "List-Available-Profiles", Some(json!({})), Some(r), &[]).await
            {
                if let Some(a) = v["profiles"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find_map(|p| p["arn"].as_str())
                {
                    arn = a.into();
                    break;
                }
            }
        }
        arn
    };
    if arn.is_empty() {
        return Err("Kiro returned no account profile".into());
    }
    Ok(arn)
}
async fn profile(c: &mut Context) -> Result<()> {
    if !s(&c.auth, "profileArn").is_empty() {
        return Ok(());
    }
    let arn = import_profile(c).await?;
    let mut next = c.auth.clone();
    next["profileArn"] = json!(arn);
    c.save(next).await
}
fn native_matches(old: &Value, next: &Value) -> bool {
    let a = s(old, "profileArn");
    let b = s(next, "profileArn");
    if !a.is_empty() && !b.is_empty() {
        a == b
    } else {
        old["access"] == next["access"]
    }
}
fn grant_hash(auth: &Value) -> String {
    let mut hash = Sha256::new();
    hash.update(s(auth, "access").as_bytes());
    hash.update([0]);
    hash.update(s(auth, "refresh").as_bytes());
    format!("{:x}", hash.finalize())
}
fn previous_native_grant(current: &Value, native: &Value) -> bool {
    current["previousNativeGrantHash"] == grant_hash(native)
        || (current["previousNativeGrantHash"].is_null()
            && current["previousNativeAccessHash"]
                == format!("{:x}", Sha256::digest(s(native, "access").as_bytes()))
            && native["access"] != current["access"]
            && native["expires"].as_u64().unwrap_or(0) <= current["expires"].as_u64().unwrap_or(0))
}
fn live_native_rotation(current: &Value, native: &Value, at: u64) -> bool {
    !previous_native_grant(current, native)
        && (native["access"] != current["access"] || native["refresh"] != current["refresh"])
        && native["expires"].as_u64().is_some_and(|expiry| expiry > at)
}
async fn pin_native_profile(c: &Context, mut next: Value) -> Result<Value> {
    let expected = s(&c.auth, "profileArn");
    if !expected.is_empty() && s(&next, "profileArn").is_empty() {
        if next["access"] != c.auth["access"] {
            // CLI rotations may omit profile_arn. Prove the new token can see
            // this exact profile instead of trusting the shared database key.
            let region = region(c)?;
            let mut request = c
                .client
                .post(format!(
                    "https://management.{region}.kiro.dev/List-Available-Profiles"
                ))
                .bearer_auth(s(&next, "access"))
                .json(&json!({}));
            if next["method"] == "external-idp" {
                request = request.header("tokentype", "EXTERNAL_IDP");
            }
            let profiles = json_request(request).await?;
            if !profiles["profiles"]
                .as_array()
                .is_some_and(|p| p.iter().any(|p| p["arn"] == expected))
            {
                return Err("Kiro native account profile changed; reconnect explicitly".into());
            }
        }
        next["profileArn"] = json!(expected);
    }
    Ok(next)
}
fn save_back(before: &Value, next: &Value) -> Result<()> {
    let expires = time::OffsetDateTime::from_unix_timestamp(
        (next["expires"].as_u64().unwrap_or(0) / 1000) as i64,
    )
    .ok()
    .and_then(|t| {
        t.format(&time::format_description::well_known::Rfc3339)
            .ok()
    })
    .unwrap_or_default();
    match s(before, "source") {
        "kiro-cli" => {
            let mut db = rusqlite::Connection::open(native_db(before)?)
                .map_err(|_| "cannot update Kiro CLI sign-in")?;
            db.busy_timeout(Duration::from_secs(3))
                .map_err(|_| "Kiro CLI database unavailable")?;
            let tx = db
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .map_err(|_| "Kiro CLI account busy")?;
            let key = s(before, "dbKey");
            let raw: String = tx
                .query_row("SELECT value FROM auth_kv WHERE key=?1", [key], |r| {
                    r.get(0)
                })
                .map_err(|_| "Kiro CLI account changed")?;
            let mut v: Value =
                serde_json::from_str(&raw).map_err(|_| "invalid Kiro CLI account")?;
            if v["access_token"] != before["access"] || v["refresh_token"] != before["refresh"] {
                return Err(
                    "Kiro CLI account changed during refresh; new grant remains in AIPass".into(),
                );
            }
            v["access_token"] = next["access"].clone();
            v["refresh_token"] = next["refresh"].clone();
            v["expires_at"] = json!(expires);
            v["profile_arn"] = next["profileArn"].clone();
            tx.execute(
                "UPDATE auth_kv SET value=?1 WHERE key=?2",
                rusqlite::params![v.to_string(), key],
            )
            .map_err(|_| "cannot update Kiro CLI sign-in")?;
            tx.commit().map_err(|_| "cannot save Kiro CLI refresh")?;
        }
        "kiro-ide" => {
            let path = native_import::root(before)
                .unwrap_or(home()?.join(".aws/sso/cache"))
                .join("kiro-auth-token.json");
            let mut v = read_json(&path)?;
            if v["accessToken"] != before["access"] || v["refreshToken"] != before["refresh"] {
                return Err(
                    "Kiro IDE account changed during refresh; new grant remains in AIPass".into(),
                );
            }
            v["accessToken"] = next["access"].clone();
            v["refreshToken"] = next["refresh"].clone();
            v["expiresAt"] = json!(expires);
            let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())
                .map_err(|_| "cannot stage Kiro IDE refresh")?;
            use std::io::Write;
            file.write_all(v.to_string().as_bytes())
                .map_err(|_| "cannot stage Kiro IDE refresh")?;
            file.persist(&path)
                .map_err(|_| "cannot save Kiro IDE refresh")?;
        }
        _ => {}
    }
    Ok(())
}
pub(super) async fn login(c: &mut Context, method: usize) -> Result<Value> {
    if method == 1 {
        return native(&c.auth);
    }
    let cb = loopback::Loopback::bind(&[
        3128, 4649, 6588, 8008, 9091, 49153, 50153, 51153, 52153, 53153,
    ])
    .await?;
    let state = format!("{}{}", hex_id(), hex_id());
    let verifier = format!("{}{}", hex_id(), hex_id());
    let redirect = format!("http://localhost:{}", cb.port);
    let mut u = url::Url::parse("https://app.kiro.dev/signin").unwrap();
    u.query_pairs_mut()
        .append_pair("state", &state)
        .append_pair(
            "code_challenge",
            &URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
        )
        .append_pair("code_challenge_method", "S256")
        .append_pair("redirect_uri", &redirect)
        .append_pair("redirect_from", "KiroIDE");
    c.challenge(
        u.as_str(),
        "Sign in with Google, GitHub, AWS Builder ID, or IAM Identity Center.",
        false,
    )
    .await?;
    let got = cb
        .wait(&["https://app.kiro.dev"], |r| {
            r.method == "GET"
                && matches!(r.path.as_str(), "/signin/callback" | "/oauth/callback")
                && r.fields["state"] == state
        })
        .await?;
    let p = got.fields;
    let opt = s(&p, "login_option");
    let (tokens, mut a) = if matches!(opt, "google" | "github") {
        let t=json_request(c.client.post(format!("{AUTH}/oauth/token")).header("User-Agent","KiroIDE-1.1.70").json(&json!({"code":p["code"],"code_verifier":verifier,"redirect_uri":format!("{redirect}{}?login_option={opt}",got.path)}))).await?;
        (
            t,
            json!({"type":"oauth","method":"social","region":"us-east-1","loginProvider":opt}),
        )
    } else if matches!(opt, "builderid" | "awsidc" | "internal") {
        let region = s(&p, "idc_region");
        if region.is_empty()
            || region.len() > 32
            || !region
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err("invalid AWS sign-in region".into());
        }
        let scopes = [
            "codewhisperer:completions",
            "codewhisperer:analysis",
            "codewhisperer:conversations",
            "codewhisperer:transformations",
            "codewhisperer:taskassist",
        ];
        let oidc = format!("https://oidc.{region}.amazonaws.com");
        let reg=json_request(c.client.post(format!("{oidc}/client/register")).json(&json!({"clientName":"Kiro IDE","clientType":"public","scopes":scopes,"grantTypes":["authorization_code","refresh_token"],"redirectUris":["http://127.0.0.1/oauth/callback"],"issuerUrl":p["issuer_url"]}))).await?;
        let state = format!("{}{}", hex_id(), hex_id());
        let verifier = format!("{}{}", hex_id(), hex_id());
        let redirect = format!("http://127.0.0.1:{}/oauth/callback", cb.port);
        let mut u = url::Url::parse(&format!("{oidc}/authorize")).unwrap();
        u.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", s(&reg, "clientId"))
            .append_pair("redirect_uri", &redirect)
            .append_pair("scopes", &scopes.join(","))
            .append_pair("state", &state)
            .append_pair(
                "code_challenge",
                &URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
            )
            .append_pair("code_challenge_method", "S256");
        c.challenge(u.as_str(), "Continue the sign-in with AWS.", false)
            .await?;
        let p = cb
            .wait(&[], |r| {
                r.method == "GET" && r.path == "/oauth/callback" && r.fields["state"] == state
            })
            .await?
            .fields;
        let t=json_request(c.client.post(format!("{oidc}/token")).json(&json!({"clientId":reg["clientId"],"clientSecret":reg["clientSecret"],"grantType":"authorization_code","redirectUri":redirect,"code":p["code"],"codeVerifier":verifier}))).await?;
        (
            t,
            json!({"type":"oauth","method":"idc","region":region,"clientId":reg["clientId"],"clientSecret":reg["clientSecret"]}),
        )
    } else {
        return Err(
            "For an external identity provider, sign in with kiro-cli and import its account"
                .into(),
        );
    };
    if s(&tokens, "accessToken").is_empty() {
        return Err("Kiro returned no access token".into());
    }
    a["access"] = tokens["accessToken"].clone();
    a["refresh"] = tokens["refreshToken"].clone();
    a["expires"] = json!(expires_after(&tokens["expiresIn"], 3600, 1000));
    a["profileArn"] = tokens["profileArn"].clone();
    Ok(a)
}
pub(super) async fn fresh(c: &mut Context) -> Result<()> {
    fresh_inner(c, false).await
}
async fn fresh_inner(c: &mut Context, mut force: bool) -> Result<()> {
    let mut owner_has_current_grant = false;
    if matches!(s(&c.auth, "source"), "kiro-cli" | "kiro-ide") {
        let mut n = native(&c.auth)?;
        if !previous_native_grant(&c.auth, &n) {
            n = pin_native_profile(c, n).await?;
            if !native_matches(&c.auth, &n) {
                return Err("Kiro native account changed; reconnect explicitly".into());
            }
            owner_has_current_grant =
                c.auth["access"] == n["access"] && c.auth["refresh"] == n["refresh"];
            if live_native_rotation(&c.auth, &n, now()) {
                n["accountId"] = c.auth["accountId"].clone();
                if s(&n, "profileArn").is_empty() {
                    n["profileArn"] = c.auth["profileArn"].clone();
                }
                c.save(n).await?;
                force = false; // refusal applied to the previous access token
                owner_has_current_grant = true;
            }
        }
    }
    if c.auth["type"] != "api"
        && (force
            || (c.auth["expires"].as_u64().unwrap_or(0) > 0
                && c.auth["expires"].as_u64().unwrap_or(0) <= now() + 120000))
    {
        let _refresh = c.refresh_guard()?;
        let before = c.auth.clone();
        if before["source"] == "kiro-cli" && owner_has_current_grant {
            if let Some(exe) = cli_executable() {
                // Let the CLI serialize rotation of the grant in its database.
                let mut command = native_cli::command(&exe, c);
                command.args(["debug", "refresh-auth-token"]);
                let _ = native_cli::run(command, 20).await;
                let mut n = pin_native_profile(c, native(&c.auth)?).await?;
                if !native_matches(&before, &n) || n["dbKey"] != before["dbKey"] {
                    return Err(
                        "Kiro CLI account changed during refresh; reconnect explicitly".into(),
                    );
                }
                if live_native_rotation(&before, &n, now()) {
                    n["accountId"] = before["accountId"].clone();
                    c.save(n).await?;
                    return profile(c).await;
                }
            }
        }
        let region = before["region"].as_str().unwrap_or("us-east-1");
        if !region
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err("invalid Kiro region".into());
        }
        let req=match s(&before,"method"){
            "idc"=>c.client.post(format!("https://oidc.{region}.amazonaws.com/token")).json(&json!({"clientId":before["clientId"],"clientSecret":before["clientSecret"],"refreshToken":before["refresh"],"grantType":"refresh_token"})),
            "external-idp"=>{let url=url::Url::parse(s(&before,"tokenURL")).map_err(|_|"invalid Kiro identity provider endpoint")?;if url.scheme()!="https"||!url.username().is_empty()||url.password().is_some(){return Err("invalid Kiro identity provider endpoint".into());}c.client.post(url).form(&[("grant_type","refresh_token"),("client_id",s(&before,"clientId")),("refresh_token",s(&before,"refresh"))])},
            _=>c.client.post(format!("https://prod.{region}.auth.desktop.kiro.dev/refreshToken")).header("User-Agent","Kiro-Desktop/0.2.13 (darwin; arm64)").json(&json!({"refreshToken":before["refresh"]}))};
        let t = json_request(req).await?;
        let external = before["method"] == "external-idp";
        let access = s(
            &t,
            if external {
                "access_token"
            } else {
                "accessToken"
            },
        );
        if access.is_empty() {
            return Err("Kiro sign-in expired; reconnect".into());
        }
        let mut next = before.clone();
        next["access"] = json!(access);
        let refresh = s(
            &t,
            if external {
                "refresh_token"
            } else {
                "refreshToken"
            },
        );
        if !refresh.is_empty() {
            next["refresh"] = json!(refresh);
        }
        next["expires"] = json!(expires_after(
            &t[if external { "expires_in" } else { "expiresIn" }],
            3600,
            1000
        ));
        if !s(&t, "profileArn").is_empty() {
            if !s(&before, "profileArn").is_empty() && before["profileArn"] != t["profileArn"] {
                return Err("Kiro refresh changed account profile".into());
            }
            next["profileArn"] = t["profileArn"].clone();
        }
        next["previousNativeAccessHash"] = json!(format!(
            "{:x}",
            Sha256::digest(s(&before, "access").as_bytes())
        ));
        next["previousNativeGrantHash"] = json!(grant_hash(&before));
        c.save(next.clone()).await?;
        save_back(&before, &next)?;
    }
    profile(c).await
}
pub(super) async fn models(c: &mut Context) -> Result<Value> {
    let v = management(
        c,
        "List-Available-Models",
        None,
        None,
        &[
            ("origin", "KIRO_CLI"),
            ("profileArn", s(&c.auth, "profileArn")),
        ],
    )
    .await?;
    let mut out = json!({});
    for m in v["models"].as_array().into_iter().flatten() {
        let id = s(m, "modelId");
        if id.is_empty() {
            continue;
        }
        let mut item = model(
            id,
            m["modelName"].as_str().unwrap_or(id),
            "anthropic",
            "https://runtime.us-east-1.kiro.dev",
            m["tokenLimits"]["maxInputTokens"].as_u64().unwrap_or(0),
            m["tokenLimits"]["maxOutputTokens"].as_u64().unwrap_or(0),
        );
        item["reasoning"] = json!(id.contains("claude") || id == "auto");
        item["attachment"] = json!(m["supportedInputTypes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .any(|s| s.eq_ignore_ascii_case("image")));
        out[id] = item;
    }
    Ok(out)
}
pub(super) async fn usage(c: &mut Context) -> Result<Value> {
    let v = management(
        c,
        "Get-Usage-Limits",
        None,
        None,
        &[
            ("origin", "KIRO_CLI"),
            ("profileArn", s(&c.auth, "profileArn")),
            ("resourceType", "CREDIT"),
            ("isEmailRequired", "true"),
        ],
    )
    .await?;
    let mut windows = vec![];
    for u in v["usageBreakdownList"].as_array().into_iter().flatten() {
        for (name, x, reset) in [
            (
                u["displayNamePlural"].as_str().unwrap_or("Credits"),
                u,
                u["nextDateReset"].as_f64().or(v["nextDateReset"].as_f64()),
            ),
            (
                "Free trial",
                &u["freeTrialInfo"],
                u["freeTrialInfo"]["freeTrialExpiry"].as_f64(),
            ),
        ] {
            if name == "Free trial" && x["freeTrialStatus"] != "ACTIVE" {
                continue;
            }
            if let Some(limit) = x["usageLimitWithPrecision"].as_f64().filter(|n| *n > 0.0) {
                let used = x["currentUsageWithPrecision"].as_f64().unwrap_or(0.0);
                let at = reset
                    .and_then(|n| time::OffsetDateTime::from_unix_timestamp(n as i64).ok())
                    .and_then(|t| {
                        t.format(&time::format_description::well_known::Rfc3339)
                            .ok()
                    });
                windows.push(json!({"name":name,"used":100.0*used/limit,"display":format!("{used:.2} / {limit}"),"resetsAt":at,"span":2592000}));
            }
        }
    }
    Ok(json!({"plan":v["subscriptionInfo"]["subscriptionTitle"],"windows":windows}))
}
pub(super) async fn generate(c: &mut Context, m: &Value, body: Value) -> Result<()> {
    use aipass_proxy_conversion::providers::kiro::{build_request, KiroStream};
    let model = s(&body, "model");
    let (req, budget) = build_request(&body, model, s(&c.auth, "profileArn"), &c.session)?;
    let mut response = None;
    for attempt in 0..2 {
        let region = region(c)?;
        let ua=format!("aws-sdk-rust/1.0.0 ua/2.1 os/other lang/rust api/codewhispererstreaming#1.28.3 m/E app/AmazonQ-For-CLI md/appVersion-1.28.3-{}",hex_id());
        let result = send(
            signed(
                c,
                c.client.post(format!(
                    "https://runtime.{region}.kiro.dev/generateAssistantResponse"
                )),
            )?
            .header("Accept", "application/vnd.amazon.eventstream")
            .header("x-amzn-codewhisperer-optout", "true")
            .header("amz-sdk-invocation-id", uuid::Uuid::new_v4().to_string())
            .header("amz-sdk-request", "attempt=1; max=1")
            .header("x-amzn-kiro-agent-mode", "vibe")
            .header("x-amz-user-agent", &ua)
            .header("User-Agent", ua)
            .json(&req),
        )
        .await?;
        if attempt == 0 && result.status() == 403 && c.auth["type"] != "api" {
            fresh_inner(c, true).await?;
            continue;
        }
        response = Some(result);
        break;
    }
    let response = response.ok_or("Kiro token refused")?;
    let mut converter = KiroStream::new(
        model,
        &format!("msg_{}", hex_id()),
        budget,
        m["limit"]["context"].as_u64().unwrap_or(0),
    );
    c.converted(response, &mut converter).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_generation_binds_both_tokens_and_preserves_profile_ownership() {
        let native = json!({"access":"same-access","refresh":"old-refresh","profileArn":"arn:aws:kiro:us-east-1:alice:profile/main","expires":100});
        let mut current = native.clone();
        current["previousNativeGrantHash"] = json!(grant_hash(&native));
        assert!(previous_native_grant(&current, &native));
        let mut rotated = native.clone();
        rotated["refresh"] = json!("new-refresh");
        assert!(!previous_native_grant(&current, &rotated));
        assert!(live_native_rotation(&current, &rotated, 50));
        rotated["expires"] = json!(75);
        assert!(live_native_rotation(&current, &rotated, 50));
        assert!(!live_native_rotation(&current, &rotated, 75));
        assert!(!live_native_rotation(&current, &native, 50));
        assert!(native_matches(&current, &rotated));
        rotated["profileArn"] = json!("arn:aws:kiro:us-east-1:bob:profile/main");
        assert!(!native_matches(&current, &rotated));
    }
}
