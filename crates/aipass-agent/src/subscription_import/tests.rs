use super::*;
use aipass_crypto::SecretString;
use serde_json::json;
use std::path::Path;

#[test]
fn provider_aliases_have_one_import_identity() {
    assert_eq!(sources::canonical_provider("openai"), Some("codex"));
    assert_eq!(sources::canonical_provider("claude"), Some("anthropic"));
    assert_eq!(sources::PROVIDERS.len(), 12);
    assert_eq!(sources::canonical_provider("factory"), None);
}

fn source(provider: &str, root: &Path, selector: &str) -> SubscriptionImportSource {
    SubscriptionImportSource {
        provider: provider.into(),
        root: root.to_owned(),
        selector: selector.into(),
    }
}
fn state(root: &Path) -> Arc<AgentState> {
    let state = crate::server::tests::sync_test_state(root.to_owned());
    let vault = aipass_vault::Vault::create(root, &SecretString::new("fixture password"))
        .unwrap()
        .vault;
    *state.session.lock().unwrap() =
        SessionState::Unlocked(Box::new(crate::session::SessionInfo {
            id: Uuid::new_v4(),
            vault,
            unlocked_at: time::OffsetDateTime::now_utc(),
            last_activity_at: time::OffsetDateTime::now_utc(),
        }));
    state
}
fn write_workbuddy(root: &Path, provider: &str, user: &str, token: &str) {
    std::fs::create_dir_all(root).unwrap();
    std::fs::write(
        root.join(if provider == "workbuddy-ai" {
            "workbuddy-desktop-ai.info"
        } else {
            "workbuddy-desktop.info"
        }),
        json!({"auth":{"accessToken":token},"account":{"uid":user}}).to_string(),
    )
    .unwrap();
}
async fn read(source: &SubscriptionImportSource) -> reader::Account {
    reader::read(source, &aipass_proxy::UpstreamProxyConfig::default())
        .await
        .unwrap()
}
fn summaries(state: &Arc<AgentState>) -> Vec<aipass_vault::EntrySummary> {
    crate::session::with_vault(state, false, |v| {
        v.list_provider_summaries().map_err(map_vault_error)
    })
    .unwrap()
}
fn seed_retry(
    state: &Arc<AgentState>,
    sources: Vec<SubscriptionImportSource>,
) -> SubscriptionImportInput {
    let results = sources
        .into_iter()
        .map(|s| failure(s, SubscriptionImportStatus::Failed, "read_failed", "retry"))
        .collect::<Vec<_>>();
    let ticket = Uuid::new_v4();
    let ids = results.iter().map(|r| r.source_id).collect();
    *state.subscription_imports.current.lock().unwrap() = Some(Arc::new(Job {
        session: session_id(state).unwrap(),
        cancelled: AtomicBool::new(false),
        progress: Mutex::new(SubscriptionImportTask {
            ticket,
            phase: "complete".into(),
            total: results.len(),
            completed: results.len(),
            results,
        }),
        finished: Mutex::new(Some(Instant::now())),
    }));
    SubscriptionImportInput {
        retry: Some(SubscriptionImportRetry {
            ticket,
            source_ids: ids,
        }),
        ..Default::default()
    }
}

