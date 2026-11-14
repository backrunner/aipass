use super::*;
use crate::subscription_ui::{fixtures, tests::view};
use gpui::TestAppContext;

#[gpui::test]
fn closing_settings_discards_edits_without_collapsing_or_rewriting_the_overview(
    cx: &mut TestAppContext,
) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            s.fixture("subscription-overview", window, cx);
            let saved = serde_json::to_value(&s.runtime_options.value).unwrap();
            s.show_options(1, window, cx);
            s.runtime_options.value.as_mut().unwrap().quota_tracking = false;
            s.runtime_options.dirty = true;
            let secret = s.runtime_options.fields["proxy-password"].clone();
            secret.update(cx, |f, cx| f.set_value("synthetic-only", window, cx));
            s.close(window, cx);
            assert!(secret.read(cx).value().is_empty());
            s.discard_options_draft(window, cx);
            assert!(s.runtime_options.expanded);
            assert_eq!(
                serde_json::to_value(&s.runtime_options.value).unwrap(),
                saved
            );
            assert!(s.runtime_options.fields.is_empty());
            s.show_options(0, window, cx);
            assert!(s.runtime_options.value.as_ref().unwrap().quota_tracking);
            s.clear(window, cx);
            assert!(s.runtime_options.value.is_none());
            assert!(!s.runtime_options.expanded);
        });
    });
}

#[gpui::test]
fn settings_save_keeps_exact_account_and_omitted_secrets(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            s.fixture("subscription-runtime-proxy", window, cx);
            let id = s.entry.as_ref().unwrap().id;
            s.runtime_options.fields["proxy-url"]
                .update(cx, |f, cx| f.set_value("http://127.0.0.1:9999", window, cx));
            s.runtime_options.dirty = true;
            s.options_save(window, cx);
            let request = s.take_request().unwrap();
            let Operation::Options(actual_id, Action::Save(draft)) = request.operation else {
                panic!("typed runtime save required")
            };
            assert_eq!(id, actual_id);
            let proxy = draft.proxy.unwrap();
            assert_eq!(proxy.url.as_deref(), Some("http://127.0.0.1:9999"));
            assert!(
                proxy.password.is_none() && proxy.username.is_none() && !proxy.clear_credentials
            );
            assert!(draft.balance.unwrap().headers[0].1.is_none());
            assert!(draft.webhooks[0].secret.is_none() && !draft.webhooks[0].clear_secret);
            s.close(window, cx);
            assert!(
                s.open,
                "a pending save cannot be cancelled as if it had not committed"
            );
            s.options_complete(
                id,
                Completion {
                    generation: request.generation,
                    kind: Kind::Options(id),
                    result: Err(aipass_agent::AgentCommandError {
                        code: None,
                        message: "fixture failure".into(),
                    }),
                },
                window,
                cx,
            );
            assert!(!s.runtime_options.pending);
            assert!(s.runtime_options.dirty);
            assert_eq!(s.options_value("proxy-url", cx), "http://127.0.0.1:9999");
            s.close(window, cx);
            assert!(!s.open);
        })
    });
}
#[gpui::test]
fn settings_secrets_retire_on_account_change_and_late_response_cannot_cross_accounts(
    cx: &mut TestAppContext,
) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            s.fixture("subscription-runtime-proxy", window, cx);
            let a = s.entry.clone().unwrap();
            s.runtime_options.fields["proxy-password"]
                .update(cx, |f, cx| f.set_value("synthetic-only", window, cx));
            s.runtime_options.dirty = true;
            s.options_save(window, cx);
            assert_eq!(s.options_value("proxy-password", cx), "synthetic-only");
            let request = s.take_request().unwrap();
            let Operation::Options(_, Action::Save(draft)) = request.operation else {
                panic!()
            };
            assert_eq!(
                draft
                    .proxy
                    .as_ref()
                    .unwrap()
                    .password
                    .as_ref()
                    .unwrap()
                    .expose(),
                "synthetic-only"
            );
            let mut b = a.clone();
            b.id = Uuid::new_v4();
            s.sync_entry(Some(b), window, cx);
            s.sync_entry(Some(a.clone()), window, cx);
            s.options_complete(
                a.id,
                Completion {
                    generation: request.generation,
                    kind: Kind::Options(a.id),
                    result: Ok(Outcome::Options(ResultData::Saved(Box::default()))),
                },
                window,
                cx,
            );
            assert!(s.runtime_options.value.is_none());
            assert!(s.runtime_options.fields.is_empty());
        })
    });
}
#[gpui::test]
fn deleting_a_header_preserves_other_draft_rows_and_hook_limit(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            s.fixture("subscription-runtime-balance", window, cx);
            s.options_add_header(window, cx);
            s.runtime_options.fields["header-1-name"]
                .update(cx, |f, cx| f.set_value("X-Account", window, cx));
            s.runtime_options.fields["header-1-value"]
                .update(cx, |f, cx| f.set_value("synthetic-value", window, cx));
            s.options_remove_header(0, window, cx);
            let value = s.options_draft(cx).unwrap();
            assert_eq!(value.balance.as_ref().unwrap().headers[0].0, "X-Account");
            assert_eq!(
                value.balance.unwrap().headers[0]
                    .1
                    .as_ref()
                    .unwrap()
                    .expose(),
                "synthetic-value"
            );
            for _ in 0..12 {
                s.options_add_hook(window, cx);
            }
            assert_eq!(s.runtime_options.value.as_ref().unwrap().webhooks.len(), 8);
            s.clear(window, cx);
            assert!(s.runtime_options.fields.is_empty());
            assert!(s.take_request().is_none());
        })
    });
}
#[gpui::test]
fn opening_connect_does_not_drop_an_independent_quota_refresh(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            s.sync_entry(Some(fixtures::entry("subscription-usage")), window, cx);
            s.refresh_usage(cx);
            s.show(None, None, window, cx);
            assert!(matches!(
                s.take_request().unwrap().operation,
                Operation::Refresh(_)
            ));
        })
    });
}

