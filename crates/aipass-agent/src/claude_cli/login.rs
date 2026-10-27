//! Ticket-bound Claude Code login, cancellation and account completion.
use super::*;

pub(super) struct Flow {
    binary: PathBuf,
    child: Child,
    directory: ConfigDir,
    output: PathBuf,
    started: Instant,
    url: Option<String>,
    ready: Option<NativeAccount>,
    saved: Option<Uuid>,
}
impl Drop for Flow {
    fn drop(&mut self) {
        stop(&mut self.child);
    }
}

pub(crate) struct NativeAccount {
    pub(crate) home: PathBuf,
    pub(crate) identity: String,
    pub(crate) plan: Option<String>,
}
pub(crate) struct LoginManager {
    pub(super) flows: Arc<Mutex<HashMap<Uuid, Flow>>>,
    pub(super) epoch: AtomicU64,
}
pub(crate) fn logins() -> &'static LoginManager {
    static MANAGER: OnceLock<LoginManager> = OnceLock::new();
    MANAGER.get_or_init(|| {
        let flows = Arc::new(Mutex::new(HashMap::<Uuid, Flow>::new()));
        let sweep = flows.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(30));
            if let Ok(mut flows) = sweep.lock() {
                flows.retain(|_, f| f.ready.is_some() || f.started.elapsed() < LOGIN_TTL);
            }
        });
        LoginManager {
            flows,
            epoch: AtomicU64::new(0),
        }
    })
}
impl LoginManager {
    pub(crate) fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }
    pub(crate) fn clear(&self) {
        let mut flows = self.flows.lock().unwrap();
        self.epoch.fetch_add(1, Ordering::AcqRel);
        flows.clear();
    }
    pub(crate) fn cancel(&self, ticket: Uuid) -> bool {
        self.flows.lock().unwrap().remove(&ticket).is_some()
    }
    pub(crate) fn start(&self, epoch: u64) -> Result<ClaudeLoginStatus, String> {
        let (_, binary) = available_from(&candidates());
        let binary = binary.ok_or(INSTALL_ERROR)?;
        self.start_with(binary, epoch)
    }
    pub(super) fn start_with(
        &self,
        binary: PathBuf,
        epoch: u64,
    ) -> Result<ClaudeLoginStatus, String> {
        let directory = ConfigDir::login()?;
        let path = directory.path().join(".login-output");
        crate::claude_bridge::write_private(&path, b"")?;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .map_err(|_| "cannot capture Claude sign-in")?;
        let stderr = file
            .try_clone()
            .map_err(|_| "cannot capture Claude sign-in")?;
        let mut command = Command::new(&binary);
        configure(&mut command, directory.path());
        command
            .args(["auth", "login", "--claudeai"])
            .env(
                "BROWSER",
                std::env::current_exe().map_err(|_| "Agent executable unavailable")?,
            )
            .env(OPENED_URL, directory.path().join(".opened-url"))
            .stdin(Stdio::piped())
            .stdout(file)
            .stderr(stderr);
        let child = command.spawn().map_err(|_| INSTALL_ERROR)?;
        let flow = Flow {
            binary,
            child,
            directory,
            output: path,
            started: Instant::now(),
            url: None,
            ready: None,
            saved: None,
        };
        let ticket = Uuid::new_v4();
        let mut flows = self.flows.lock().unwrap();
        if self.epoch() != epoch {
            return Err("Claude sign-in was cancelled".into());
        }
        if flows.values().filter(|flow| flow.saved.is_none()).count() >= 4 {
            return Err("Close another Claude sign-in before adding an account".into());
        }
        flows.insert(ticket, flow);
        Ok(ClaudeLoginStatus {
            ticket,
            status: "pending".into(),
            url: None,
            entry_id: None,
            message: None,
        })
    }
    pub(crate) fn poll(&self, ticket: Uuid) -> Result<ClaudeLoginStatus, String> {
        let mut flows = self.flows.lock().unwrap();
        let Some(flow) = flows.get_mut(&ticket) else {
            return Ok(ClaudeLoginStatus {
                ticket,
                status: "expired".into(),
                url: None,
                entry_id: None,
                message: None,
            });
        };
        let mut result = ClaudeLoginStatus {
            ticket,
            status: "pending".into(),
            url: flow.url.clone(),
            entry_id: None,
            message: None,
        };
        if let Some(id) = flow.saved {
            result.status = "authorized".into();
            result.entry_id = Some(id);
            return Ok(result);
        }
        if flow.ready.is_some() {
            result.status = "authorized".into();
            return Ok(result);
        }
        if flow.started.elapsed() >= LOGIN_TTL
            || std::fs::metadata(&flow.output).map_or(true, |m| m.len() > OUTPUT_LIMIT)
        {
            flows.remove(&ticket);
            result.status = "expired".into();
            return Ok(result);
        }
        if flow.url.is_none() {
            let opened = read_bounded(&flow.directory.path().join(".opened-url")).ok();
            let text = read_bounded(&flow.output)?;
            let url = opened
                .filter(|s| authorization_url(s.trim()).is_some())
                .or_else(|| {
                    regex::Regex::new(r#"https://[^\s\x1b\"'<>]+"#)
                        .unwrap()
                        .find_iter(&text)
                        .map(|m| m.as_str())
                        .find(|s| authorization_url(s).is_some())
                        .map(str::to_owned)
                });
            flow.url = url.map(|s| s.trim().to_owned());
            result.url = flow.url.clone();
        }
        if let Some(status) = flow
            .child
            .try_wait()
            .map_err(|_| "cannot read Claude sign-in status")?
        {
            if !status.success() {
                flows.remove(&ticket);
                result.status = "error".into();
                result.message =
                    Some("Claude Code sign-in failed; retry or update Claude Code".into());
                return Ok(result);
            }
            flow.ready = Some(signed_in(&flow.binary, &flow.directory)?);
            result.status = "authorized".into();
        }
        Ok(result)
    }
    /// Called under the vault lock. Failed writes retain the already signed-in grant.
    pub(crate) fn commit(
        &self,
        ticket: Uuid,
        write: impl FnOnce(&NativeAccount) -> Result<Uuid, crate::session::ServiceError>,
    ) -> Result<Uuid, crate::session::ServiceError> {
        use crate::session::ServiceError;
        let mut flows = self.flows.lock().unwrap();
        if let Some(id) = flows.get(&ticket).and_then(|flow| flow.saved) {
            return Ok(id);
        }
        let flow = flows
            .get_mut(&ticket)
            .filter(|f| f.ready.is_some())
            .ok_or_else(|| {
                ServiceError::new(
                    aipass_agent_protocol::AgentErrorCode::ValidationFailed,
                    "Claude sign-in was cancelled",
                )
            })?;
        // A failed vault write can be ambiguous (record committed, audit failed).
        // Once persistence is attempted, cancellation must not delete a CLI home
        // that a durable provider may already reference.
        flow.directory.keep();
        let account = flow.ready.as_ref().unwrap();
        let result = write(account)?;
        if let Some(flow) = flows.get_mut(&ticket) {
            flow.saved = Some(result);
            flow.ready = None;
        }
        Ok(result)
    }
    pub(crate) fn code(&self, ticket: Uuid, raw: &str) -> Result<(), String> {
        let mut flows = self.flows.lock().unwrap();
        let flow = flows.get_mut(&ticket).ok_or("Claude sign-in expired")?;
        if flow.ready.is_some() || flow.started.elapsed() >= LOGIN_TTL {
            return Err("Claude sign-in is no longer pending".into());
        }
        let raw = raw.trim();
        if raw.len() > 8192 || raw.contains(['\n', '\r', ' ']) {
            return Err("Paste the code or callback URL from this Claude sign-in".into());
        }
        if let Ok(url) = url::Url::parse(raw) {
            let auth = flow
                .url
                .as_deref()
                .and_then(authorization_url)
                .ok_or("Claude sign-in has no browser link")?;
            let redirect = auth
                .query_pairs()
                .find(|(k, _)| k == "redirect_uri")
                .and_then(|(_, v)| url::Url::parse(&v).ok())
                .ok_or("This sign-in expects a pasted code")?;
            if url.scheme() != "http"
                || !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
                || url.port() != redirect.port()
                || url.path() != redirect.path()
                || !matches!(
                    redirect.host_str(),
                    Some("localhost" | "127.0.0.1" | "[::1]")
                )
                || !url.username().is_empty()
                || url.password().is_some()
                || !url.query_pairs().any(|(k, v)| k == "code" && !v.is_empty())
            {
                return Err("That callback URL does not belong to this Claude sign-in".into());
            }
            let response = reqwest::blocking::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| "cannot deliver Claude callback")?
                .get(url)
                .send()
                .map_err(|_| "Claude Code could not receive the callback")?;
            if response.status().is_client_error() || response.status().is_server_error() {
                return Err("Claude Code rejected the callback".into());
            }
            return Ok(());
        }
        if !raw
            .split_once('#')
            .is_some_and(|(code, state)| !code.is_empty() && !state.is_empty())
        {
            return Err("Paste the complete code shown by Claude, including #state".into());
        }
        let stdin = flow
            .child
            .stdin
            .as_mut()
            .ok_or("Claude sign-in has ended")?;
        writeln!(stdin, "{raw}").map_err(|_| "Claude sign-in has ended".into())
    }
}