#[tokio::test]
async fn imports_are_encrypted_idempotent_and_preserve_entry_and_secret_ids() {
    let temp = tempfile::tempdir().unwrap();
    let state = state(&temp.path().join("vault"));
    let s = source("workbuddy", &temp.path().join("vendor"), "");
    write_workbuddy(&s.root, "workbuddy", "alice", "fake-import-private-access");
    let session = session_id(&state).unwrap();
    let mut revisions = crate::session::with_vault(&state, false, sources::snapshot)
        .unwrap()
        .revisions;
    let mut seen = HashMap::new();
    let first = persist::commit(
        &state,
        session,
        s.clone(),
        read(&s).await,
        &mut revisions,
        &mut seen,
    );
    assert_eq!(first.status, SubscriptionImportStatus::Imported);
    let before = summaries(&state).remove(0);
    let repeated = persist::commit(
        &state,
        session,
        s.clone(),
        read(&s).await,
        &mut revisions,
        &mut HashMap::new(),
    );
    assert_eq!(repeated.status, SubscriptionImportStatus::Existing);
    assert_eq!(repeated.entry_id, first.entry_id);
    assert_eq!(summaries(&state)[0].updated_at, before.updated_at);
    write_workbuddy(&s.root, "workbuddy", "alice", "fake-import-rotated-access");
    let same_binding = persist::commit(
        &state,
        session,
        s.clone(),
        read(&s).await,
        &mut revisions,
        &mut HashMap::new(),
    );
    assert_eq!(same_binding.status, SubscriptionImportStatus::Existing);
    assert_eq!(summaries(&state)[0].updated_at, before.updated_at);
    let alternate = source("workbuddy", &temp.path().join("alternate"), "");
    write_workbuddy(
        &alternate.root,
        "workbuddy",
        "alice",
        "fake-import-rotated-access",
    );
    let updated = persist::commit(
        &state,
        session,
        alternate.clone(),
        read(&alternate).await,
        &mut revisions,
        &mut HashMap::new(),
    );
    assert_eq!(updated.status, SubscriptionImportStatus::Updated);
    assert_eq!(updated.entry_id, first.entry_id);
    assert_eq!(
        summaries(&state)[0].secret_refs[0].id,
        before.secret_refs[0].id
    );
    let json = serde_json::to_string(&updated).unwrap();
    assert!(!json.contains("fake-import"));
    fn files(root: &Path) -> Vec<std::path::PathBuf> {
        std::fs::read_dir(root)
            .unwrap()
            .flatten()
            .flat_map(|entry| {
                if entry.path().is_dir() {
                    files(&entry.path())
                } else {
                    vec![entry.path()]
                }
            })
            .collect()
    }
    let stored = files(&temp.path().join("vault"));
    assert!(!stored.is_empty());
    for file in stored {
        let bytes = std::fs::read(file).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("fake-import"));
    }
}

#[tokio::test]
async fn account_edits_and_new_unlock_sessions_reject_delayed_commits() {
    let temp = tempfile::tempdir().unwrap();
    let state = state(&temp.path().join("vault"));
    let s = source("workbuddy", &temp.path().join("vendor"), "");
    write_workbuddy(&s.root, "workbuddy", "alice", "fixture-access");
    let session = session_id(&state).unwrap();
    let mut revisions = HashMap::new();
    let first = persist::commit(
        &state,
        session,
        s.clone(),
        read(&s).await,
        &mut revisions,
        &mut HashMap::new(),
    );
    let id = first.entry_id.unwrap();
    crate::session::with_vault(&state, false, |v| {
        v.set_provider_favorite(id, true).map_err(map_vault_error)
    })
    .unwrap();
    let stale = persist::commit(
        &state,
        session,
        s.clone(),
        read(&s).await,
        &mut revisions,
        &mut HashMap::new(),
    );
    assert_eq!(stale.error_code.as_deref(), Some("account_changed"));
    if let SessionState::Unlocked(info) = &mut *state.session.lock().unwrap() {
        info.id = Uuid::new_v4();
    }
    let late = persist::commit(
        &state,
        session,
        s.clone(),
        read(&s).await,
        &mut revisions,
        &mut HashMap::new(),
    );
    assert_eq!(late.status, SubscriptionImportStatus::Cancelled);
    assert_eq!(summaries(&state).len(), 1);
}

#[tokio::test]
async fn source_priority_keeps_an_existing_account_binding_and_separates_regions() {
    let temp = tempfile::tempdir().unwrap();
    let state = state(&temp.path().join("vault"));
    let a = source("workbuddy", &temp.path().join("original"), "");
    let b = source("workbuddy", &temp.path().join("other"), "");
    write_workbuddy(&a.root, "workbuddy", "alice", "first");
    write_workbuddy(&b.root, "workbuddy", "alice", "second");
    let session = session_id(&state).unwrap();
    let mut revisions = HashMap::new();
    let mut seen = HashMap::new();
    let first = persist::commit(
        &state,
        session,
        a.clone(),
        read(&a).await,
        &mut revisions,
        &mut seen,
    );
    let duplicate = persist::commit(
        &state,
        session,
        b.clone(),
        read(&b).await,
        &mut revisions,
        &mut seen,
    );
    assert_eq!(duplicate.status, SubscriptionImportStatus::Existing);
    assert_eq!(duplicate.entry_id, first.entry_id);
    let bound = crate::session::with_vault(&state, false, sources::snapshot)
        .unwrap()
        .sources;
    assert_eq!(bound, vec![sources::normalize(a.clone()).unwrap()]);
    let found = sources::discover(
        &SubscriptionImportInput {
            provider_ids: vec!["workbuddy".into()],
            sources: vec![b],
            retry: None,
        },
        &bound,
    )
    .unwrap();
    assert_eq!(found[0], sources::normalize(a).unwrap());
    let international = source("workbuddy-ai", &temp.path().join("international"), "");
    write_workbuddy(&international.root, "workbuddy-ai", "alice", "third");
    let other = persist::commit(
        &state,
        session,
        international.clone(),
        read(&international).await,
        &mut revisions,
        &mut seen,
    );
    assert_ne!(other.entry_id, first.entry_id);
    assert_eq!(summaries(&state).len(), 2);
}

