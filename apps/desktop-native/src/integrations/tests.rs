use super::*;
use aipass_agent_protocol::{ToolConfigMode, ToolConfigPreviewResponse};
use aipass_desktop_native::catalog::Catalog;
use gpui::{TestAppContext, VisualTestContext};

fn make_response(view: &Integrations) -> ToolConfigPreviewResponse {
    let Pending::Preview(request, _) = view.pending.as_ref().unwrap() else {
        panic!("preview request expected")
    };
    ToolConfigPreviewResponse {
        preview_id: Some("preimage-1".into()),
        tool: request.tool.clone(),
        mode: request.mode.clone(),
        entry_id: request.id,
        entry_title: "fixture".into(),
        target_path: "/synthetic/config.json".into(),
        summary: String::new(),
        preview: String::new(),
        files: serde_json::from_str(include_str!(
            "../../../../tests/desktop-parity/reference/business-preview-files.json"
        ))
        .unwrap(),
    }
}

pub(super) fn fixture(cx: &mut TestAppContext) -> (Entity<Integrations>, &mut VisualTestContext) {
    cx.update(gpui_base::init);
    let mut view = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let entity = cx.new(|cx| {
            let mut view = Integrations::new(false, false, cx);
            view.sync(Some(Catalog::fixture().active.remove(0)), false, cx);
            let request = view.take_request().unwrap();
            let detections = [
                aipass_config_writers::ToolId::Codex,
                aipass_config_writers::ToolId::OpenCode,
            ]
            .into_iter()
            .map(|tool| aipass_config_writers::ToolDetection {
                tool,
                binary_found: true,
                local_runtime_found: false,
                extension_found: false,
                config_path: None,
            })
            .collect();
            view.complete(
                Completion {
                    scope: request.scope,
                    result: Ok(Outcome::Scanned {
                        detections,
                        statuses: vec![],
                    }),
                },
                window,
                cx,
            );
            view.expanded = true;
            view
        });
        view = Some(entity.clone());
        gpui_base::Root::new(entity, window, cx)
    });
    (view.unwrap(), cx)
}

#[gpui::test]
fn multiple_keys_require_explicit_selection_and_confirmation_keeps_exact_request(
    cx: &mut TestAppContext,
) {
    let (view, cx) = fixture(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            assert!(view.tools().contains(&ToolConfigTool::Codex));
            assert!(!view.can_configure(&ToolConfigTool::Codex));
            view.show_preview(ToolConfigTool::Codex, false, cx);
            assert!(!view.has_request());
            view.select_secret("key-b".into(), cx);
            view.set_mode(CodexApiKeyMode::ExperimentalBearerToken, cx);
            view.show_preview(ToolConfigTool::Codex, false, cx);
            let response = make_response(view);
            let request = view.take_request().unwrap();
            view.show_preview(ToolConfigTool::Codex, false, cx);
            assert!(!view.has_request());
            view.complete(
                Completion {
                    scope: request.scope,
                    result: Ok(Outcome::Preview(response)),
                },
                window,
                cx,
            );
            view.confirm(cx);
            let Operation::Apply(plan) = view.take_request().unwrap().operation else {
                panic!("apply expected")
            };
            assert_eq!(plan.secret_id.as_deref(), Some("key-b"));
            assert_eq!(plan.preview_id.as_deref(), Some("preimage-1"));
            assert_eq!(
                plan.codex_api_key_mode,
                Some(CodexApiKeyMode::ExperimentalBearerToken)
            );
            assert_eq!(plan.mode, ToolConfigMode::Plaintext);
            view.confirm(cx);
            assert!(!view.has_request());
        })
    });
}

#[gpui::test]
fn readonly_and_missing_preimage_never_allow_apply(cx: &mut TestAppContext) {
    let (view, cx) = fixture(cx);
    for readonly in [false, true] {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.select_secret("key-a".into(), cx);
                view.show_preview(ToolConfigTool::Codex, readonly, cx);
                let mut response = make_response(view);
                if !readonly {
                    response.preview_id = None;
                }
                let request = view.take_request().unwrap();
                view.complete(
                    Completion {
                        scope: request.scope,
                        result: Ok(Outcome::Preview(response)),
                    },
                    window,
                    cx,
                );
                assert!(view.modal().is_some());
                assert!(view.plan.is_none());
                view.confirm(cx);
                assert!(!view.has_request());
            })
        });
    }
}

