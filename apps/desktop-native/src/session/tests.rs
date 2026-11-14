use super::*;
use aipass_agent_protocol::SessionPolicy;
use std::time::{Duration, Instant};
use std::{sync::mpsc, thread};

fn status(locked: bool) -> SessionStatus {
    SessionStatus {
        exists: true,
        locked,
        policy: SessionPolicy::default(),
        last_lock_reason: None,
        vault_namespace: None,
        initial_sync_pending: false,
        initial_sync_failed: false,
        sync_revision: 0,
        sync_status: None,
    }
}

struct Controlled {
    seen: Sender<Command>,
    results: Receiver<Result<Reply, AgentCommandError>>,
}
impl Transport for Controlled {
    fn execute(&mut self, command: Command) -> Result<Reply, AgentCommandError> {
        self.seen
            .send(command)
            .expect("test command observer alive");
        self.results.recv().expect("test response producer alive")
    }
}

fn fixture() -> (
    Session,
    Receiver<Command>,
    Sender<Result<Reply, AgentCommandError>>,
) {
    let (seen, commands) = mpsc::channel();
    let (results, responses) = mpsc::channel();
    (
        Session::new(Controlled {
            seen,
            results: responses,
        })
        .unwrap(),
        commands,
        results,
    )
}

#[test]
fn integration_transport_failure_keeps_scope_and_successful_login_requests_catalog_refresh() {
    let (mut session, commands, results) = fixture();
    session.status = Some(status(false));
    let scope = crate::integration::Scope {
        entry: Uuid::new_v4(),
        generation: 42,
    };
    let ticket = Uuid::new_v4();
    for success in [false, true] {
        assert!(session.submit(Command::Integration(Box::new(
            crate::integration::Request {
                scope,
                operation: crate::integration::Operation::LoginPoll(ticket),
            }
        ))));
        commands.recv_timeout(Duration::from_secs(1)).unwrap();
        if success {
            results
                .send(Ok(Reply::Integration(Box::new(
                    crate::integration::Completion {
                        scope,
                        result: Ok(crate::integration::Outcome::Login(
                            aipass_agent_protocol::ToolConfigLoginStatus {
                                ticket,
                                status: "complete".into(),
                                url: None,
                                user_code: None,
                                requires_code: false,
                                message: None,
                                result: Some(aipass_agent_protocol::ToolConfigApplyResponse {
                                    outcome: aipass_agent_protocol::ToolConfigOutcome::Applied,
                                    message: None,
                                    tool: aipass_agent_protocol::ToolConfigTool::Codex,
                                    mode: aipass_agent_protocol::ToolConfigMode::Official,
                                    entry_id: scope.entry,
                                    entry_title: "fixture".into(),
                                    operation_id: Uuid::new_v4(),
                                    target_path: String::new(),
                                    backup_path: String::new(),
                                    summary: String::new(),
                                }),
                            },
                        )),
                    },
                ))))
                .unwrap();
        } else {
            results
                .send(Err(AgentCommandError {
                    code: Some(AgentErrorCode::ServiceUnavailable),
                    message: "offline".into(),
                }))
                .unwrap();
        }
        settle(&mut session);
        let completion = session.integration_completion.as_ref().unwrap();
        assert_eq!(completion.scope, scope);
        assert_eq!(completion.result.is_ok(), success);
        assert_eq!(session.provider_applied, success);
    }
}

fn settle(session: &mut Session) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while session.pending && Instant::now() < deadline {
        session.poll();
        thread::yield_now();
    }
    assert!(
        !session.pending,
        "worker failed to complete before test deadline"
    );
}

#[test]
fn subscription_startup_failure_returns_to_the_panel_and_lock_rejects_new_work() {
    use crate::subscriptions::{Kind, Operation, Request};
    let (mut session, commands, results) = fixture();
    session.status = Some(status(false));
    assert!(session.submit(Command::Subscription(Box::new(Request {
        generation: 42,
        operation: Operation::Catalog
    }))));
    commands.recv_timeout(Duration::from_secs(1)).unwrap();
    results
        .send(Err(AgentCommandError {
            code: Some(AgentErrorCode::ServiceUnavailable),
            message: "synthetic startup failure".into(),
        }))
        .unwrap();
    settle(&mut session);
    let completion = session.subscription_completion.take().unwrap();
    assert_eq!(completion.generation, 42);
    assert_eq!(completion.kind, Kind::Catalog);
    assert!(completion.result.is_err());
    session.status = Some(status(true));
    assert!(!session.submit(Command::Subscription(Box::new(Request {
        generation: 43,
        operation: Operation::Catalog
    }))));
}