pub(super) fn read_bounded(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|_| "cannot read Claude sign-in output")?;
    let mut text = String::new();
    file.take(OUTPUT_LIMIT + 1)
        .read_to_string(&mut text)
        .map_err(|_| "invalid Claude sign-in output")?;
    if text.len() as u64 > OUTPUT_LIMIT {
        return Err("Claude sign-in output exceeds limit".into());
    }
    Ok(text)
}

pub(super) fn signed_in(binary: &Path, directory: &ConfigDir) -> Result<NativeAccount, String> {
    let default = directories::BaseDirs::new()
        .ok_or("home directory unavailable")?
        .home_dir()
        .join(".claude");
    signed_in_at(binary, directory, &default)
}

fn signed_in_at(
    binary: &Path,
    directory: &ConfigDir,
    default: &Path,
) -> Result<NativeAccount, String> {
    use aipass_config_writers::native_auth::{claude_identity, parse, PrivateJson, Resource};
    let mut command = Command::new(binary);
    configure(&mut command, directory.path());
    command.args(["auth", "status", "--json"]);
    let text = Zeroizing::new(output(&mut command, Duration::from_secs(10))?);
    let status: Value =
        serde_json::from_str(&text).map_err(|_| "Claude Code returned invalid account status")?;
    if status["loggedIn"] != true
        || matches!(status["authMethod"].as_str(), Some("api_key" | "console"))
    {
        return Err("Claude Code did not sign in to a Claude subscription".into());
    }
    let profile = Resource::File(claude_identity::profile_path(directory.path(), default))
        .read()
        .map_err(|_| "Claude account profile unavailable")?
        .map(|bytes| parse(&bytes).map_err(|_| "Claude account profile is invalid"))
        .transpose()?
        .unwrap_or(PrivateJson(Value::Null));
    let identity = claude_identity::identity(&status, &profile.0).map_err(|e| e.to_string())?;
    Ok(NativeAccount {
        home: directory.path().to_owned(),
        identity,
        plan: status["subscriptionType"].as_str().map(str::to_owned),
    })
}

