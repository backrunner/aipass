//! Device-bound references and short-lived, zeroizing credential reads.
use super::*;

pub(crate) fn default_home(provider: &str) -> Result<PathBuf> {
    let (env, folder) = match provider {
        "codex" => ("CODEX_HOME", ".codex"),
        "grok" => ("GROK_HOME", ".grok"),
        "copilot" => ("COPILOT_HOME", ".copilot"),
        "gemini-cli" => ("GEMINI_CLI_HOME", ".gemini"),
        _ => return Err("unknown CLI account".into()),
    };
    let path = if provider == "gemini-cli" {
        std::env::var_os(env).map(PathBuf::from).unwrap_or(home()?)
    } else {
        std::env::var_os(env)
            .map(PathBuf::from)
            .unwrap_or(home()?.join(folder))
    };
    Ok(path)
}

pub(super) fn config_home(provider: &str, root: &Path) -> PathBuf {
    if provider == "gemini-cli" {
        root.join(".gemini")
    } else {
        root.to_owned()
    }
}

/// Also used by migration: write once into a new vendor home, never mirror back.
pub(crate) fn new_home(provider: &str) -> Result<PathBuf> {
    let base = match provider {
        "gemini-cli" => home()?.join(".gemini/aipass-accounts"),
        _ => default_home(provider)?.join("aipass-accounts"),
    };
    std::fs::create_dir_all(&base).map_err(|_| "cannot create CLI account directory")?;
    let root = base.join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir(&root).map_err(|_| "cannot create CLI account directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "cannot protect CLI account directory")?;
    }
    Ok(root)
}

pub(crate) fn handoff_home(provider: &str, id: uuid::Uuid) -> Result<PathBuf> {
    let base = default_home(provider)?.join("aipass-accounts");
    let path = base.join(id.to_string());
    std::fs::create_dir_all(&path).map_err(|_| "cannot create CLI account directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "cannot protect CLI account directory")?;
    }
    Ok(path)
}

pub(crate) fn reference(provider: &str, root: &Path) -> Result<Value> {
    let credential = read(provider, root)?;
    let identity = s(&credential, "accountId");
    if identity.is_empty() {
        return Err("CLI returned no account identity; sign in again".into());
    }
    Ok(
        json!({"type":"oauth","nativeHome":root,"accountId":identity,"nativeProvider":provider,"nativeDevice":device()?}),
    )
}

pub(crate) fn device() -> Result<String> {
    #[cfg(test)]
    return Ok("local-test-device".into());
    #[cfg(not(test))]
    {
        static ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        if let Some(id) = ID.get() {
            return Ok(id.clone());
        }
        let path = home()?.join(".aipass-cli-device");
        fn load(path: &Path) -> Result<String> {
            use std::io::Read;
            let mut raw = String::new();
            std::fs::File::open(path)
                .map_err(|_| "local CLI device identity unavailable")?
                .take(65)
                .read_to_string(&mut raw)
                .map_err(|_| "invalid local CLI device identity")?;
            uuid::Uuid::parse_str(raw.trim())
                .map(|v| v.to_string())
                .map_err(|_| "invalid local CLI device identity".into())
        }
        let value = if path.exists() {
            load(&path)?
        } else {
            let id = uuid::Uuid::new_v4().to_string();
            use std::io::Write;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(mut file) => {
                    file.write_all(id.as_bytes())
                        .map_err(|_| "cannot save local CLI device identity")?;
                    id
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let mut result = load(&path);
                    for _ in 0..5 {
                        if result.is_ok() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(20));
                        result = load(&path);
                    }
                    result?
                }
                Err(_) => return Err("cannot create local CLI device identity".into()),
            }
        };
        let _ = ID.set(value.clone());
        Ok(value)
    }
}

