use super::*;
use aipass_agent_protocol::{
    ToolConfigApplyResponse, ToolConfigLoginStatus, ToolConfigMode, ToolConfigPreviewResponse,
};
use aipass_provider_registry::{CredentialKind, ProviderKind};
use gpui::TestAppContext;
use uuid::Uuid;

fn native(view: &mut Integrations, cx: &mut Context<Integrations>) {
    let mut entry = view.entry.clone().unwrap();
    entry.credential_kind = CredentialKind::OAuth;
    entry.provider_kind = ProviderKind::Official;
    entry.provider_id = Some("codex".into());
    entry.secret_refs.truncate(1);
    view.sync(Some(entry), false, cx);
}
fn preview(request: &ToolConfigRequest, preimage: &str) -> Outcome {
    Outcome::Preview(ToolConfigPreviewResponse {
        tool: request.tool.clone(),
        mode: request.mode.clone(),
        entry_id: request.id,
        preview_id: Some(preimage.into()),
        entry_title: "fixture".into(),
        target_path: String::new(),
        summary: String::new(),
        preview: String::new(),
        files: vec![],
    })
}
fn applied(request: &ToolConfigRequest, outcome: ToolConfigOutcome) -> ToolConfigApplyResponse {
    ToolConfigApplyResponse {
        tool: request.tool.clone(),
        mode: request.mode.clone(),
        entry_id: request.id,
        outcome,
        message: None,
        entry_title: "fixture".into(),
        operation_id: Uuid::new_v4(),
        target_path: String::new(),
        backup_path: String::new(),
        summary: String::new(),
    }
}
fn status(ticket: Uuid) -> ToolConfigLoginStatus {
    ToolConfigLoginStatus {
        ticket,
        status: "pending".into(),
        url: None,
        user_code: None,
        requires_code: false,
        message: None,
        result: None,
    }
}

#[gpui::test]
fn native_switch_keeps_exact_secret_and_never_uses_api_auth_mode(cx: &mut TestAppContext) {
    let (view, cx) = tests::fixture(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            native(view, cx);
            assert_eq!(view.tools(), vec![ToolConfigTool::Codex]);
            assert!(view.can_configure(&ToolConfigTool::Codex));
            view.show_preview(ToolConfigTool::Codex, false, cx);
            let request = view.take_request().unwrap();
            let Operation::Preview(plan) = request.operation else {
                panic!()
            };
            assert_eq!(plan.mode, ToolConfigMode::Official);
            assert_eq!(plan.secret_id.as_deref(), Some("key-a"));
            assert!(plan.codex_api_key_mode.is_none());
            view.complete(
                Completion {
                    scope: request.scope,
                    result: Ok(preview(&plan, "native-preimage")),
                },
                window,
                cx,
            );
            view.confirm(cx);
            let Operation::Apply(plan) = view.take_request().unwrap().operation else {
                panic!()
            };
            assert_eq!(plan.preview_id.as_deref(), Some("native-preimage"));
            assert_eq!(plan.secret_id.as_deref(), Some("key-a"));
            assert_eq!(plan.mode, ToolConfigMode::Official);
        })
    });
}

#[gpui::test]
fn reconnect_uses_cli_bound_account_and_renews_preimage_before_login(cx: &mut TestAppContext) {
    let (view, cx) = tests::fixture(cx);
    cx.update(|window, cx| view.update(cx, |view, cx| {
        let bound = Uuid::new_v4();
        view.statuses = vec![ToolConfigStatus { tool: ToolConfigTool::Codex, state: "login_required".into(),
            entry_id: Some(bound), entry_title: None, secret_id: Some("bound-secret".into()),
            mode: Some(ToolConfigMode::Official), account_identity: None, operation_id: None, message: None, overrides: vec![] }];
        view.reconnect(&ToolConfigTool::Codex, cx);
        let request = view.take_request().unwrap();
        let Operation::Preview(plan) = request.operation else { panic!() };
        assert_eq!(plan.id, bound);
        assert_eq!(plan.secret_id.as_deref(), Some("bound-secret"));
        view.complete(Completion { scope: request.scope, result: Ok(preview(&plan, "old")) }, window, cx);
        let request = view.take_request().unwrap();
        assert!(matches!(&request.operation, Operation::Apply(p) if p.id == bound && p.preview_id.as_deref() == Some("old")));
        view.complete(Completion { scope: request.scope, result: Ok(Outcome::Applied(applied(&plan, ToolConfigOutcome::LoginRequired))) }, window, cx);
        let request = view.take_request().unwrap();
        let Operation::Preview(renewed) = request.operation else { panic!() };
        assert!(renewed.preview_id.is_none());
        assert_eq!(renewed.secret_id, plan.secret_id);
        view.complete(Completion { scope: request.scope, result: Ok(preview(&renewed, "renewed")) }, window, cx);
        assert!(view.login_modal().is_some());
        let Operation::LoginStart(login) = view.take_request().unwrap().operation else { panic!() };
        assert_eq!(login.id, bound);
        assert_eq!(login.secret_id.as_deref(), Some("bound-secret"));
        assert_eq!(login.preview_id.as_deref(), Some("renewed"));
    }));
}