#[test]
fn pricing_background_reads_keep_scope_on_startup_failure_and_fail_closed_on_lock() {
    use crate::pricing::{Operation, Request, Scope};
    let (mut session, commands, results) = fixture();
    session.status = Some(status(false));
    let scope = Scope {
        entry: Uuid::new_v4(),
        generation: 81,
    };
    for code in [AgentErrorCode::ServiceUnavailable, AgentErrorCode::Locked] {
        assert!(session.submit(Command::Pricing(Box::new(Request {
            scope,
            operation: Operation::Load
        }))));
        assert!(!session.foreground_pending);
        commands.recv_timeout(Duration::from_secs(1)).unwrap();
        results
            .send(Err(AgentCommandError {
                code: Some(code.clone()),
                message: "Synthetic pricing failure".into(),
            }))
            .unwrap();
        settle(&mut session);
        let completion = session.pricing_completion.take().unwrap();
        assert_eq!(completion.scope, scope);
        assert_eq!(completion.result.unwrap_err().code, Some(code));
    }
    assert!(!session.is_unlocked());
    assert!(!session.submit(Command::Pricing(Box::new(Request {
        scope,
        operation: Operation::Load
    }))));
}

#[test]
fn locking_drops_a_late_credential_read_and_rejects_further_operations() {
    use crate::credentials::{Completion, Operation, Outcome, Request, Scope};
    let (mut session, commands, results) = fixture();
    session.status = Some(status(false));
    let scope = Scope {
        entry: Uuid::new_v4(),
        generation: 7,
    };
    assert!(session.submit(Command::Credential(Box::new(Request {
        scope,
        operation: Operation::Reveal("stable-id".into())
    }))));
    commands.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(session.submit(Command::Lock));
    assert!(!session.submit(Command::Credential(Box::new(Request {
        scope,
        operation: Operation::Remove("stable-id".into())
    }))));
    assert!(results
        .send(Ok(Reply::Credential(Completion {
            scope,
            result: Ok(Outcome::Revealed("synthetic-stale-key".into()))
        })))
        .is_ok());
    assert!(matches!(
        commands.recv_timeout(Duration::from_secs(1)).unwrap(),
        Command::Lock
    ));
    assert!(results.send(Ok(Reply::Locked(status(true)))).is_ok());
    settle(&mut session);
    assert!(session.credential_completion.is_none());
    assert!(!session.is_unlocked());
}

#[test]
fn credential_errors_retain_scope_and_locked_errors_hide_the_catalog() {
    use crate::credentials::{Completion, Operation, Request, Scope};
    for code in [AgentErrorCode::Internal, AgentErrorCode::Locked] {
        let (mut session, commands, results) = fixture();
        session.status = Some(status(false));
        session.catalog = Catalog::fixture();
        let scope = Scope {
            entry: session.catalog.active[0].id,
            generation: 4,
        };
        assert!(session.submit(Command::Credential(Box::new(Request {
            scope,
            operation: Operation::Remove("stable-id".into())
        }))));
        commands.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(results
            .send(Ok(Reply::Credential(Completion {
                scope,
                result: Err(AgentCommandError {
                    code: Some(code.clone()),
                    message: "synthetic failure".into()
                })
            })))
            .is_ok());
        settle(&mut session);
        assert!(session
            .credential_completion
            .as_ref()
            .is_some_and(|result| result.scope == scope && result.result.is_err()));
        assert!(!session.provider_applied);
        assert_eq!(session.is_unlocked(), code != AgentErrorCode::Locked);
        assert_eq!(
            session.catalog.active.is_empty(),
            code == AgentErrorCode::Locked
        );
    }
}

#[test]
fn lock_discards_late_integration_preview_and_rejects_new_writes() {
    use crate::integration::{Completion, Operation, Outcome, Request, Scope};
    let (mut session, commands, results) = fixture();
    session.status = Some(status(false));
    let scope = Scope {
        entry: Uuid::new_v4(),
        generation: 3,
    };
    assert!(session.submit(Command::Integration(Box::new(Request {
        scope,
        operation: Operation::Scan
    }))));
    commands.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(session.submit(Command::Lock));
    assert!(!session.submit(Command::Integration(Box::new(Request {
        scope,
        operation: Operation::Rollback(Uuid::new_v4())
    }))));
    results
        .send(Ok(Reply::Integration(Box::new(Completion {
            scope,
            result: Ok(Outcome::Scanned {
                detections: vec![],
                statuses: vec![],
            }),
        }))))
        .ok()
        .unwrap();
    assert!(matches!(
        commands.recv_timeout(Duration::from_secs(1)).unwrap(),
        Command::Lock
    ));
    results.send(Ok(Reply::Locked(status(true)))).ok().unwrap();
    settle(&mut session);
    assert!(session.integration_completion.is_none());
    assert!(!session.is_unlocked());
}