pub(super) struct PrivateJson(Value);
impl std::ops::Deref for PrivateJson {
    type Target = Value;
    fn deref(&self) -> &Value {
        &self.0
    }
}
impl Drop for PrivateJson {
    fn drop(&mut self) {
        fn clear(v: &mut Value) {
            match v {
                Value::String(s) => zeroize::Zeroize::zeroize(s),
                Value::Array(a) => a.iter_mut().for_each(clear),
                Value::Object(o) => o.values_mut().for_each(clear),
                _ => {}
            }
        }
        clear(&mut self.0);
    }
}
pub(super) fn read(provider: &str, root: &Path) -> Result<PrivateJson> {
    let dir = config_home(provider, root);
    let value: Result<Value> = match provider {
        "codex" => {
            let raw = PrivateJson(read_json(&dir.join("auth.json"))?);
            if !s(&raw, "OPENAI_API_KEY").is_empty() {
                return Err("Sign in to ChatGPT in Codex instead of using an API key".into());
            }
            let tokens = &raw["tokens"];
            let id = claims(s(tokens, "id_token"));
            let account = s(tokens, "account_id");
            let user = s(&id, "email");
            if (user.is_empty() && s(&id, "sub").is_empty())
                || account.is_empty()
                || s(tokens, "access_token").is_empty()
            {
                return Err("Codex is not signed in to ChatGPT".into());
            }
            Ok(
                json!({"access":tokens["access_token"],"accountId":format!("{}:{account}", if user.is_empty() {s(&id,"sub")} else {user}),"workspace":account,"expires":claims(s(tokens,"access_token"))["exp"]}),
            )
        }
        "grok" => {
            let raw = PrivateJson(read_json(&dir.join("auth.json"))?);
            let item = raw
                .as_object()
                .into_iter()
                .flatten()
                .find_map(|(issuer, v)| {
                    (issuer.trim_end_matches('/') == "https://auth.x.ai" && !s(v, "key").is_empty())
                        .then_some(v)
                })
                .ok_or("Grok Build is not signed in")?;
            let expiry = item["expires_at"]
                .as_str()
                .and_then(|s| {
                    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
                        .ok()
                })
                .map(|t| t.unix_timestamp());
            Ok(json!({"access":item["key"],"accountId":item["email"],"expires":expiry}))
        }
        "gemini-cli" => {
            let raw = PrivateJson(read_json(&dir.join("oauth_creds.json"))?);
            let users = read_json(&dir.join("google_accounts.json"))?;
            if s(&raw, "access_token").is_empty() || s(&users, "active").is_empty() {
                return Err("Gemini CLI is not signed in with Google".into());
            }
            Ok(
                json!({"access":raw["access_token"],"accountId":users["active"],"expires":raw["expiry_date"].as_i64().map(|v|v/1000)}),
            )
        }
        "copilot" => {
            let config = PrivateJson(read_json(&dir.join("config.json"))?);
            let user = &config["lastLoggedInUser"];
            let login = s(user, "login");
            if login.is_empty()
                || !matches!(
                    s(user, "host").trim_end_matches('/'),
                    "https://github.com" | "github.com" | ""
                )
            {
                return Err("Sign in to github.com with Copilot CLI".into());
            }
            let key = format!("https://github.com:{login}");
            let token = config["copilotTokens"][&key]
                .as_str()
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
                .or_else(|| {
                    #[cfg(target_os = "macos")]
                    {
                        crate::official_accounts::read_keychain_bytes("copilot-cli", Some(&key))
                            .ok()
                            .and_then(|v| std::str::from_utf8(&v).ok().map(str::to_owned))
                            .map(|v| v.trim().to_owned())
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        None
                    }
                })
                .filter(|v| !v.is_empty())
                .ok_or("Copilot CLI credential store is unavailable; sign in again")?;
            Ok(json!({"access":token,"accountId":login}))
        }
        _ => Err("unknown CLI account".into()),
    };
    Ok(PrivateJson(value?))
}

pub(super) fn root(auth: &Value) -> Result<PathBuf> {
    let path = PathBuf::from(s(auth, "nativeHome"));
    if !path.is_absolute() || !path.is_dir() {
        return Err("This CLI account is unavailable on this computer; connect it locally".into());
    }
    Ok(path)
}

pub(crate) fn check_device(auth: &Value) -> Result<()> {
    if s(auth, "nativeDevice") != device()? {
        return Err("Connect this CLI subscription on this computer before using it".into());
    }
    Ok(())
}