#[test]
fn a_batch_continues_after_corrupt_input_and_retries_only_failures() {
    let temp = tempfile::tempdir().unwrap();
    let state = state(&temp.path().join("vault"));
    let good = source("workbuddy", &temp.path().join("good"), "");
    let bad = source("workbuddy-ai", &temp.path().join("bad"), "");
    write_workbuddy(&good.root, "workbuddy", "alice", "fixture-access");
    write_workbuddy(&bad.root, "workbuddy-ai", "bob", "fixture-access");
    std::fs::write(bad.root.join("workbuddy-desktop-ai.info"), "{bad").unwrap();
    let input = seed_retry(&state, vec![bad.clone(), good]);
    let task = state.subscription_imports.start(&state, input).unwrap();
    let result = wait(&state, task.ticket);
    assert_eq!(result.completed, 2);
    assert_eq!(result.results[0].status, SubscriptionImportStatus::Failed);
    assert_eq!(result.results[1].status, SubscriptionImportStatus::Imported);
    write_workbuddy(&bad.root, "workbuddy-ai", "bob", "fixture-access");
    let task = state
        .subscription_imports
        .start(
            &state,
            SubscriptionImportInput {
                retry: Some(SubscriptionImportRetry {
                    ticket: result.ticket,
                    source_ids: vec![result.results[0].source_id],
                }),
                ..Default::default()
            },
        )
        .unwrap();
    let retry = wait(&state, task.ticket);
    assert_eq!(retry.total, 1);
    assert_eq!(retry.results[0].status, SubscriptionImportStatus::Imported);
    assert_eq!(
        state
            .subscription_imports
            .poll(&state, result.ticket)
            .unwrap()
            .results,
        result.results
    );
    assert_eq!(summaries(&state).len(), 2);
}