#[test]
fn lock_is_ordered_after_auth_and_stale_completion_cannot_reveal_workspace() {
    let (mut session, commands, results) = fixture();
    session.status = Some(status(true));
    assert!(session.submit(Command::Unlock("fixture-password".into())));
    assert!(matches!(
        commands.recv_timeout(Duration::from_secs(1)).unwrap(),
        Command::Unlock(_)
    ));
    assert!(session.submit(Command::Lock));
    assert!(!session.is_unlocked());
    assert!(results
        .send(Ok(Reply::Snapshot {
            status: status(false),
            entries: vec![]
        }))
        .is_ok());
    assert!(matches!(
        commands.recv_timeout(Duration::from_secs(1)).unwrap(),
        Command::Lock
    ));
    session.poll();
    assert!(!session.is_unlocked());
    assert!(session.pending);
    assert!(results.send(Ok(Reply::Locked(status(true)))).is_ok());
    settle(&mut session);
    assert!(!session.is_unlocked());
    assert!(!session.lock_pending);
}

#[test]
fn failed_lock_remains_hidden_and_can_be_retried() {
    let (mut session, commands, results) = fixture();
    session.status = Some(status(false));
    assert!(session.submit(Command::Lock));
    commands.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(results
        .send(Err(AgentCommandError {
            code: Some(AgentErrorCode::ServiceUnavailable),
            message: "fixture failure".into()
        }))
        .is_ok());
    settle(&mut session);
    assert!(!session.is_unlocked());
    assert!(!session.submit(Command::Refresh));
    assert!(session.submit(Command::Lock));
    commands.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(results.send(Ok(Reply::Locked(status(true)))).is_ok());
    settle(&mut session);
    assert!(!session.lock_pending);
}

#[test]
fn repeated_submit_does_not_queue_duplicate_authentication() {
    let (mut session, commands, results) = fixture();
    assert!(session.submit(Command::Unlock("first".into())));
    commands.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(!session.submit(Command::Unlock("second".into())));
    assert!(results
        .send(Err(AgentCommandError {
            code: Some(AgentErrorCode::InvalidPassword),
            message: "invalid password".into()
        }))
        .is_ok());
    settle(&mut session);
    assert!(matches!(commands.try_recv(), Err(TryRecvError::Empty)));
    assert_eq!(
        session.error.as_ref().unwrap().code,
        Some(AgentErrorCode::InvalidPassword)
    );
}

#[test]
fn worker_loss_clears_pending_state_and_fails_closed() {
    struct Exits;
    impl Transport for Exits {
        fn execute(&mut self, _: Command) -> Result<Reply, AgentCommandError> {
            panic!("simulated worker failure")
        }
    }
    let mut session = Session::new(Exits).unwrap();
    session.status = Some(status(false));
    session.submit(Command::Refresh);
    settle(&mut session);
    assert!(!session.is_unlocked());
    assert!(!session.submit(Command::Refresh));
}

#[test]
fn queued_lock_survives_view_teardown_during_authentication() {
    let (mut session, commands, results) = fixture();
    assert!(session.submit(Command::Unlock("fixture-password".into())));
    commands.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(session.submit(Command::Lock));
    drop(session);
    assert!(results
        .send(Ok(Reply::Snapshot {
            status: status(false),
            entries: vec![]
        }))
        .is_ok());
    assert!(matches!(
        commands.recv_timeout(Duration::from_secs(1)).unwrap(),
        Command::Lock
    ));
    assert!(results.send(Ok(Reply::Locked(status(true)))).is_ok());
}

#[test]
fn lock_requires_an_acknowledged_locked_state() {
    let (mut session, commands, results) = fixture();
    session.status = Some(status(false));
    assert!(session.submit(Command::Lock));
    commands.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(results.send(Ok(Reply::Locked(status(false)))).is_ok());
    settle(&mut session);
    assert!(!session.is_unlocked());
    assert!(session.lock_pending);
    assert!(session.error.is_some());
    assert!(session.submit(Command::Lock));
    commands.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(results.send(Ok(Reply::Locked(status(true)))).is_ok());
    settle(&mut session);
}

