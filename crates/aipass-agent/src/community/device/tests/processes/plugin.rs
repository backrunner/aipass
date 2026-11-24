use super::*;
use aipass_agent_protocol::{SecretValue, SubscriptionPluginAction as Action};
use futures_util::StreamExt;

const ID: &str = "plugin.example.offline";
const EXAMPLE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/subscription-provider/"
);

fn manage(process: &Process, action: Action) -> Result<Value, crate::AgentCommandError> {
    process
        .client
        .request(&AgentRequest::SubscriptionPlugin { operation: action })
}

fn enable(process: &Process, path: &Path) {
    let preview = manage(process, Action::Inspect { path: path.into() }).unwrap();
    manage(
        process,
        Action::Enable {
            path: path.into(),
            digest: preview["digest"].as_str().unwrap().into(),
        },
    )
    .unwrap();
}

fn start(process: &Process, id: Option<Uuid>, account: &str) -> CommunityLoginStatus {
    let grants = manage(process, Action::List).unwrap();
    start_with_digest(process, id, account, grants[0]["digest"].as_str()).unwrap()
}
fn start_with_digest(
    process: &Process,
    id: Option<Uuid>,
    account: &str,
    digest: Option<&str>,
) -> Result<CommunityLoginStatus, crate::AgentCommandError> {
    process.client.request(&AgentRequest::CommunityLoginStart {
        input: CommunityLoginInput {
            entry_id: id,
            plugin_digest: digest.map(str::to_owned),
            provider: ID.into(),
            method: 0,
            inputs: [("account".into(), SensitiveString::new(account))].into(),
            api_key: Some(SensitiveString::new("fixture-private-key")),
        },
    })
}
fn poll(
    process: &Process,
    ticket: Uuid,
    done: impl Fn(&CommunityLoginStatus) -> bool,
) -> CommunityLoginStatus {
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        let status: CommunityLoginStatus = process
            .client
            .request(&AgentRequest::CommunityLoginPoll { ticket })
            .unwrap();
        if done(&status) {
            return status;
        }
        assert!(Instant::now() < deadline, "login status: {status:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[test]
fn plugin_accounts_run_through_agent_login_quota_proxy_and_reauthorization() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("vault");
    drop(Vault::create(&path, &SecretString::new("fixture-password")).unwrap());
    let process = Process::start(&path);
    let manifest = root.path().join("manifest.json");
    let program = root.path().join("provider.py");
    std::fs::copy(Path::new(EXAMPLE).join("manifest.json"), &manifest).unwrap();
    std::fs::copy(Path::new(EXAMPLE).join("provider.py"), &program).unwrap();
    enable(&process, &manifest);
    let catalog: Value = process
        .client
        .request(&AgentRequest::CommunityCatalog)
        .unwrap();
    assert!(catalog
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["id"] == ID && p["plugin"] == true));

    let old_digest = catalog
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == ID)
        .unwrap()["pluginDigest"]
        .as_str()
        .unwrap();
    assert!(start_with_digest(&process, None, "alice", None).is_err());
    let login = start(&process, None, "alice");
    let done = poll(&process, login.ticket, |s| s.status != "pending");
    assert_eq!(done.status, "complete", "{:?}", done.error);
    let id = done.entry_id.unwrap();
    let refresh = AgentRequest::CommunityRefresh { entry_id: id };
    let usage: SubscriptionSnapshot = process.client.request(&refresh).unwrap();
    assert_eq!(usage.windows[0].used_percent, Some(25.0));
    let entry: aipass_vault::EntrySummary = process
        .client
        .request(&AgentRequest::ProviderGet { id })
        .unwrap();
    assert_eq!(entry.provider_kind, ProviderKind::ThirdParty);
    check_proxy(&process, &path, id, &entry);
    let source = std::fs::read_to_string(&program).unwrap();
    std::fs::write(&program, format!("{source}\n# updated\n")).unwrap();
    assert!(process.client.request::<Value>(&refresh).is_err());
    manage(&process, Action::Disable { id: ID.into() }).unwrap();
    assert!(process.client.request::<Value>(&refresh).is_err());
    enable(&process, &manifest);
    assert!(process
        .client
        .request::<Value>(&refresh)
        .unwrap_err()
        .message
        .contains("Reconnect"));
    assert!(
        start_with_digest(&process, Some(id), "alice", Some(old_digest))
            .unwrap_err()
            .message
            .contains("Plugin login form changed")
    );
    let wrong = start(&process, Some(id), "bob");
    assert_eq!(
        poll(&process, wrong.ticket, |s| s.status != "pending").status,
        "failed"
    );
    let reconnect = start(&process, Some(id), "alice");
    assert_eq!(
        poll(&process, reconnect.ticket, |s| s.status != "pending").entry_id,
        Some(id)
    );
    process.client.request::<Value>(&refresh).unwrap();

    // Keep the plugin waiting for a code and revoke its authorization while it is live.
    manage(&process, Action::Disable { id: ID.into() }).unwrap();
    enable(&process, &manifest);
    for lock in [true, false] {
        let pending = example_login(&process, ID, 1, "alice", None);
        poll(&process, pending.ticket, |s| {
            s.method.as_deref() == Some("code")
        });
        if lock {
            process
                .client
                .request::<SessionStatus>(&AgentRequest::SessionLock {
                    reason: LockReason::Manual,
                })
                .unwrap();
            process.unlock();
        } else {
            manage(&process, Action::Disable { id: ID.into() }).unwrap();
        }
        assert_eq!(
            poll(&process, pending.ticket, |s| s.status != "pending").status,
            "failed"
        );
    }
}