#[cfg(all(test, unix))]
mod review_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn default_profile_identity_matches_native_switches_and_malformed_profiles_fail() {
        use aipass_config_writers::native_auth::{claude_identity, NativeStore, Resource};
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(".claude");
        std::fs::create_dir(&path).unwrap();
        let binary = root.path().join("cli");
        std::fs::write(
            &binary,
            "#!/bin/sh\necho '{\"loggedIn\":true,\"email\":\"alice@example.test\"}'\n",
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let profile = claude_identity::profile_path(&path, &path);
        std::fs::write(
            &profile,
            json!({"oauthAccount":{"emailAddress":"alice@example.test","organizationUuid":"a"}})
                .to_string(),
        )
        .unwrap();
        let directory = ConfigDir {
            path: path.clone(),
            temporary: None,
        };
        let account = signed_in_at(&binary, &directory, &path).unwrap();
        let primary = Resource::File(path.join(".credentials.json"));
        primary
            .write(Some(br#"{"claudeAiOauth":{"accessToken":"fixture-access"}}"#))
            .unwrap();
        let store = NativeStore {
            tool: "claude-code".into(),
            home: path,
            primary,
            fallback: None,
            profile: Some(Resource::File(profile.clone())),
        };
        assert_eq!(Some(account.identity), store.identity().unwrap());
        std::fs::write(profile, "invalid profile").unwrap();
        assert!(signed_in_at(&binary, &directory, &directory.path).is_err());
    }

    #[test]
    fn canceled_login_removes_its_owned_directory() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join("cli");
        std::fs::write(&binary, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let manager = LoginManager {
            flows: Arc::new(Mutex::new(HashMap::new())),
            epoch: AtomicU64::new(0),
        };
        let login = manager.start_with(binary, 0).unwrap();
        let directory = manager.flows.lock().unwrap()[&login.ticket]
            .directory
            .path
            .clone();
        assert!(directory.is_dir());
        assert!(manager.cancel(login.ticket));
        assert!(!directory.exists());
    }

    #[test]
    fn failed_commit_retains_the_home_after_cancellation_for_ambiguous_writes() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join("cli");
        std::fs::write(&binary, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let manager = LoginManager {
            flows: Arc::new(Mutex::new(HashMap::new())),
            epoch: AtomicU64::new(0),
        };
        let login = manager.start_with(binary, 0).unwrap();
        let path = {
            let mut flows = manager.flows.lock().unwrap();
            let flow = flows.get_mut(&login.ticket).unwrap();
            flow.ready = Some(NativeAccount {
                home: flow.directory.path.clone(),
                identity: "alice@example.test".into(),
                plan: None,
            });
            flow.directory.path.clone()
        };
        assert!(manager
            .commit(login.ticket, |_| Err(
                crate::session::ServiceError::internal(anyhow::anyhow!("ambiguous write"))
            ))
            .is_err());
        manager.cancel(login.ticket);
        assert!(path.is_dir());
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn completed_logins_do_not_block_a_fifth_account() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join("claude");
        std::fs::write(&binary, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let manager = LoginManager {
            flows: Arc::new(Mutex::new(HashMap::new())),
            epoch: AtomicU64::new(0),
        };
        for _ in 0..4 {
            let flow = manager.start_with(binary.clone(), 0).unwrap();
            manager
                .flows
                .lock()
                .unwrap()
                .get_mut(&flow.ticket)
                .unwrap()
                .saved = Some(Uuid::new_v4());
        }
        assert!(manager.start_with(binary, 0).is_ok());
        manager.clear();
    }
}