#[test]
fn reset_waits_for_acknowledgement_and_clears_recovery_material() {
    let (mut session, commands, results) = fixture();
    session.status = Some(status(true));
    session.recovery = Some("fixture-recovery".into());
    assert!(session.submit(Command::Reset));
    assert!(matches!(
        commands.recv_timeout(Duration::from_secs(1)).unwrap(),
        Command::Reset
    ));
    assert!(!session.submit(Command::Reset));
    assert!(session.status.as_ref().unwrap().exists);
    let mut removed = status(true);
    removed.exists = false;
    results.send(Ok(Reply::Reset(removed))).unwrap();
    settle(&mut session);
    assert!(!session.status.as_ref().unwrap().exists);
    assert!(session.recovery.is_none());
    assert!(!session.is_unlocked());
    assert!(session.error.is_none());
}

#[test]
fn reset_failure_keeps_the_vault_and_allows_explicit_retry() {
    let (mut session, commands, results) = fixture();
    session.status = Some(status(true));
    assert!(session.submit(Command::Reset));
    commands.recv_timeout(Duration::from_secs(1)).unwrap();
    results
        .send(Err(AgentCommandError {
            code: Some(AgentErrorCode::Internal),
            message: "fixture reset failure".into(),
        }))
        .unwrap();
    settle(&mut session);
    assert!(session.status.as_ref().unwrap().exists);
    assert!(session.error.is_some());
    assert!(!session.is_unlocked());
    assert!(session.submit(Command::Reset));
    commands.recv_timeout(Duration::from_secs(1)).unwrap();
    results.send(Ok(Reply::Reset(status(false)))).unwrap();
    settle(&mut session);
    assert!(session.status.as_ref().unwrap().exists);
    assert!(!session.is_unlocked());
    assert!(session.error.is_some());
}

#[test]
fn provider_completion_does_not_reopen_a_locked_catalog() {
    let (mut session, commands, results) = fixture();
    session.status = Some(status(false));
    session.catalog = Catalog::fixture();
    let id = session.catalog.active[0].id;
    assert!(session.submit(Command::Provider(ProviderAction::Favorite(id, true))));
    assert!(matches!(
        commands.recv_timeout(Duration::from_secs(1)).unwrap(),
        Command::Provider(_)
    ));
    assert!(!session.submit(Command::Provider(ProviderAction::Favorite(id, true))));
    assert!(session.submit(Command::Lock));
    assert!(session.catalog.active.is_empty());
    results
        .send(Ok(Reply::ProviderApplied {
            selected: Some(id),
            saved: true,
        }))
        .unwrap();
    assert!(matches!(
        commands.recv_timeout(Duration::from_secs(1)).unwrap(),
        Command::Lock
    ));
    session.poll();
    assert!(!session.provider_saved);
    assert!(session.selected.is_none());
    assert!(session.catalog.active.is_empty());
    results.send(Ok(Reply::Locked(status(true)))).unwrap();
    settle(&mut session);
    assert!(!session.submit(Command::Provider(ProviderAction::Restore(id))));
}

#[test]
fn provider_acknowledgement_is_independent_of_a_failed_followup_refresh() {
    let (mut session, commands, results) = fixture();
    session.status = Some(status(false));
    let mut draft = crate::catalog::draft::Draft::new(None);
    draft.api_key = "synthetic-key".into();
    assert!(session.submit(Command::Provider(draft.into_action().unwrap())));
    commands.recv_timeout(Duration::from_secs(1)).unwrap();
    let id = Uuid::new_v4();
    results
        .send(Ok(Reply::ProviderApplied {
            selected: Some(id),
            saved: true,
        }))
        .unwrap();
    settle(&mut session);
    assert!(session.provider_saved);
    assert!(session.provider_applied);
    assert_eq!(session.selected, Some(id));
    assert!(session.submit(Command::Refresh));
    commands.recv_timeout(Duration::from_secs(1)).unwrap();
    results
        .send(Err(AgentCommandError {
            code: Some(AgentErrorCode::ServiceUnavailable),
            message: "synthetic refresh failure".into(),
        }))
        .unwrap();
    settle(&mut session);
    assert!(!session.provider_saved);
    assert!(session.is_unlocked());
    assert!(session.error.is_some());
    assert!(matches!(commands.try_recv(), Err(TryRecvError::Empty)));
}