#[gpui::test]
fn late_login_start_is_cancelled_without_reopening_or_replacing_selection(cx: &mut TestAppContext) {
    let (view, cx) = tests::fixture(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            native(view, cx);
            view.login_plan = view.request_for(ToolConfigTool::Codex);
            view.open_login(window, cx);
            let request = view.take_request().unwrap();
            let mut entry = view.entry.clone().unwrap();
            entry.id = Uuid::new_v4();
            view.sync(Some(entry.clone()), false, cx);
            assert!(view.login.is_none());
            let ticket = Uuid::new_v4();
            view.complete(
                Completion {
                    scope: request.scope,
                    result: Ok(Outcome::Login(status(ticket))),
                },
                window,
                cx,
            );
            assert_eq!(view.entry.as_ref().unwrap().id, entry.id);
            assert!(view.login.is_none());
            let request = view.take_request().unwrap();
            assert!(matches!(request.operation, Operation::LoginCancel(id) if id == ticket));
            view.complete(
                Completion {
                    scope: request.scope,
                    result: Ok(Outcome::Cancelled),
                },
                window,
                cx,
            );
            assert!(view.pending.is_none());
        })
    });
}

#[gpui::test]
fn failed_cleanup_does_not_consume_new_preview_and_lock_releases_inflight(cx: &mut TestAppContext) {
    let (view, cx) = tests::fixture(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            native(view, cx);
            view.cancel_ticket(Uuid::new_v4(), cx);
            let cleanup = view.take_request().unwrap();
            view.show_preview(ToolConfigTool::Codex, false, cx);
            assert!(!view.has_request());
            view.complete(
                Completion {
                    scope: cleanup.scope,
                    result: Err(aipass_agent::AgentCommandError {
                        code: None,
                        message: "offline".into(),
                    }),
                },
                window,
                cx,
            );
            assert!(matches!(view.pending, Some(Pending::Preview(_, false))));
            assert!(view.has_request());
            let _in_flight = view.take_request().unwrap();
            let entry = view.entry.clone();
            view.clear(cx); // Session may discard the retired request's reply on lock.
            view.sync(entry, false, cx);
            assert!(matches!(
                view.take_request().unwrap().operation,
                Operation::Scan
            ));
        })
    });
}

#[gpui::test]
fn complete_without_applied_is_not_success_and_mismatched_preview_cannot_start_login(
    cx: &mut TestAppContext,
) {
    let (view, cx) = tests::fixture(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            native(view, cx);
            let plan = view.request_for(ToolConfigTool::Codex).unwrap();
            view.send(
                Operation::Preview(plan.clone()),
                Pending::PrepareLogin(plan.clone(), true),
                cx,
            );
            let request = view.take_request().unwrap();
            let mut wrong = plan.clone();
            wrong.id = Uuid::new_v4();
            view.complete(
                Completion {
                    scope: request.scope,
                    result: Ok(preview(&wrong, "wrong")),
                },
                window,
                cx,
            );
            assert!(view.login.is_none() && !view.has_request());
            view.login_plan = Some(plan.clone());
            view.open_login(window, cx);
            let request = view.take_request().unwrap();
            let mut login = status(Uuid::new_v4());
            login.status = "complete".into();
            login.result = Some(applied(&plan, ToolConfigOutcome::Conflict));
            assert!(!login::succeeded(&login));
            view.complete(
                Completion {
                    scope: request.scope,
                    result: Ok(Outcome::Login(login)),
                },
                window,
                cx,
            );
            assert!(view.login.is_some() && view.message.is_empty());
            assert!(!view.has_request());
        })
    });
}