#[test]
fn invalid_source_locations_do_not_abort_other_accounts() {
    let temp = tempfile::tempdir().unwrap();
    let state = state(&temp.path().join("vault"));
    let good = source("workbuddy", temp.path(), "");
    write_workbuddy(temp.path(), "workbuddy", "alice", "fixture-private");
    let input = seed_retry(
        &state,
        vec![source("workbuddy", Path::new("relative"), ""), good],
    );
    let task = state.subscription_imports.start(&state, input).unwrap();
    let result = wait(&state, task.ticket);
    assert_eq!(
        result.results[0].error_code.as_deref(),
        Some("invalid_source")
    );
    assert_eq!(result.results[1].status, SubscriptionImportStatus::Imported);
}
fn wait(state: &Arc<AgentState>, ticket: Uuid) -> SubscriptionImportTask {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let task = state.subscription_imports.poll(state, ticket).unwrap();
        if matches!(task.phase.as_str(), "complete" | "cancelled") {
            return task;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn cancellation_interrupts_stalled_identity_queries_and_keeps_four_source_limit() {
    let temp = tempfile::tempdir().unwrap();
    let state = state(&temp.path().join("vault"));
    let sources=(0..5).map(|i| {
        let root=temp.path().join(format!("vendor-{i}"));std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("credentials.toml"),format!("windsurf_api_key = 'fixture-private-{i}'\napi_server_url = 'https://provider.invalid'\n")).unwrap();
        source("devin",&root,"")
    }).collect::<Vec<_>>();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let (arrived, arrivals) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let proxy = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut sockets = Vec::new();
        while sockets.len() < 4 {
            match listener.accept() {
                Ok((socket, _)) => sockets.push(socket),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(e) => panic!("{e}"),
            }
            assert!(
                Instant::now() < deadline,
                "four sources did not reach the fixture proxy"
            );
        }
        // The fifth source must remain queued while the first four are blocked.
        std::thread::sleep(Duration::from_millis(200));
        assert!(listener.accept().is_err());
        arrived.send(()).unwrap();
        let _ = released.recv_timeout(Duration::from_secs(5));
        drop(sockets);
    });
    let input = seed_retry(&state, sources.clone());
    let ticket = input.retry.unwrap().ticket;
    let job = state
        .subscription_imports
        .current
        .lock()
        .unwrap()
        .clone()
        .unwrap();
    *job.finished.lock().unwrap() = None;
    {
        let mut progress = job.progress.lock().unwrap();
        progress.phase = "importing".into();
        progress.results.clear();
        progress.completed = 0;
    }
    let snapshot = crate::session::with_vault(&state, false, sources::snapshot).unwrap();
    let outbound =
        serde_json::from_value(json!({"mode":"custom","customUrl":format!("http://{address}")}))
            .unwrap();
    let weak = Arc::downgrade(&state);
    let worker = std::thread::spawn(move || {
        run(
            weak,
            job,
            Default::default(),
            Some(sources),
            snapshot,
            outbound,
        )
    });
    arrivals.recv_timeout(Duration::from_secs(6)).unwrap();
    let began = Instant::now();
    state.subscription_imports.cancel(&state, ticket).unwrap();
    let result = wait(&state, ticket);
    assert!(began.elapsed() < Duration::from_secs(2));
    assert_eq!(result.results.len(), 5);
    assert!(result
        .results
        .iter()
        .all(|r| r.status == SubscriptionImportStatus::Cancelled));
    assert!(summaries(&state).is_empty());
    assert!(!serde_json::to_string(&result)
        .unwrap()
        .contains("fixture-private"));
    let _ = release.send(());
    proxy.join().unwrap();
    worker.join().unwrap();
}

#[test]
fn cancelled_work_has_no_commit_authority_and_finished_tasks_expire() {
    let temp = tempfile::tempdir().unwrap();
    let state = state(&temp.path().join("vault"));
    let s = source("workbuddy", &temp.path().join("vendor"), "");
    write_workbuddy(&s.root, "workbuddy", "alice", "fixture-access");
    let input = seed_retry(&state, vec![s.clone()]);
    let old = state
        .subscription_imports
        .current
        .lock()
        .unwrap()
        .clone()
        .unwrap();
    *old.finished.lock().unwrap() = None;
    old.progress.lock().unwrap().phase = "importing".into();
    assert_eq!(
        state
            .subscription_imports
            .start(&state, Default::default())
            .unwrap_err()
            .code,
        AgentErrorCode::Conflict
    );
    state
        .subscription_imports
        .cancel(&state, input.retry.unwrap().ticket)
        .unwrap();
    old.progress.lock().unwrap().results.clear();
    let snapshot = crate::session::with_vault(&state, false, sources::snapshot).unwrap();
    run(
        Arc::downgrade(&state),
        old.clone(),
        Default::default(),
        Some(vec![s]),
        snapshot,
        Default::default(),
    );
    let task = old.progress.lock().unwrap().clone();
    assert_eq!(task.results[0].status, SubscriptionImportStatus::Cancelled);
    assert!(summaries(&state).is_empty());
    *old.finished.lock().unwrap() = Some(Instant::now() - Duration::from_secs(301));
    assert_eq!(
        state
            .subscription_imports
            .poll(&state, task.ticket)
            .unwrap_err()
            .code,
        AgentErrorCode::NotFound
    );
    state.subscription_imports.prune();
    assert!(state.subscription_imports.current.lock().unwrap().is_none());
    state.subscription_imports.clear();
    assert!(state.subscription_imports.current.lock().unwrap().is_none());
    *state.session.lock().unwrap() = SessionState::Locked;
    assert_eq!(
        state
            .subscription_imports
            .start(&state, Default::default())
            .unwrap_err()
            .code,
        AgentErrorCode::Locked
    );
}

