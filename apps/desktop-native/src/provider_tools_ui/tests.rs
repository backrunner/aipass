use super::*;
use gpui::{TestAppContext, VisualTestContext};
fn tools(cx: &mut TestAppContext) -> (Entity<Tools>, &mut VisualTestContext) {
    cx.update(gpui_base::init);
    let mut tools = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Tools::new(false, false, window, cx));
        view.update(cx, |view, cx| {
            view.sync(
                Some(
                    aipass_desktop_native::catalog::Catalog::fixture()
                        .active
                        .remove(0),
                ),
                window,
                cx,
            )
        });
        tools = Some(view.clone());
        gpui_base::Root::new(view, window, cx)
    });
    (tools.unwrap(), cx)
}
fn result() -> UsageProbeResult {
    serde_json::from_value(
        serde_json::json!({"ok":true,"source":"new_api_token_usage","quota":{"remaining":"75"}}),
    )
    .unwrap()
}
#[gpui::test]
fn reset_requires_explicit_confirmation_cancel_is_inert_and_late_completion_is_ignored(
    cx: &mut TestAppContext,
) {
    let (view, cx) = tools(cx);
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.confirm_reset(window, cx);
            assert!(!v.has_request());
            v.show_reset(window, cx);
            assert!(!v.has_request());
            v.close(window, cx);
            v.confirm_reset(window, cx);
            assert!(!v.has_request());
            v.show_reset(window, cx);
            v.confirm_reset(window, cx);
            let request = v.take_request().unwrap();
            let Operation::Reset { secrets, .. } = request.operation else {
                panic!()
            };
            assert_eq!(secrets, v.entry.as_ref().unwrap().secret_refs);
            v.confirm_reset(window, cx);
            assert!(!v.has_request());
            v.close(window, cx);
            assert!(v.panel == Some(Panel::Reset));
            let original = v.entry.clone().unwrap();
            v.clear(window, cx);
            v.complete(
                Completion {
                    scope: request.scope,
                    mutated: true,
                    result: Ok(Outcome::Reset(Box::new(original))),
                },
                window,
                cx,
            );
            assert!(v.take_reset().is_none());
            assert!(v.panel.is_none());
            assert!(!v.busy());
        })
    });
}
#[gpui::test]
fn usage_is_key_scoped_and_clears_transient_access_token_before_reply(cx: &mut TestAppContext) {
    let (view, cx) = tools(cx);
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.show_usage(window, cx);
            v.mode = UsageProbeMode::NewApiAdvanced;
            v.inputs[1].update(cx, |s, cx| s.set_value("synthetic-token", window, cx));
            v.inputs[2].update(cx, |s, cx| s.set_value("42", window, cx));
            let id = v.entry.as_ref().unwrap().secret_refs[1].id.clone();
            v.secret_id = Some(id.clone());
            v.test_usage(window, cx);
            let request = v.take_request().unwrap();
            let Operation::Usage {
                secret_id,
                access_token,
                user_id,
                ..
            } = request.operation
            else {
                panic!()
            };
            assert_eq!(secret_id, Some(id));
            assert_eq!(access_token.unwrap().expose(), "synthetic-token");
            assert_eq!(user_id.as_deref(), Some("42"));
            assert!(v.inputs[1].read(cx).value().is_empty());
            v.test_usage(window, cx);
            assert!(!v.has_request());
            v.complete(
                Completion {
                    scope: request.scope,
                    mutated: false,
                    result: Ok(Outcome::Usage(Box::new(result()))),
                },
                window,
                cx,
            );
            assert!(v.usage.is_some());
            assert!(!v.busy());
            v.apply(window, cx);
            let request = v.take_request().unwrap();
            assert!(matches!(request.operation, Operation::Apply { .. }));
            v.complete(
                Completion {
                    scope: request.scope,
                    mutated: false,
                    result: Err(aipass_agent::AgentCommandError {
                        code: None,
                        message: "fixture failure".into(),
                    }),
                },
                window,
                cx,
            );
            assert!(v.usage.is_some());
            assert!(v.panel == Some(Panel::Usage));
            assert_eq!(v.error, "fixture failure");
            v.apply(window, cx);
            let request = v.take_request().unwrap();
            v.complete(
                Completion {
                    scope: request.scope,
                    mutated: true,
                    result: Ok(Outcome::Applied),
                },
                window,
                cx,
            );
            assert!(v.panel.is_none());
            assert!(v.notice.is_some());
        })
    });
}
#[gpui::test]
fn changing_mode_or_provider_invalidates_preview_and_modal_keyboard_stays_scoped(
    cx: &mut TestAppContext,
) {
    let (view, cx) = tools(cx);
    cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(640.)));
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.show_usage(window, cx);
            v.usage = Some(result());
            v.mode_focus[0].focus(window, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_keystrokes("right");
    assert_eq!(view.read_with(cx, |v, _| v.mode), UsageProbeMode::NewApi);
    assert!(view.read_with(cx, |v, _| v.usage.is_none()));
    cx.simulate_keystrokes("end");
    assert_eq!(
        view.read_with(cx, |v, _| v.mode),
        UsageProbeMode::NewApiAdvanced
    );
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.test_usage(window, cx);
            let request = v.take_request().unwrap();
            let other = aipass_desktop_native::catalog::Catalog::fixture()
                .active
                .remove(1);
            v.sync(Some(other), window, cx);
            v.complete(
                Completion {
                    scope: request.scope,
                    mutated: false,
                    result: Ok(Outcome::Usage(Box::new(result()))),
                },
                window,
                cx,
            );
            assert!(v.panel.is_none());
            assert!(v.usage.is_none());
        })
    });
}
#[gpui::test]
fn partial_reset_failure_keeps_error_and_confirmation_snapshot_across_refresh(
    cx: &mut TestAppContext,
) {
    let (view, cx) = tools(cx);
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.show_reset(window, cx);
            let expected = v.reset_snapshot.clone();
            v.confirm_reset(window, cx);
            let request = v.take_request().unwrap();
            v.complete(
                Completion {
                    scope: request.scope,
                    mutated: true,
                    result: Err(aipass_agent::AgentCommandError {
                        code: None,
                        message: "fixture interrupted".into(),
                    }),
                },
                window,
                cx,
            );
            let mut partial = v.entry.clone().unwrap();
            partial.secret_refs.remove(0);
            v.sync(Some(partial), window, cx);
            assert!(v.panel == Some(Panel::Reset));
            assert_eq!(v.error, "fixture interrupted");
            assert_eq!(v.reset_snapshot, expected);
            v.confirm_reset(window, cx);
            let request = v.take_request().unwrap();
            assert!(matches!(request.operation,Operation::Reset{secrets,..}if secrets==expected));
        })
    });
}

