use super::*;
#[test]
fn copilot_uses_one_cli_authorization_header() {
    let (mut c, _input, _output) = super::super::tests::context(Client::new());
    c.provider = "copilot".into();
    c.native_token = Some(aipass_agent_protocol::SensitiveString::new(
        "synthetic-cli-access",
    ));
    let request = copilot_user_request(&c).unwrap().build().unwrap();
    assert_eq!(request.headers().get_all("authorization").iter().count(), 1);
    assert_eq!(
        request.headers()["authorization"],
        "token synthetic-cli-access"
    );
    assert_eq!(
        request.headers()["copilot-integration-id"],
        "copilot-developer-cli"
    );
}
#[test]
fn references_contain_no_grants_and_pin_workspace() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("auth.json"),json!({"tokens":{"id_token":"x.eyJzdWIiOiJ1c2VyLWEifQ.x","access_token":"fake-access","refresh_token":"fake-refresh","account_id":"workspace-a"}}).to_string()).unwrap();
    let reference = reference("codex", dir.path()).unwrap();
    assert_eq!(reference["accountId"], "user-a:workspace-a");
    assert!(!reference.to_string().contains("fake-"));
    assert!(reference.get("refresh").is_none());
    assert!(reference.get("access").is_none());
}
#[tokio::test]
async fn credentials_are_reread_but_account_and_device_changes_fail_closed() {
    let dir = tempfile::tempdir().unwrap();
    let write = |user: &str, token: &str| {
        std::fs::write(dir.path().join("config.json"),json!({"lastLoggedInUser":{"login":user,"host":"https://github.com"},"copilotTokens":{format!("https://github.com:{user}"):token}}).to_string()).unwrap()
    };
    write("alice", "first-access");
    let (mut c, _input, _output) = super::super::tests::context(Client::new());
    c.provider = "copilot".into();
    c.auth = reference("copilot", dir.path()).unwrap();
    fresh(&mut c).await.unwrap();
    assert_eq!(c.token().unwrap(), "first-access");
    assert!(c.auth.get("access").is_none());
    write("alice", "rotated-access");
    fresh(&mut c).await.unwrap();
    assert_eq!(c.token().unwrap(), "rotated-access");
    write("bob", "foreign-access");
    assert!(fresh(&mut c).await.unwrap_err().contains("account changed"));
    assert!(c.native_token.is_none());
    c.auth["nativeDevice"] = json!("another-computer");
    assert!(fresh(&mut c).await.unwrap_err().contains("this computer"));
}
#[cfg(unix)]
#[tokio::test]
async fn account_rpc_initializes_and_preserves_cli_errors_without_a_prompt() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("cli");
    std::fs::write(
        &exe,
        r#"#!/bin/sh
read init
printf '%s\n' '{"id":1,"result":{}}'
read notification
read request
case "$request" in
 *account/read*) printf '%s\n' '{"id":2,"result":{"account":{"type":"chatgpt"}}}' ;;
 *) printf '%s\n' '{"id":2,"error":{"message":"synthetic private error"}}' ;;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
    let (mut c, _input, _output) = super::super::tests::context(Client::new());
    let cmd = native_cli::command(&exe, &c);
    let value = rpc(
        cmd,
        &mut c,
        "account/read",
        json!({"refreshToken":true}),
        false,
        false,
    )
    .await
    .unwrap();
    assert_eq!(value["account"]["type"], "chatgpt");
    let cmd = native_cli::command(&exe, &c);
    let error = rpc(
        cmd,
        &mut c,
        "account/rateLimits/read",
        json!(null),
        false,
        false,
    )
    .await
    .unwrap_err();
    assert!(!error.contains("synthetic private"));
}
#[test]
fn codex_and_copilot_usage_preserve_numbers_and_scoped_limits() {
    let v=codex_usage(&json!({"rateLimits":{"planType":"pro","credits":{"balance":"10"}},"rateLimitsByLimitId":{"codex":{"primary":{"usedPercent":25,"windowDurationMins":300,"resetsAt":2000000000}},"spark":{"primary":{"usedPercent":100}}}})).unwrap();
    assert_eq!(v["windows"][0]["span"], 18000);
    assert_eq!(v["windows"][1]["aside"], true);
    let v=copilot_usage(&json!({"copilot_plan":"pro","quota_snapshots":{"premium":{"entitlement":"300","quota_remaining":"225"},"chat":{"percent_remaining":80},"unlimited":{"unlimited":true,"percent_remaining":0}}}),&json!({"paid":{"premiumMultiplier":1},"free":{"premiumMultiplier":0}})).unwrap();
    assert_eq!(v["windows"].as_array().unwrap().len(), 2);
    assert_eq!(v["windows"][0]["used"].as_f64(), Some(20.0));
    assert_eq!(v["windows"][1]["used"].as_f64(), Some(25.0));
    assert_eq!(v["windows"][1]["models"], json!(["paid"]));
    assert_eq!(v["windows"][0]["aside"], true);
    assert!(codex_usage(&json!({})).is_err());
    assert!(copilot_usage(&json!({}), &json!({})).is_err());
}
#[test]
#[ignore = "local CLI capability checks only; no credentials, login or generation"]
fn installed_native_clis_can_be_probed_without_credentials() {
    for provider in ["codex", "grok", "gemini-cli"] {
        let status = status(provider);
        assert!(status.available, "{provider}: {status:?}");
    }
    let missing = status("aipass-unknown-cli");
    assert!(!missing.available);
    assert_eq!(missing.reason.as_deref(), Some("unsupported"));
}
#[test]
fn google_home_is_a_home_not_a_config_directory() {
    assert_eq!(
        config_home("gemini-cli", Path::new("/private/home")),
        PathBuf::from("/private/home/.gemini")
    );
}