fn check_proxy(process: &Process, path: &Path, id: Uuid, entry: &aipass_vault::EntrySummary) {
    let marker: SecretValue = process
        .client
        .request(&AgentRequest::SecretRevealId {
            id,
            secret_id: entry.secret_refs[0].id.clone(),
        })
        .unwrap();
    let target = ResolvedTarget {
        config: serde_json::from_value(json!({"id":Uuid::new_v4(),"providerEntryId":id,"secretId":entry.secret_refs[0].id,"label":"demo","baseUrl":"https://community.aipass.invalid/v1","authScheme":"bearer","group":null,"priority":0,"enabled":true})).unwrap(),
        api_key:marker.secret.expose().into(), upstream_kind:UpstreamKind::CommunitySubscription,
        profile:aipass_proxy::ProviderProfile::Generic,upstream_proxy:None,quota:vec![],
        model_override:None,max_concurrent_requests:Some(1),supports_websockets:false,
    };
    let bridge = CommunityBridge::new(path);
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        for stream in [false,true] {
            let mut response = bridge.request(target.clone(),json!({"model":"offline","messages":[{"role":"user","content":"hello"}],"stream":stream}),None).await.unwrap();
            assert_eq!(response.status,200);
            let mut bytes = Vec::new();
            while let Some(chunk)=response.body.next().await {bytes.extend_from_slice(&chunk.unwrap());}
            assert!(String::from_utf8(bytes).unwrap().contains("Offline example response"));
        }
    });
}

fn example_login(
    process: &Process,
    provider: &str,
    method: usize,
    account: &str,
    entry_id: Option<Uuid>,
) -> CommunityLoginStatus {
    let grants = manage(process, Action::List).unwrap();
    let grant = grants
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["id"] == provider)
        .unwrap();
    let mut inputs =
        std::collections::BTreeMap::from([("account".into(), SensitiveString::new(account))]);
    if method == 0 {
        inputs.insert("region".into(), SensitiveString::new("ap"));
        inputs.insert("seats".into(), SensitiveString::new("8"));
    }
    process
        .client
        .request(&AgentRequest::CommunityLoginStart {
            input: CommunityLoginInput {
                entry_id,
                plugin_digest: grant["digest"].as_str().map(str::to_owned),
                provider: provider.to_owned(),
                method,
                inputs,
                api_key: (method == 0).then(|| SensitiveString::new("fixture-only")),
            },
        })
        .unwrap()
}

#[test]
#[ignore = "demo.py verify prepares real plugins using their native toolchains"]
fn prepared_plugins_share_the_desktop_contract() {
    let manifests: Vec<PathBuf> =
        serde_json::from_str(&std::env::var("AIPASS_EXAMPLE_MANIFESTS").unwrap()).unwrap();
    assert!(!manifests.is_empty());
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("vault");
    drop(Vault::create(&path, &SecretString::new("fixture-password")).unwrap());
    let process = Process::start(&path);
    for manifest in manifests {
        let descriptor: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
        let provider = descriptor["id"].as_str().unwrap();
        enable(&process, &manifest);
        let login = example_login(&process, provider, 0, "Demo Studio", None);
        let done = poll(&process, login.ticket, |s| s.status != "pending");
        assert_eq!(done.status, "complete", "{provider}: {:?}", done.error);
        let id = done.entry_id.unwrap();
        let usage: SubscriptionSnapshot = process
            .client
            .request(&AgentRequest::CommunityRefresh { entry_id: id })
            .unwrap();
        let entry: aipass_vault::EntrySummary = process
            .client
            .request(&AgentRequest::ProviderGet { id })
            .unwrap();
        let wire: Value = serde_json::from_slice(
            &std::fs::read(manifest.with_file_name("verified-usage.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&usage.sections).unwrap(),
            wire["sections"],
            "{provider}"
        );
        assert_eq!(
            entry.subscription.as_ref().unwrap().sections,
            usage.sections
        );
        check_proxy(&process, &path, id, &entry);
        let pending = example_login(&process, provider, 1, "Demo Studio", Some(id));
        poll(&process, pending.ticket, |s| {
            s.method.as_deref() == Some("code")
        });
        process
            .client
            .request::<Value>(&AgentRequest::CommunityLoginCode {
                ticket: pending.ticket,
                code: SensitiveString::new("DEMO"),
            })
            .unwrap();
        assert_eq!(
            poll(&process, pending.ticket, |s| s.status != "pending").entry_id,
            Some(id)
        );
        manage(
            &process,
            Action::Disable {
                id: provider.into(),
            },
        )
        .unwrap();
        assert!(process
            .client
            .request::<Value>(&AgentRequest::CommunityRefresh { entry_id: id })
            .is_err());
    }
}