#[test]
fn custom_roots_normalize_without_recursively_scanning_unrelated_directories() {
    let temp = tempfile::tempdir().unwrap();
    let s = source("gemini", temp.path(), "");
    let normalized = sources::normalize(s).unwrap();
    assert_eq!(normalized.provider, "gemini-cli");
    assert!(sources::normalize(source("codex", Path::new("relative"), "")).is_err());
    assert!(sources::normalize(source("codex", &temp.path().join("../foreign"), "")).is_err());
    assert!(sources::normalize(source("factory", temp.path(), "")).is_err());
    assert!(sources::normalize(source("cursor", temp.path(), "foreign-store")).is_err());
}

#[tokio::test]
async fn every_provider_reports_missing_or_damaged_sources_without_exposing_file_contents() {
    let temp = tempfile::tempdir().unwrap();
    for provider in [
        "zcode",
        "devin",
        "commandcode-plan",
        "cursor",
        "kiro",
        "workbuddy",
        "workbuddy-ai",
    ] {
        let s = source(
            provider,
            temp.path(),
            if provider == "cursor" {
                "file"
            } else if provider == "kiro" {
                "ide"
            } else {
                ""
            },
        );
        let file = crate::subscriptions::native_import::required_file(&s).unwrap();
        assert_eq!(
            reader::read(&s, &Default::default()).await.err().unwrap().0,
            SubscriptionImportStatus::NotFound
        );
        std::fs::write(&file, "fake-private-malformed-input").unwrap();
        let error = reader::read(&s, &Default::default()).await.err().unwrap();
        assert!(matches!(
            error.0,
            SubscriptionImportStatus::Failed | SubscriptionImportStatus::NeedsLogin
        ));
        assert!(!error.1.contains("fake-private"));
        std::fs::remove_file(file).unwrap();
    }
}

#[tokio::test]
async fn expired_native_sources_do_not_start_identity_queries_or_refresh_grants() {
    let temp = tempfile::tempdir().unwrap();
    // No profile/user ID: expiry must be checked before the identity-only query.
    std::fs::write(temp.path().join("kiro-auth-token.json"),json!({"accessToken":"fixture-private","refreshToken":"fixture-refresh","expiresAt":"2000-01-01T00:00:00Z"}).to_string()).unwrap();
    let error = reader::read(&source("kiro", temp.path(), "ide"), &Default::default())
        .await
        .err()
        .unwrap();
    assert_eq!(error.0, SubscriptionImportStatus::NeedsLogin);
    std::fs::write(
        temp.path().join("auth.json"),
        json!({"accessToken":"x.eyJleHAiOjEsInN1YiI6ImFsaWNlIn0.x"}).to_string(),
    )
    .unwrap();
    let error = reader::read(&source("cursor", temp.path(), "file"), &Default::default())
        .await
        .err()
        .unwrap();
    assert_eq!(error.0, SubscriptionImportStatus::NeedsLogin);
}

#[cfg(unix)]
#[tokio::test]
async fn refused_file_access_reports_permissions_instead_of_missing_login() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    write_workbuddy(temp.path(), "workbuddy", "alice", "fixture-private");
    let file = temp.path().join("workbuddy-desktop.info");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
    let denied = std::fs::File::open(&file).is_err();
    let result = reader::read(&source("workbuddy", temp.path(), ""), &Default::default()).await;
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    if denied {
        assert_eq!(result.err().unwrap().1, "permission_denied");
    }
}

#[tokio::test]
async fn devin_and_commandcode_read_explicit_directories_without_interactive_login() {
    let temp = tempfile::tempdir().unwrap();
    let token = "x.eyJzdWIiOiJhbGljZSJ9.x";
    std::fs::write(
        temp.path().join("credentials.toml"),
        format!("windsurf_api_key = '{token}'\n"),
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        json!({"apiKey":"fixture-private-key","userId":"bob","email":"bob@example.test"})
            .to_string(),
    )
    .unwrap();
    for (provider, expected) in [("devin", "alice"), ("commandcode-plan", "bob@example.test")] {
        let s = source(provider, temp.path(), "");
        let reader::Account::Auth(auth) = read(&s).await else {
            panic!()
        };
        let private = reader::PrivateAuth(serde_json::from_str(auth.expose()).unwrap());
        assert_eq!(
            crate::community::identity(&private.0).as_deref(),
            Some(expected)
        );
        assert_eq!(
            private.0["nativeSource"]["root"],
            temp.path().canonicalize().unwrap().to_str().unwrap()
        );
        assert_eq!(private.0["nativeDevice"], "local-test-device");
    }
}

