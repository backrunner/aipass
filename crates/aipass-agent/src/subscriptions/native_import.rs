//! Provider-local discovery, without interactive login or credential rotation.
use super::*;
use aipass_agent_protocol::{SensitiveString, SubscriptionImportSource};
use std::path::PathBuf;

pub(crate) fn root(auth: &Value) -> Option<PathBuf> {
    auth["nativeSource"]["root"].as_str().map(PathBuf::from)
}
pub(crate) fn selector(auth: &Value) -> &str {
    auth["nativeSource"]["selector"].as_str().unwrap_or("")
}
pub(crate) fn method(provider: &str) -> usize {
    if provider == "zcode" {
        2
    } else {
        1
    }
}

pub(crate) fn defaults(provider: &str) -> Result<Vec<SubscriptionImportSource>> {
    let h = home()?;
    let root = match provider {
        "zcode" => h.join(".zcode/v2"),
        "devin" => {
            if cfg!(windows) {
                std::env::var_os("APPDATA")
                    .map(PathBuf::from)
                    .unwrap_or(h.join("AppData/Roaming"))
                    .join("devin")
            } else {
                std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .unwrap_or(h.join(".local/share"))
                    .join("devin")
            }
        }
        "commandcode-plan" => h.join(".commandcode"),
        "cursor" => {
            if cfg!(target_os = "macos") {
                h.join(".cursor")
            } else if cfg!(windows) {
                std::env::var_os("APPDATA")
                    .map(PathBuf::from)
                    .unwrap_or(h.join("AppData/Roaming"))
                    .join("Cursor")
            } else {
                std::env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .unwrap_or(h.join(".config"))
                    .join("cursor")
            }
        }
        "kiro" => h.join(if cfg!(target_os = "macos") {
            "Library/Application Support/kiro-cli"
        } else if cfg!(windows) {
            "AppData/Roaming/kiro-cli"
        } else {
            ".local/share/kiro-cli"
        }),
        "workbuddy" | "workbuddy-ai" => h
            .join(if cfg!(target_os = "macos") {
                "Library/Application Support/CodeBuddyExtension"
            } else if cfg!(windows) {
                "AppData/Local/CodeBuddyExtension"
            } else {
                ".local/share/CodeBuddyExtension"
            })
            .join("Data/Public/auth"),
        _ => return Err("unsupported native provider".into()),
    };
    let source = SubscriptionImportSource {
        provider: provider.into(),
        root,
        selector: String::new(),
    };
    let mut sources = expand(source)?;
    if provider == "kiro" {
        sources.push(SubscriptionImportSource {
            provider: provider.into(),
            root: h.join(".aws/sso/cache"),
            selector: "ide".into(),
        });
    }
    if provider == "cursor" && cfg!(target_os = "macos") {
        sources.insert(
            0,
            SubscriptionImportSource {
                provider: provider.into(),
                root: h.join(".cursor"),
                selector: "keychain".into(),
            },
        );
    }
    Ok(sources)
}

pub(crate) fn expand(source: SubscriptionImportSource) -> Result<Vec<SubscriptionImportSource>> {
    if !source.selector.is_empty() {
        return Ok(vec![source]);
    }
    let selectors = match source.provider.as_str() {
        "cursor" => vec!["file".to_owned()],
        "kiro" => {
            if source.root.join("kiro-auth-token.json").is_file() {
                if source.root.join("data.sqlite3").is_file() {
                    vec![
                        "cli:social".into(),
                        "cli:odic".into(),
                        "cli:external-idp".into(),
                        "ide".into(),
                    ]
                } else {
                    vec!["ide".into()]
                }
            } else {
                vec![
                    "cli:social".into(),
                    "cli:odic".into(),
                    "cli:external-idp".into(),
                ]
            }
        }
        "zcode" => {
            let mut names = Vec::new();
            if let Ok(store) = read_json(&source.root.join("credentials.json")) {
                for name in store.as_object().into_iter().flat_map(|s| s.keys()) {
                    if name.contains(":coding-plan:") && name.ends_with(":api-key") {
                        names.push(format!("key:{name}"));
                    }
                }
            }
            if let Ok(settings) = read_json(&source.root.join("setting.json")) {
                for site in ["zai", "bigmodel"] {
                    if settings["providerFamilyConnectionSelections"][site]["kind"]
                        == "team-coding-plan"
                    {
                        names.push(format!("team:{site}"));
                    }
                }
            }
            if names.is_empty() {
                names.push(String::new());
            }
            names.sort();
            names
        }
        _ => vec![String::new()],
    };
    Ok(selectors
        .into_iter()
        .map(|selector| SubscriptionImportSource {
            selector,
            ..source.clone()
        })
        .collect())
}