#[gpui::test]
fn balance_body_preserves_multiline_input_and_is_erased_on_close(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            s.fixture("subscription-runtime-balance", window, cx);
            let body = s.runtime_options.body.clone().unwrap();
            body.update(cx, |f, cx| {
                f.set_value("{\n  \"account\": \"synthetic\"\n}", window, cx)
            });
            let value = s.options_draft(cx).unwrap();
            assert_eq!(
                value.balance.unwrap().body.unwrap().expose(),
                "{\n  \"account\": \"synthetic\"\n}"
            );
            s.close(window, cx);
            assert!(body.read(cx).value().is_empty());
        })
    });
}

#[gpui::test]
fn lock_retires_a_pending_settings_save(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            s.fixture("subscription-runtime-proxy", window, cx);
            s.runtime_options.dirty = true;
            s.options_save(window, cx);
            let request = s.take_request().unwrap();
            let id = s.entry.as_ref().unwrap().id;
            s.clear(window, cx);
            assert!(!s.open && !s.runtime_options.pending);
            s.options_complete(
                id,
                Completion {
                    generation: request.generation,
                    kind: Kind::Options(id),
                    result: Ok(Outcome::Options(ResultData::Saved(Box::default()))),
                },
                window,
                cx,
            );
            assert!(s.entry.is_none() && s.runtime_options.value.is_none());
        })
    });
}
#[gpui::test]
fn disabling_options_erases_only_the_removed_draft(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            s.fixture("subscription-runtime-balance", window, cx);
            s.runtime_options.fields["proxy-url"]
                .update(cx, |f, cx| f.set_value("http://127.0.0.1:9999", window, cx));
            let body = s.runtime_options.body.clone().unwrap();
            body.update(cx, |f, cx| f.set_value("synthetic-body", window, cx));
            s.options_toggle("balance", false, window, cx);
            assert!(body.read(cx).value().is_empty());
            s.options_toggle("balance", true, window, cx);
            let balance = s.options_draft(cx).unwrap().balance.unwrap();
            assert!(balance.url.is_empty() && balance.body.is_none() && balance.headers.is_empty());
            assert_eq!(s.options_value("proxy-url", cx), "http://127.0.0.1:9999");
            s.options_choice("runtime-proxy", "inherit", window, cx);
            s.options_choice("runtime-proxy", "custom", window, cx);
            assert!(s.options_value("proxy-url", cx).is_empty());
        })
    });
}