#[gpui::test]
fn late_preview_cannot_survive_changed_provider_metadata_or_lock(cx: &mut TestAppContext) {
    let (view, cx) = fixture(cx);
    for reason in ["provider", "fingerprint", "endpoint", "model", "lock"] {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                let original = Catalog::fixture().active.remove(0);
                view.sync(Some(original.clone()), false, cx);
                view.select_secret("key-a".into(), cx);
                view.show_preview(ToolConfigTool::Codex, false, cx);
                let response = make_response(view);
                let request = view.take_request().unwrap();
                let mut entry = original;
                match reason {
                    "provider" => entry = Catalog::fixture().active.remove(1),
                    "fingerprint" => entry.secret_refs[0].fingerprint = "new-value".into(),
                    "endpoint" => {
                        entry.endpoints[0].url = Some("https://changed.invalid/v1".into())
                    }
                    "model" => entry.default_model = Some("changed".into()),
                    _ => {}
                }
                if reason == "lock" {
                    view.clear(cx);
                } else {
                    view.sync(Some(entry), false, cx);
                }
                view.complete(
                    Completion {
                        scope: request.scope,
                        result: Ok(Outcome::Preview(response)),
                    },
                    window,
                    cx,
                );
                assert!(view.plan.is_none() && view.preview.is_none(), "{reason}");
            })
        });
    }
}

#[gpui::test]
fn apply_notice_expires_restore_uses_agent_status_and_failure_is_retryable(
    cx: &mut TestAppContext,
) {
    let (view, cx) = fixture(cx);
    cx.update(|window,cx|view.update(cx,|view,cx|{
        view.select_secret("key-a".into(),cx);view.show_preview(ToolConfigTool::OpenCode,false,cx);
        let response=make_response(view);let request=view.take_request().unwrap();
        view.complete(Completion{scope:request.scope,result:Ok(Outcome::Preview(response))},window,cx);
        view.confirm(cx);let request=view.take_request().unwrap();
        view.complete(Completion{scope:request.scope,result:Err(aipass_agent::AgentCommandError{code:Some(aipass_agent_protocol::AgentErrorCode::Conflict),message:"changed".into()})},window,cx);
        assert!(view.preview.is_none()&&view.plan.is_none());assert_eq!(view.error,"changed");
        view.show_preview(ToolConfigTool::OpenCode,false,cx);
        let response=make_response(view);let request=view.take_request().unwrap();
        view.complete(Completion{scope:request.scope,result:Ok(Outcome::Preview(response))},window,cx);
        view.confirm(cx);let request=view.take_request().unwrap();
        let operation_id=uuid::Uuid::new_v4();
        view.complete(Completion{scope:request.scope,result:Ok(Outcome::Applied(aipass_agent_protocol::ToolConfigApplyResponse {
            outcome:ToolConfigOutcome::Applied,message:None,tool:ToolConfigTool::OpenCode,mode:ToolConfigMode::Helper,entry_id:request.scope.entry,entry_title:"fixture".into(),operation_id,target_path:String::new(),backup_path:String::new(),summary:String::new()
        }))},window,cx);
        assert!(!view.message.is_empty());
        view.scan(cx);let request=view.take_request().unwrap();
        view.complete(Completion{scope:request.scope,result:Ok(Outcome::Scanned{detections:view.detections.clone(),statuses:vec![ToolConfigStatus {tool:ToolConfigTool::Codex,state:"ready".into(),entry_title:Some("fixture".into()),entry_id:Some(request.scope.entry),secret_id:Some("key-a".into()),mode:Some(ToolConfigMode::Plaintext),account_identity:None,operation_id:Some(operation_id),message:None,overrides:vec![]}]})},window,cx);
        view.restore(&ToolConfigTool::Codex,cx);
        assert!(matches!(view.take_request().unwrap().operation,Operation::Rollback(id) if id==operation_id));
    }));
    cx.executor().advance_clock(Duration::from_secs(5));
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.applied.is_none()
        && !view.message.is_empty()));
}

#[gpui::test]
fn key_picker_keyboard_uses_stable_id_and_dismisses(cx: &mut TestAppContext) {
    let (view, cx) = fixture(cx);
    cx.update(|window, cx| view.read(cx).picker_focus.trigger.clone().focus(window, cx));
    for key in ["enter", "end", "enter"] {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let keystroke = gpui::Keystroke::parse(key).unwrap();
        cx.simulate_event(gpui::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(gpui::KeyUpEvent { keystroke });
    }
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        view.read_with(cx, |view, _| view.secret_id.as_deref() == Some("key-b")
            && !view.picker_open)
    );
}