pub(crate) fn required_file(source: &SubscriptionImportSource) -> Option<PathBuf> {
    let name = match source.provider.as_str() {
        "zcode" => "credentials.json",
        "devin" => "credentials.toml",
        "commandcode-plan" => "auth.json",
        "cursor" if source.selector == "keychain" => return None,
        "cursor" => "auth.json",
        "kiro" if source.selector == "ide" => "kiro-auth-token.json",
        "kiro" => "data.sqlite3",
        "workbuddy" => "workbuddy-desktop.info",
        "workbuddy-ai" => "workbuddy-desktop-ai.info",
        _ => return None,
    };
    Some(source.root.join(name))
}

pub(crate) async fn read(
    source: &SubscriptionImportSource,
    outbound: &UpstreamProxyConfig,
) -> Result<SensitiveString> {
    let mut builder = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30));
    builder = aipass_proxy::apply_upstream_proxy(builder, outbound)?;
    let (_input, rx) = mpsc::channel(1);
    let (tx, _output) = mpsc::channel(1);
    let (_cancel, cancelled) = watch::channel(false);
    let (refreshing, _) = watch::channel(false);
    let mut c = Context {
        client: builder
            .build()
            .map_err(|_| "cannot create import transport")?,
        outbound: Some(outbound.clone()),
        input: rx,
        output: tx,
        provider: source.provider.clone(),
        auth: json!({"nativeSource":source}),
        native_token: None,
        native_workspace: String::new(),
        models: json!({}),
        session: uuid::Uuid::new_v4().to_string(),
        sequence: 0,
        cancellation: cancelled,
        refreshing,
    };
    let auth = match source.provider.as_str() {
        "zcode" => zcode::login(&mut c, 2).await?,
        "devin" => devin::login(&mut c, 1).await?,
        "commandcode-plan" => commandcode::login(&mut c, 1).await?,
        "cursor" => cursor::login(&mut c, 1).await?,
        "kiro" => kiro::login(&mut c, 1).await?,
        "workbuddy" | "workbuddy-ai" => workbuddy::login(&mut c, 1).await?,
        _ => return Err("unsupported native provider".into()),
    };
    let mut private = crate::subscription_import::reader::PrivateAuth(auth);
    let auth = &mut private.0;
    let expiry = auth["expires"]
        .as_u64()
        .unwrap_or(0)
        .max(expires(s(auth, "access")))
        .max(expires(s(auth, "key")));
    if expiry > 0 && expiry <= now() {
        return Err("native sign-in expired".into());
    }
    if source.provider == "zcode" && auth["accountId"].as_str().unwrap_or("").is_empty() {
        if let Some(user) = crate::community::identity(auth) {
            let state: Value =
                serde_json::from_str(s(auth, "refresh")).map_err(|_| "invalid ZCode state")?;
            auth["accountId"] = json!(format!(
                "{}::{}::{}::{}",
                user,
                s(&state, "site"),
                s(&state, "org"),
                s(&state, "project")
            ));
        }
    }
    // Existing stable claims are sufficient; identity-only requests never refresh grants.
    if crate::community::identity(auth).is_none() && source.provider == "devin" {
        let server = auth["metadata"]["server"]
            .as_str()
            .unwrap_or("https://server.codeium.com");
        let u = url::Url::parse(server).map_err(|_| "invalid Devin server")?;
        if u.scheme() != "https" || !u.username().is_empty() || u.password().is_some() {
            return Err("invalid Devin server".into());
        }
        let v = json_request(
            c.client
                .post(format!(
                    "{}/exa.seat_management_pb.SeatManagementService/GetUserStatus",
                    server.trim_end_matches('/')
                ))
                .header("Connect-Protocol-Version", "1")
                .json(&json!({"metadata":{"apiKey":auth["key"],"ideName":"devin-cli"}})),
        )
        .await?;
        auth["accountId"] = v["userStatus"]["userId"]
            .as_str()
            .or(v["userStatus"]["email"].as_str())
            .map(|s| json!(s))
            .unwrap_or(Value::Null);
    }
    if crate::community::identity(auth).is_none() && source.provider == "kiro" {
        c.auth = auth.clone();
        auth["profileArn"] = json!(kiro::import_profile(&c).await?);
    }
    if crate::community::identity(auth).is_none() {
        erase(auth);
        return Err("native account identity missing".into());
    }
    auth["nativeSource"] = json!(source);
    auth["nativeDevice"] = json!(cli_accounts::device()?);
    let secret = SensitiveString::new(auth.to_string());
    erase(auth);
    Ok(secret)
}

pub(crate) fn erase(value: &mut Value) {
    use zeroize::Zeroize;
    match value {
        Value::String(s) => s.zeroize(),
        Value::Array(a) => a.iter_mut().for_each(erase),
        Value::Object(o) => o.values_mut().for_each(erase),
        _ => {}
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        erase(&mut self.auth);
    }
}
