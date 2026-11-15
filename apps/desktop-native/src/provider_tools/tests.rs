use super::*;
fn entry() -> EntrySummary {
    crate::catalog::Catalog::fixture().active.remove(0)
}
fn failure() -> AgentCommandError {
    AgentCommandError {
        code: Some(AgentErrorCode::ServiceUnavailable),
        message: "fixture unavailable".into(),
    }
}
fn request(entry: &EntrySummary, operation: Operation) -> Request {
    Request {
        scope: Scope {
            entry: entry.id,
            generation: 7,
        },
        operation,
    }
}
fn usage(value: serde_json::Value) -> UsageProbeResult {
    serde_json::from_value(value).unwrap()
}
#[test]
fn reset_only_removes_confirmed_ids_and_retry_skips_completed_removals() {
    let mut entry = entry();
    assert!(entry.secret_refs.len() > 1);
    let original = entry.clone();
    let mut removals = vec![];
    let result = request(
        &entry,
        Operation::Reset {
            secrets: entry.secret_refs.clone(),
            archived: false,
        },
    )
    .execute_with(|call| match call {
        AgentRequest::EntriesList { .. } => Ok(serde_json::to_value(vec![entry.clone()]).unwrap()),
        AgentRequest::SecretRemove { label, .. } => {
            if !removals.is_empty() {
                return Err(failure());
            }
            entry.secret_refs.retain(|s| s.id != label);
            removals.push(label);
            Ok(serde_json::Value::Null)
        }
        _ => panic!("unexpected request"),
    });
    assert!(result.mutated && result.result.is_err());
    let result = request(
        &entry,
        Operation::Reset {
            secrets: original.secret_refs.clone(),
            archived: false,
        },
    )
    .execute_with(|call| match call {
        AgentRequest::EntriesList { .. } => Ok(serde_json::to_value(vec![entry.clone()]).unwrap()),
        AgentRequest::SecretRemove { label, .. } => {
            assert!(!removals.contains(&label));
            entry.secret_refs.retain(|s| s.id != label);
            removals.push(label);
            Ok(serde_json::Value::Null)
        }
        _ => panic!("unexpected request"),
    });
    assert!(matches!(result.result,Ok(Outcome::Reset(e)) if e.secret_refs.is_empty()));
    assert_eq!(removals.len(), original.secret_refs.len());
}
#[test]
fn reset_rejects_changed_fingerprints_and_never_removes_new_keys() {
    let original = entry();
    for changed in [true, false] {
        let mut current = original.clone();
        if changed {
            current.secret_refs[0].fingerprint = "replacement".into();
        } else {
            let mut added = current.secret_refs[0].clone();
            added.id = "new-key".into();
            current.secret_refs.push(added);
        }
        let result = request(
            &original,
            Operation::Reset {
                secrets: original.secret_refs.clone(),
                archived: false,
            },
        )
        .execute_with(|call| match call {
            AgentRequest::EntriesList { .. } => {
                Ok(serde_json::to_value(vec![current.clone()]).unwrap())
            }
            AgentRequest::SecretRemove { label, .. } => {
                assert_ne!(label, "new-key");
                assert!(!changed);
                current.secret_refs.retain(|s| s.id != label);
                Ok(serde_json::Value::Null)
            }
            _ => panic!("unexpected request"),
        });
        assert_eq!(
            result.result.err().unwrap().code,
            Some(AgentErrorCode::Conflict)
        );
        assert_eq!(result.mutated, !changed);
        assert!(!current.secret_refs.is_empty());
    }
}
#[test]
fn probe_forwards_only_agent_owned_identity_and_usage_options() {
    let e = entry();
    let mut count = 0;
    let completion = request(
        &e,
        Operation::Usage {
            secret_id: Some("key-id".into()),
            mode: UsageProbeMode::NewApiAdvanced,
            base_url: Some("https://fixture.invalid".into()),
            access_token: Some("synthetic".into()),
            user_id: Some("42".into()),
        },
    )
    .execute_with(|call| {
        count += 1;
        let AgentRequest::ProviderUsageProbe {
            id,
            secret_id,
            mode,
            timeout_seconds,
            access_token,
            user_id,
            ..
        } = call
        else {
            panic!()
        };
        assert_eq!(id, e.id);
        assert_eq!(secret_id.as_deref(), Some("key-id"));
        assert_eq!(mode, UsageProbeMode::NewApiAdvanced);
        assert_eq!(timeout_seconds, 15);
        assert_eq!(access_token.unwrap().expose(), "synthetic");
        assert_eq!(user_id.as_deref(), Some("42"));
        Ok(serde_json::json!({"ok":false,"source":"unknown","status":401,"error":"fixture denied"}))
    });
    assert_eq!(count, 1);
    assert!(!completion.mutated);
    assert!(matches!(completion.result,Ok(Outcome::Usage(r)) if !r.ok && r.status==Some(401)));
}
#[test]
fn subapi_apply_clears_missing_snapshots_and_preserves_explicit_zero() {
    let e = entry();
    let result = usage(
        serde_json::json!({"ok":true,"source":"sub_api_v1_usage","gateway":{"group":"unlimited"}}),
    );
    let AgentRequest::ProviderUsageApply {
        quota,
        gateway,
        subscription,
        source,
        ..
    } = apply_request(&e, &result)
    else {
        panic!()
    };
    assert!(quota.is_some_and(|q| q.remaining.is_none() && q.used.is_none()));
    assert_eq!(gateway.unwrap().group.as_deref(), Some("unlimited"));
    assert!(subscription.is_none());
    assert_eq!(source, Some(UsageProbeSource::SubApiV1Usage));
    assert!(can_apply(&usage(
        serde_json::json!({"ok":true,"source":"new_api_token_usage","quota":{"remaining":"0"}})
    )));
    assert!(!can_apply(&usage(
        serde_json::json!({"ok":false,"source":"unknown","quota":{"remaining":"100"}})
    )));
    assert!(!can_apply(&usage(
        serde_json::json!({"ok":true,"source":"unknown"})
    )));
}
#[test]
fn apply_failure_remains_retryable_and_invalid_result_makes_no_call() {
    let e = entry();
    let result = usage(
        serde_json::json!({"ok":true,"source":"new_api_token_usage","quota":{"remaining":"75"}}),
    );
    let completion = request(
        &e,
        Operation::Apply {
            entry: Box::new(e.clone()),
            result: Box::new(result),
        },
    )
    .execute_with(|_| Err(failure()));
    assert!(!completion.mutated && completion.result.is_err());
    let result = usage(serde_json::json!({"ok":false,"source":"unknown"}));
    let completion = request(
        &e,
        Operation::Apply {
            entry: Box::new(e.clone()),
            result: Box::new(result),
        },
    )
    .execute_with(|_| panic!("must not apply"));
    assert!(completion.result.is_err());
}

#[test]
fn apply_rejects_a_key_changed_after_the_probe_and_gateway_merge_keeps_missing_fields() {
    let mut e = entry();
    e.gateway = Some(GatewayMetadata {
        group: Some("old".into()),
        rate: Some("2".into()),
    });
    let result = usage(
        serde_json::json!({"ok":true,"source":"new_api_token_usage","gateway":{"group":"new"}}),
    );
    let AgentRequest::ProviderUsageApply { gateway, .. } = apply_request(&e, &result) else {
        panic!()
    };
    assert_eq!(
        gateway.unwrap(),
        GatewayMetadata {
            group: Some("new".into()),
            rate: Some("2".into())
        }
    );
    let mut current = e.clone();
    current.secret_refs[0].fingerprint = "changed".into();
    let completion = request(
        &e,
        Operation::Apply {
            entry: Box::new(e.clone()),
            result: Box::new(result),
        },
    )
    .execute_with(|call| {
        assert!(matches!(call, AgentRequest::EntriesList { .. }));
        Ok(serde_json::to_value(vec![current.clone()]).unwrap())
    });
    assert_eq!(
        completion.result.err().unwrap().code,
        Some(AgentErrorCode::Conflict)
    );
    assert!(!completion.mutated);
}