#[test]
fn locked_missing_or_sync_pending_snapshots_never_expose_catalog_data() {
    for mode in 0..3 {
        let (mut session, commands, results) = fixture();
        session.status = Some(status(false));
        session.catalog = Catalog::fixture();
        assert!(session.submit(Command::Refresh));
        commands.recv_timeout(Duration::from_secs(1)).unwrap();
        let mut reply = status(mode == 0);
        reply.exists = mode != 1;
        reply.initial_sync_pending = mode == 2;
        results
            .send(Ok(Reply::Catalog {
                sync_settings: None,
                status: reply,
                catalog: Box::new(Catalog::fixture()),
                selected: None,
                saved: false,
            }))
            .unwrap();
        settle(&mut session);
        assert!(!session.is_unlocked());
        assert!(session.catalog.active.is_empty());
        assert!(session.catalog.archived.is_empty());
        assert!(session.catalog.trash.is_empty());
    }
}

#[test]
fn credential_transport_failure_settles_the_requesting_panel() {
    let (mut session, commands, results) = fixture();
    session.status = Some(status(false));
    let scope = crate::credentials::Scope {
        entry: Uuid::new_v4(),
        generation: 17,
    };
    assert!(
        session.submit(Command::Credential(Box::new(crate::credentials::Request {
            scope,
            operation: crate::credentials::Operation::Reveal("fixture-key".into())
        })))
    );
    commands.recv_timeout(Duration::from_secs(1)).unwrap();
    results
        .send(Err(AgentCommandError {
            code: Some(AgentErrorCode::ServiceUnavailable),
            message: "offline".into(),
        }))
        .unwrap();
    settle(&mut session);
    assert!(!session.pending);
    let completion = session.credential_completion.as_ref().unwrap();
    assert!(completion.scope == scope);
    assert_eq!(completion.result.as_ref().err().unwrap().message, "offline");
}

#[test]
fn trash_failure_settles_confirmation_and_refreshes_possible_partial_deletion() {
    let (mut session, commands, results) = fixture();
    session.status = Some(status(false));
    assert!(session.submit(Command::Trash(crate::trash::Request {
        generation: 31,
        target: None
    })));
    commands.recv_timeout(Duration::from_secs(1)).unwrap();
    results
        .send(Err(AgentCommandError {
            code: Some(AgentErrorCode::ServiceUnavailable),
            message: "offline".into(),
        }))
        .unwrap();
    settle(&mut session);
    let completion = session.trash_completion.as_ref().unwrap();
    assert_eq!(completion.generation, 31);
    assert!(completion.result.is_err());
    assert!(!session.pending);
    assert!(session.provider_applied);
}

#[test]
fn activity_requires_recent_unlocked_input_and_does_not_block_foreground_ui() {
    let (mut session, commands, results) = fixture();
    session.record_activity();
    session.status = Some(status(false));
    assert!(!session.submit_activity());
    session.record_activity();
    assert!(session.submit_activity());
    assert!(!session.foreground_pending);
    assert!(matches!(
        commands.recv_timeout(Duration::from_secs(1)).unwrap(),
        Command::Touch
    ));
    results.send(Ok(Reply::Touched(status(false)))).unwrap();
    settle(&mut session);
    assert!(!session.submit_activity());
    assert!(session.is_unlocked());
    // Transport failures in best-effort keepalive should not replace UI feedback.
    assert!(session.submit(Command::Touch));
    commands.recv_timeout(Duration::from_secs(1)).unwrap();
    results
        .send(Err(AgentCommandError {
            code: Some(AgentErrorCode::ServiceUnavailable),
            message: "offline".into(),
        }))
        .unwrap();
    settle(&mut session);
    assert!(session.error.is_none());
    assert!(session.submit(Command::Touch));
    commands.recv_timeout(Duration::from_secs(1)).unwrap();
    results
        .send(Err(AgentCommandError {
            code: Some(AgentErrorCode::Locked),
            message: "locked".into(),
        }))
        .unwrap();
    settle(&mut session);
    assert!(!session.is_unlocked());
    session.record_activity();
    assert!(!session.submit_activity());
}

#[test]
fn setup_metadata_failure_preserves_loaded_session_status() {
    let (mut session, commands, results) = fixture();
    assert!(session.submit(Command::Refresh));
    commands.recv_timeout(Duration::from_secs(1)).unwrap();
    let mut ready = status(true);
    ready.exists = false;
    results
        .send(Ok(Reply::Catalog {
            sync_settings: Some(Err(AgentCommandError {
                code: Some(AgentErrorCode::ServiceUnavailable),
                message: "synthetic setup metadata failure".into(),
            })),
            status: ready,
            catalog: Box::default(),
            selected: None,
            saved: false,
        }))
        .unwrap();
    settle(&mut session);
    assert!(!session.status.as_ref().unwrap().exists);
    assert!(session.error.is_some());
    assert!(!session.pending);
}
