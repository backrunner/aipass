use super::*;

fn now() -> chrono::DateTime<Utc> {
    "2026-10-01T04:00:00Z".parse().unwrap()
}

#[test]
fn usage_preserves_percentages_model_scopes_and_iana_resets() {
    let windows = parse_usage("\x1b[32mCurrent session: 13% used · resets Oct 1 at 3:30pm (Asia/Shanghai)\x1b[0m\nCurrent week (all models): 0.4% used · resets Oct 3, 2:59pm (UTC)\nCurrent week (Sonnet): 7% used\nCurrent week (Fable): 0% used", now()).unwrap();
    assert_eq!(windows.len(), 4);
    assert_eq!(
        windows[0].resets_at.as_deref(),
        Some("2026-10-01T07:30:00Z")
    );
    assert_eq!(windows[1].used_percent, Some(0.4));
    assert_eq!(
        windows[1].resets_at.as_deref(),
        Some("2026-10-03T14:59:00Z")
    );
    assert_eq!(windows[2].id, "seven_day_sonnet");
    assert_eq!(windows[3].label, "7d Fable");
    assert_eq!(windows[0].source.as_deref(), Some("claude-code-usage"));
    let december = "2026-12-31T20:00:00Z".parse().unwrap();
    assert_eq!(
        reset_time("Jan 1 at 3pm (UTC)", december, 10080).unwrap(),
        "2027-01-01T15:00:00Z"
    );
    assert_eq!(
        reset_time("3pm (Asia/Shanghai)", now(), 300).unwrap(),
        "2026-10-01T07:00:00Z"
    );
}

#[test]
fn usage_never_treats_denied_unknown_or_invalid_output_as_success() {
    for text in [
        "",
        "You are currently using your subscription to power your Claude Code usage.",
        "Current session: 12% used\nError: 401 unauthorized",
        "Current session: 101% used",
        "Current session: 4% used · resets tomorrow (Not/AZone)",
        "Current session: 4% used\nCurrent session: 5% used",
    ] {
        assert!(parse_usage(text, now()).is_err(), "{text}");
    }
    assert!(authorization_url("https://claude.ai.evil.test/oauth/authorize").is_none());
    assert!(authorization_url("https://alice:secret@claude.ai/oauth/authorize").is_none());
    assert!(authorization_url("https://claude.ai/oauth/authorize?state=test").is_some());
}

#[cfg(unix)]
fn script(text: &str) -> (tempfile::TempDir, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("claude");
    std::fs::write(&path, format!("#!/bin/sh\n{text}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    (temp, path)
}

#[cfg(unix)]
#[test]
fn unavailable_detection_skips_broken_candidates_and_bounds_hangs() {
    assert_eq!(available_from(&[]).0.reason.as_deref(), Some("missing"));
    let (_temp, broken) = script("exit 1");
    assert_eq!(
        available_from(std::slice::from_ref(&broken))
            .0
            .reason
            .as_deref(),
        Some("unusable")
    );
    let (_temp, old) = script("echo '1.0 (Claude Code)'");
    assert_eq!(
        available_from(&[old]).0.reason.as_deref(),
        Some("unsupported")
    );
    let (_temp, good) = script("case \"$*\" in --version) echo '2.1.284 (Claude Code)' ;; 'auth login --help') echo --claudeai ;; --help) echo '--no-session-persistence --setting-sources --strict-mcp-config' ;; esac");
    assert!(available_from(&[broken, good]).0.available);
    let (_temp, hang) = script("sleep 60");
    let mut command = Command::new(hang);
    let dir = ConfigDir::new().unwrap();
    configure(&mut command, dir.path());
    let start = Instant::now();
    assert!(output(&mut command, Duration::from_millis(50)).is_err());
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[cfg(unix)]
#[test]
fn isolated_login_survives_failed_save_and_replays_only_the_saved_entry() {
    let (_temp, binary) = script(
        r#"
if [ "$1 $2" = 'auth status' ]; then
  echo '{"loggedIn":true,"authMethod":"claude.ai","email":"alice@example.test","subscriptionType":"max"}'
  exit
fi
echo 'https://claude.ai/oauth/authorize?state=test'
read -r code
printf '%s' '{"claudeAiOauth":{"accessToken":"synthetic-access","refreshToken":"synthetic-refresh","expiresAt":2000000000000}}' > .credentials.json
printf '%s' '{"oauthAccount":{"emailAddress":"alice@example.test"}}' > .claude.json
"#,
    );
    let manager = LoginManager {
        flows: Arc::new(Mutex::new(HashMap::new())),
        epoch: AtomicU64::new(0),
    };
    let challenge = manager.start_with(binary.clone(), 0).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while manager.poll(challenge.ticket).unwrap().url.is_none() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(manager.code(challenge.ticket, "wrong\ncode").is_err());
    assert!(manager
        .code(challenge.ticket, "http://localhost:9999/callback?code=x")
        .is_err());
    manager.code(challenge.ticket, "code#test").unwrap();
    while manager.poll(challenge.ticket).unwrap().status != "authorized" {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(manager
        .commit(challenge.ticket, |_| Err(
            crate::session::ServiceError::internal(anyhow::anyhow!("fixture IO"))
        ))
        .is_err());
    assert_eq!(manager.poll(challenge.ticket).unwrap().status, "authorized");
    let id = Uuid::new_v4();
    assert_eq!(
        manager
            .commit(challenge.ticket, |native| {
                assert_eq!(native.identity, "alice@example.test");
                assert_eq!(native.plan.as_deref(), Some("max"));
                Ok(id)
            })
            .unwrap(),
        id
    );
    assert_eq!(manager.poll(challenge.ticket).unwrap().entry_id, Some(id));
    manager.clear();
    assert_eq!(manager.poll(challenge.ticket).unwrap().status, "expired");
    assert!(manager.start_with(binary, 0).is_err());
}

#[test]
#[ignore = "explicit local CLI capability smoke, no login or model call"]
fn installed_claude_can_be_probed_without_credentials() {
    assert!(status().available);
}

#[cfg(target_os = "macos")]
#[test]
fn default_account_keeps_the_vendor_keychain_while_new_accounts_are_isolated() {
    let home = directories::BaseDirs::new()
        .unwrap()
        .home_dir()
        .join(".claude");
    let mut command = Command::new("claude");
    account_environment(&mut command, &home);
    assert!(command.get_envs().any(
        |(k, v)| k == "CLAUDE_SECURESTORAGE_CONFIG_DIR" && v == Some(std::ffi::OsStr::new(""))
    ));
    let dir = tempfile::tempdir().unwrap();
    account_environment(&mut command, dir.path());
    assert!(command
        .get_envs()
        .any(|(k, v)| k == "CLAUDE_SECURESTORAGE_CONFIG_DIR" && v.is_none()));
}

#[cfg(unix)]
#[test]
fn account_discovery_uses_cli_status_without_reading_an_oauth_grant() {
    let (_temp, binary) = script("echo '{\"loggedIn\":true,\"authMethod\":\"claude.ai\",\"email\":\"alice@example.test\",\"subscriptionType\":\"max\"}'");
    let dir = ConfigDir::new().unwrap();
    // No credentials file or Keychain access is needed by the account reader.
    let account = signed_in(&binary, &dir).unwrap();
    assert_eq!(account.identity, "alice@example.test");
    assert_eq!(account.home, dir.path());
}