#[tokio::test]
async fn kiro_enumerates_each_cli_login_and_the_ide_without_using_a_default_store() {
    let temp = tempfile::tempdir().unwrap();
    let db = rusqlite::Connection::open(temp.path().join("data.sqlite3")).unwrap();
    db.execute(
        "CREATE TABLE auth_kv (key TEXT PRIMARY KEY, value TEXT)",
        [],
    )
    .unwrap();
    for kind in ["social", "odic", "external-idp"] {
        let value = json!({"access_token":format!("fixture-{kind}"),"refresh_token":"fixture-refresh","expires_at":"2040-01-01T00:00:00Z","profile_arn":format!("arn:fixture:{kind}")});
        db.execute(
            "INSERT INTO auth_kv VALUES (?1,?2)",
            rusqlite::params![format!("kirocli:{kind}:token"), value.to_string()],
        )
        .unwrap();
    }
    let cli = crate::subscriptions::native_import::expand(source("kiro", temp.path(), "")).unwrap();
    assert_eq!(cli.len(), 3);
    let mut identities = Vec::new();
    for s in cli {
        let reader::Account::Auth(auth) = read(&s).await else {
            panic!()
        };
        let private = reader::PrivateAuth(serde_json::from_str(auth.expose()).unwrap());
        identities.push(crate::community::identity(&private.0).unwrap());
        assert_eq!(private.0["nativeSource"]["selector"], s.selector);
    }
    assert_eq!(identities.iter().collect::<HashSet<_>>().len(), 3);
    std::fs::write(temp.path().join("kiro-auth-token.json"), json!({"accessToken":"fixture-ide","refreshToken":"fixture-refresh","expiresAt":"2040-01-01T00:00:00Z","profileArn":"arn:fixture:ide"}).to_string()).unwrap();
    let reader::Account::Auth(auth) = read(&source("kiro", temp.path(), "ide")).await else {
        panic!()
    };
    let private = reader::PrivateAuth(serde_json::from_str(auth.expose()).unwrap());
    assert_eq!(
        crate::community::identity(&private.0).as_deref(),
        Some("arn:fixture:ide")
    );
    assert_eq!(private.0["source"], "kiro-ide");
    let combined =
        crate::subscriptions::native_import::expand(source("kiro", temp.path(), "")).unwrap();
    assert_eq!(combined.len(), 4);
}

#[test]
fn legacy_refresh_keeps_unsupported_provider_filter_semantics() {
    let temp = tempfile::tempdir().unwrap();
    let state = state(&temp.path().join("vault"));
    assert!(
        refresh_compat(&state, vec!["factory".into(), "zcode".into()])
            .unwrap()
            .is_empty()
    );
    assert!(state.subscription_imports.current.lock().unwrap().is_none());
}

#[test]
fn unloading_the_agent_cancels_import_work_and_drops_retained_results() {
    let temp = tempfile::tempdir().unwrap();
    let state = state(&temp.path().join("vault"));
    seed_retry(&state, vec![source("workbuddy", temp.path(), "")]);
    let job = state
        .subscription_imports
        .current
        .lock()
        .unwrap()
        .clone()
        .unwrap();
    drop(state);
    assert!(job.cancelled.load(Ordering::Acquire));
}

#[test]
fn legacy_zcode_identity_upgrades_only_its_matching_site_and_preserves_ids() {
    let temp = tempfile::tempdir().unwrap();
    let state = state(&temp.path().join("vault"));
    let scope = json!({"site":"zai","org":"org-a","project":"project-a"}).to_string();
    let old = json!({"type":"oauth","access":"fixture-key","accountId":"alice","refresh":scope,"expires":0});
    let id = crate::session::with_vault(&state, true, |v| {
        crate::community::register_cli(v, "zcode", old)
    })
    .unwrap();
    let before = summaries(&state).remove(0);
    let snapshot = crate::session::with_vault(&state, false, sources::snapshot).unwrap();
    let s = source("zcode", temp.path(), "team:zai");
    let auth = json!({"type":"oauth","access":"fixture-key","accountId":"alice::zai::org-a::project-a","refresh":scope,"expires":0,"nativeSource":s,"nativeDevice":"local-test-device"});
    let result = persist::commit(
        &state,
        session_id(&state).unwrap(),
        s,
        reader::Account::Auth(SensitiveString::new(auth.to_string())),
        &mut snapshot.revisions.clone(),
        &mut HashMap::new(),
    );
    assert_eq!(result.status, SubscriptionImportStatus::Updated);
    assert_eq!(result.entry_id, Some(id));
    let after = summaries(&state).remove(0);
    assert_eq!(after.secret_refs[0].id, before.secret_refs[0].id);
    assert_eq!(
        after.account_identity.as_deref(),
        Some("alice::zai::org-a::project-a")
    );
}

