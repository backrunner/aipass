use super::*;
use crate::integrations::preview::Event;
use aipass_agent_protocol::{ToolConfigMode, ToolConfigPreviewResponse, ToolConfigTool};

fn open(
    view: &Entity<Proxy>,
    readonly: bool,
    cx: &mut VisualTestContext,
) -> Entity<crate::integrations::preview::Preview> {
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            s.preview("proxy-ready", w, cx);
            s.close_focus.focus(w, cx);
            s.previous_focus = Some(s.close_focus.clone());
            let route_id = s.config.as_ref().unwrap().routes[0].id;
            s.plan = Some(aipass_agent_protocol::ToolConfigProxyRequest {
                tool: aipass_config_writers::ToolId::Codex,
                route_id,
            });
            s.preview_readonly = readonly;
            s.show_tool_preview(
                ToolConfigPreviewResponse {
                    preview_id: None,
                    tool: ToolConfigTool::Codex,
                    mode: ToolConfigMode::Plaintext,
                    entry_id: route_id,
                    entry_title: "Synthetic route".into(),
                    target_path: "/synthetic/config.toml".into(),
                    summary: "Synthetic plan".into(),
                    preview: "+ model = \"fixture\"".into(),
                    files: vec![],
                },
                w,
                cx,
            );
            s.preview.clone().unwrap()
        })
    })
}

#[gpui::test]
fn readonly_preview_cannot_write_and_cancel_restores_focus(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    let preview = open(&view, true, cx);
    preview.read_with(cx, |s, _| assert!(!s.can_confirm()));
    cx.update(|_, cx| preview.update(cx, |_, cx| cx.emit(Event::Confirm)));
    cx.run_until_parked();
    cx.update(|_, cx| view.update(cx, |s, _| assert!(s.take_request().is_none())));
    cx.update(|_, cx| preview.update(cx, |_, cx| cx.emit(Event::Close)));
    cx.run_until_parked();
    cx.update(|w, cx| {
        view.read_with(cx, |s, _| {
            assert!(s.preview.is_none() && s.plan.is_none());
            assert!(s.close_focus.is_focused(w));
        })
    });
}

#[gpui::test]
fn preview_apply_error_retries_once_and_busy_cancel_is_rejected(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    let preview = open(&view, false, cx);
    cx.update(|_, cx| preview.update(cx, |_, cx| cx.emit(Event::Confirm)));
    cx.run_until_parked();
    cx.update(|_, cx| {
        preview.update(cx, |_, cx| {
            cx.emit(Event::Confirm);
            cx.emit(Event::Close);
        })
    });
    cx.run_until_parked();
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            let request = s.take_request().unwrap();
            assert!(matches!(request.operation, Operation::ToolApply(_)));
            assert!(s.take_request().is_none());
            assert!(s.preview.is_some());
            s.complete(
                Completion {
                    scope: request.scope,
                    result: Err(aipass_agent::AgentCommandError {
                        code: None,
                        message: "Synthetic write failure".into(),
                    }),
                },
                w,
                cx,
            );
        })
    });
    preview.read_with(cx, |s, _| {
        assert!(s.can_confirm());
        assert_eq!(s.error, "Synthetic write failure");
    });
    cx.update(|_, cx| preview.update(cx, |_, cx| cx.emit(Event::Confirm)));
    cx.run_until_parked();
    preview.read_with(cx, |s, _| {
        assert!(s.busy);
        assert!(s.error.is_empty(), "retry retires the prior failure");
    });
    cx.update(|_, cx| {
        view.update(cx, |s, _| {
            assert!(matches!(
                s.take_request().unwrap().operation,
                Operation::ToolApply(_)
            ));
            assert!(s.take_request().is_none());
        })
    });
}