#[gpui::test]
fn failed_settings_save_keeps_secret_for_retry_success_erases_it(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            s.fixture("subscription-runtime-proxy", window, cx);
            let input = s.runtime_options.fields["proxy-password"].clone();
            input.update(cx, |f, cx| {
                f.set_value("synthetic-retry-secret", window, cx)
            });
            let id = s.entry.as_ref().unwrap().id;
            s.runtime_options.dirty = true;
            s.options_save(window, cx);
            let request = s.take_request().unwrap();
            s.options_complete(
                id,
                Completion {
                    generation: request.generation,
                    kind: Kind::Options(id),
                    result: Err(aipass_agent::AgentCommandError {
                        code: None,
                        message: "fixture failure".into(),
                    }),
                },
                window,
                cx,
            );
            assert_eq!(input.read(cx).value().as_str(), "synthetic-retry-secret");
            s.options_save(window, cx);
            let retry = s.take_request().unwrap();
            let Operation::Options(_, Action::Save(value)) = &retry.operation else {
                panic!()
            };
            assert_eq!(
                value
                    .proxy
                    .as_ref()
                    .unwrap()
                    .password
                    .as_ref()
                    .unwrap()
                    .expose(),
                "synthetic-retry-secret"
            );
            s.options_complete(
                id,
                Completion {
                    generation: retry.generation,
                    kind: Kind::Options(id),
                    result: Ok(Outcome::Options(ResultData::Saved(Box::default()))),
                },
                window,
                cx,
            );
            assert!(input.read(cx).value().is_empty());
        })
    });
}
#[gpui::test]
fn settings_arrow_keys_move_focus_and_panel_together(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| {
        window.activate_window();
        view.update(cx, |s, cx| {
            s.fixture("subscription-runtime-quota", window, cx);
            s.runtime_options.tab_focus[0].focus(window, cx);
        });
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    cx.update(|window, cx| {
        let s = view.read(cx);
        assert_eq!(s.runtime_options.tab, 1);
        assert!(s.runtime_options.tab_focus[1].is_focused(window));
    });
}

#[gpui::test]
fn settings_loading_can_close_and_late_read_stays_retired(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            let entry = fixtures::entry("subscription-usage");
            let id = entry.id;
            s.sync_entry(Some(entry), window, cx);
            s.show_options(0, window, cx);
            let request = s.take_request().unwrap();
            assert!(s.runtime_options.loading);
            s.close(window, cx);
            assert!(!s.open);
            s.options_complete(
                id,
                Completion {
                    generation: request.generation,
                    kind: Kind::Options(id),
                    result: Ok(Outcome::Options(ResultData::Loaded(
                        Box::default(),
                        vec![],
                        None,
                    ))),
                },
                window,
                cx,
            );
            assert!(s.runtime_options.value.is_none());
        })
    });
}

#[gpui::test]
fn quota_picker_starts_at_selected_value_for_pointer_and_keyboard(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    for keyboard in [false, true] {
        cx.update(|window, cx| {
            view.update(cx, |s, cx| {
                s.fixture("subscription-runtime-quota", window, cx)
            })
        });
        cx.update(|w, cx| w.draw(cx).clear(cx));
        if keyboard {
            cx.update(|w, cx| {
                let focus = view.read(cx).picker_triggers["runtime-interval"].clone();
                focus.focus(w, cx);
            });
            cx.simulate_keystrokes("down");
        } else {
            let b = cx.update(|_, cx| {
                cx.global::<crate::parity::Geometry>().0["subscription-trigger-runtime-interval"]
            });
            cx.simulate_click(
                gpui::point(gpui::px(b[0] + b[2] / 2.), gpui::px(b[1] + b[3] / 2.)),
                Default::default(),
            );
        }
        cx.update(|w, cx| {
            w.draw(cx).clear(cx);
            w.simulate_next_frame(cx);
        });
        cx.update(|w, cx| w.draw(cx).clear(cx));
        assert!(cx.update(|w, cx| view.read(cx).picker_focus.items[1].is_focused(w)));
        cx.simulate_keystrokes("down");
        cx.update(|w, cx| w.draw(cx).clear(cx));
        assert!(
            cx.update(|w, cx| view.read(cx).picker_focus.items[2].is_focused(w)),
            "Down must advance from the selected option"
        );
        let keystroke = gpui::Keystroke::parse("enter").unwrap();
        cx.simulate_event(gpui::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(gpui::KeyUpEvent { keystroke });
        cx.update(|w, cx| w.draw(cx).clear(cx));
        view.read_with(cx, |s, _| {
            assert_eq!(
                s.runtime_options
                    .value
                    .as_ref()
                    .unwrap()
                    .quota_refresh_seconds,
                300
            );
            assert!(s.picker.is_none());
        });
        assert!(cx.update(|w, cx| view.read(cx).picker_triggers["runtime-interval"].is_focused(w)));
    }
}

#[gpui::test]
fn connection_inputs_follow_tab_order_inside_the_dialog(cx: &mut TestAppContext) {
    use gpui::Focusable;
    let (view, cx) = view(cx);
    cx.update(|w, cx| {
        cx.set_reduce_motion(true);
        view.update(cx, |s, cx| s.fixture("subscription-runtime-proxy", w, cx));
    });
    cx.update(|w, cx| w.draw(cx).clear(cx));
    cx.update(|w, cx| {
        let focus = view.read(cx).runtime_options.fields["proxy-url"]
            .read(cx)
            .focus_handle(cx);
        focus.focus(w, cx);
    });
    cx.update(|w, cx| w.draw(cx).clear(cx));
    for key in ["proxy-user", "proxy-password"] {
        cx.simulate_keystrokes("tab");
        cx.update(|w, cx| w.draw(cx).clear(cx));
        assert!(
            cx.update(|w, cx| view.read(cx).runtime_options.fields[key]
                .read(cx)
                .focus_handle(cx)
                .is_focused(w)),
            "Tab must focus {key}"
        );
    }
}