#[tokio::test]
async fn zcode_keeps_site_and_team_scopes_and_records_its_actual_native_method() {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use sha2::{Digest, Sha256};
    let temp = tempfile::tempdir().unwrap();
    let h = directories::BaseDirs::new().unwrap().home_dir().to_owned();
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default();
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(windows) {
        "win32"
    } else {
        "linux"
    };
    let seed = std::env::var("ZCODE_CREDENTIAL_SECRET").unwrap_or_else(|_| {
        format!(
            "zcode-credential-fallback:{os}:{}:{}",
            h.display(),
            user.rsplit('\\').next().unwrap_or("")
        )
    });
    let key = Sha256::digest(seed.as_bytes());
    let encrypt = |text: &str| {
        let nonce = [7_u8; 12];
        let mut cipher = Aes256Gcm::new_from_slice(&key)
            .unwrap()
            .encrypt(aes_gcm::Nonce::from_slice(&nonce), text.as_bytes())
            .unwrap();
        let tag = cipher.split_off(cipher.len() - 16);
        format!(
            "enc:v1:{}.{}.{}",
            URL_SAFE_NO_PAD.encode(nonce),
            URL_SAFE_NO_PAD.encode(tag),
            URL_SAFE_NO_PAD.encode(cipher)
        )
    };
    let store = json!({
        "alice:coding-plan:zai:api-key":encrypt("fixture.zai.key"),
        "alice:coding-plan:bigmodel-default:api-key":encrypt("fixture.bigmodel.key"),
        "bob:coding-plan:zai:api-key":encrypt("fixture.bob.key"),
        "oauth:zai:user_info":encrypt(r#"{"user_id":"alice"}"#),
        "oauth:bigmodel:user_info":encrypt(r#"{"user_id":"alice"}"#),
        "oauth:zai:access_token":encrypt("fixture-team-token")
    });
    std::fs::write(temp.path().join("credentials.json"), store.to_string()).unwrap();
    std::fs::write(temp.path().join("setting.json"), json!({"providerFamilyConnectionSelections":{"zai":{"kind":"team-coding-plan","organizationId":"org-a","projectId":"project-a"}}}).to_string()).unwrap();
    let sources = sources::retry_sources(vec![source("zcode", temp.path(), "")]).unwrap();
    assert_eq!(sources.len(), 4);
    let state = state(&temp.path().join("vault"));
    let session = session_id(&state).unwrap();
    let mut revisions = HashMap::new();
    let mut seen = HashMap::new();
    for s in sources {
        let imported = persist::commit(
            &state,
            session,
            s.clone(),
            read(&s).await,
            &mut revisions,
            &mut seen,
        );
        assert_eq!(imported.status, SubscriptionImportStatus::Imported);
        crate::session::with_vault(&state, false, |v| {
            let bundle = v
                .provider_runtime_extension(imported.entry_id.unwrap(), "community_account_v1")
                .map_err(map_vault_error)?
                .unwrap();
            let data: Value = serde_json::from_str(bundle.expose()).unwrap();
            assert_eq!(data["nativeMethod"], 2);
            Ok(())
        })
        .unwrap();
    }
    let names = summaries(&state)
        .into_iter()
        .map(|e| e.account_identity.unwrap())
        .collect::<Vec<_>>();
    assert!(names.contains(&"alice::zai::::".into()));
    assert!(names.contains(&"alice::bigmodel::::".into()));
    assert!(names.contains(&"alice::zai::org-a::project-a".into()));
    assert!(names.contains(&"bob::zai::::".into()));
}