#[gpui::test]
fn key_picker_selects_by_identity_and_escape_keeps_the_usage_dialog(cx: &mut TestAppContext) {
    let (view, cx) = tools(cx);
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.show_usage(window, cx);
            v.key_focus.trigger.focus(window, cx);
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    press(cx, "down");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(view.read_with(cx, |v, _| v.key_open));
    cx.update(|window, cx| {
        window.simulate_next_frame(cx);
        window.draw(cx).clear(cx);
        assert!(view.read(cx).key_focus.items[0].is_focused(window));
    });
    press(cx, "end");
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    press(cx, "enter");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(view.read_with(cx, |v, _| v.secret_id.as_ref()
        == v.entry.as_ref().unwrap().secret_refs.last().map(|s| &s.id)));
    assert!(!view.read_with(cx, |v, _| v.key_open));
    press(cx, "down");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| {
        window.simulate_next_frame(cx);
        window.draw(cx).clear(cx);
    });
    press(cx, "escape");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(view.read_with(cx, |v, _| v.panel == Some(Panel::Usage) && !v.key_open));
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.test_usage(window, cx);
            let request = v.take_request().unwrap();
            v.close(window, cx);
            assert!(v.panel.is_none());
            v.complete(
                Completion {
                    scope: request.scope,
                    mutated: false,
                    result: Ok(Outcome::Usage(Box::new(result()))),
                },
                window,
                cx,
            );
            assert!(v.usage.is_none());
            assert!(!v.busy());
        })
    });
}

fn press(cx: &mut VisualTestContext, key: &str) {
    let keystroke = gpui::Keystroke::parse(key).unwrap();
    cx.simulate_event(gpui::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.simulate_event(gpui::KeyUpEvent { keystroke });
    cx.update(|window, cx| window.draw(cx).clear(cx));
}
