use super::*;
use aipass_agent::AgentCommandError;
use aipass_desktop_native::catalog::Catalog;
use gpui::{TestAppContext, VisualTestContext};

fn press(cx: &mut VisualTestContext, key: &str) {
    let keystroke =
        cx.update(|_, cx| gpui::Keystroke::parse_with_os(key, cx.platform_info().os).unwrap());
    cx.simulate_event(gpui::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.simulate_event(gpui::KeyUpEvent { keystroke });
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn fixture(cx: &mut TestAppContext) -> (Entity<Credentials>, &mut VisualTestContext) {
    cx.update(gpui_base::init);
    let mut view = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let entity = cx.new(|cx| {
            let mut view = Credentials::new(false, false, window, cx);
            view.sync(
                Some(Catalog::fixture().active.remove(0)),
                false,
                false,
                window,
                cx,
            );
            view
        });
        view = Some(entity.clone());
        gpui_base::Root::new(entity, window, cx)
    });
    (view.unwrap(), cx)
}

fn fill(
    view: &Credentials,
    index: usize,
    value: &str,
    window: &mut Window,
    cx: &mut Context<Credentials>,
) {
    view.inputs[index].update(cx, |input, cx| {
        input.set_value(value.to_owned(), window, cx)
    });
}

#[gpui::test]
fn add_validation_duplicate_submit_failure_retry_and_undo_erasure(cx: &mut TestAppContext) {
    let (view, cx) = fixture(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.begin(None, window, cx);
            assert!(!view.save(cx));
            fill(view, 0, " Backup ", window, cx);
            fill(view, 1, "synthetic-credential", window, cx);
            fill(view, 3, "https://user:pass@fixture.invalid", window, cx);
            assert!(!view.save(cx));
            fill(view, 3, "https://fixture.invalid/v1", window, cx);
            assert!(view.save(cx));
            let request = view.take_request().unwrap();
            assert!(!view.save(cx));
            assert!(view.take_request().is_none());
            let Operation::Add {
                label,
                value,
                metadata,
            } = request.operation
            else {
                panic!("add expected")
            };
            assert_eq!(label, "Backup");
            assert_eq!(value.expose(), "synthetic-credential");
            assert_eq!(
                metadata.endpoint.as_deref(),
                Some("https://fixture.invalid/v1")
            );
            view.complete(
                Completion {
                    scope: request.scope,
                    result: Err(AgentCommandError {
                        code: None,
                        message: "synthetic failure".into(),
                    }),
                },
                window,
                cx,
            );
            assert_eq!(
                view.inputs[1].read(cx).value().as_ref(),
                "synthetic-credential"
            );
            assert!(view.has_editor());
            assert!(view.save(cx));
            let request = view.take_request().unwrap();
            view.complete(
                Completion {
                    scope: request.scope,
                    result: Ok(Outcome::Applied),
                },
                window,
                cx,
            );
            assert!(!view.has_editor());
            assert!(view
                .inputs
                .iter()
                .all(|input| input.read(cx).value().is_empty()));
            view.begin(None, window, cx);
            view.inputs[1].read(cx).focus_handle(cx).focus(window, cx);
        })
    });
    cx.simulate_keystrokes("cmd-z");
    assert!(view.read_with(cx, |view, cx| view.inputs[1].read(cx).value().is_empty()));
}

#[gpui::test]
fn late_reads_cannot_repopulate_cancelled_switched_or_replaced_credentials(
    cx: &mut TestAppContext,
) {
    let (view, cx) = fixture(cx);
    for invalidation in ["cancel", "provider", "fingerprint", "lock"] {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                let entry = Catalog::fixture().active.remove(0);
                let secret = entry.secret_refs[0].clone();
                view.sync(Some(entry.clone()), false, false, window, cx);
                view.begin(Some(secret), window, cx);
                let request = view.take_request().unwrap();
                match invalidation {
                    "cancel" => view.cancel(window, cx),
                    "provider" => view.sync(
                        Some(Catalog::fixture().active.remove(1)),
                        false,
                        false,
                        window,
                        cx,
                    ),
                    "fingerprint" => {
                        let mut changed = entry;
                        changed.secret_refs[0].fingerprint = "replaced-key".into();
                        view.sync(Some(changed), false, false, window, cx);
                    }
                    _ => view.clear(window, cx),
                }
                view.complete(
                    Completion {
                        scope: request.scope,
                        result: Ok(Outcome::Revealed("late-synthetic-value".into())),
                    },
                    window,
                    cx,
                );
                assert!(view.inputs[1].read(cx).value().is_empty(), "{invalidation}");
                assert!(view.revealed.is_empty(), "{invalidation}");
                assert!(!view.has_editor(), "{invalidation}");
            })
        });
    }
}

#[gpui::test]
fn edit_keeps_stable_id_and_explicitly_clears_overrides_without_losing_billing_note(
    cx: &mut TestAppContext,
) {
    let (view, cx) = fixture(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let mut secret = view.entry.as_ref().unwrap().secret_refs[0].clone();
            secret.billing = Some(BillingRule {
                note: Some("retained note".into()),
                rate: Some("1.5".into()),
                ..Default::default()
            });
            view.begin(Some(secret.clone()), window, cx);
            assert!(!view.save(cx), "cannot save before the read completes");
            let request = view.take_request().unwrap();
            assert!(matches!(request.operation, Operation::Reveal(ref id) if *id == secret.id));
            view.complete(
                Completion {
                    scope: request.scope,
                    result: Ok(Outcome::Revealed("synthetic-original".into())),
                },
                window,
                cx,
            );
            fill(view, 0, "renamed", window, cx);
            for index in 2..8 {
                fill(view, index, "", window, cx);
            }
            view.interface = InterfaceType::Gemini;
            assert!(view.save(cx));
            let Operation::Update {
                secret_id,
                label,
                value,
                metadata,
            } = view.take_request().unwrap().operation
            else {
                panic!("update expected")
            };
            assert_eq!(secret_id, secret.id);
            assert_eq!(label, "renamed");
            assert_eq!(value.expose(), "synthetic-original");
            assert_eq!(metadata.interface_type, Some(InterfaceType::Gemini));
            assert_eq!(metadata.group.as_deref(), Some(""));
            assert_eq!(metadata.endpoint.as_deref(), Some(""));
            assert_eq!(metadata.default_model.as_deref(), Some(""));
            assert_eq!(
                metadata.billing.unwrap().note.as_deref(),
                Some("retained note")
            );
        })
    });
}

#[gpui::test]
fn reveal_is_toggled_and_expires_without_clearing_newer_editor_input(cx: &mut TestAppContext) {
    let (view, cx) = fixture(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let id = view.entry.as_ref().unwrap().secret_refs[0].id.clone();
            for hide in [true, false] {
                view.reveal(id.clone(), cx);
                let request = view.take_request().unwrap();
                view.complete(
                    Completion {
                        scope: request.scope,
                        result: Ok(Outcome::Revealed("synthetic-visible".into())),
                    },
                    window,
                    cx,
                );
                assert_eq!(view.revealed[&id].value.expose(), "synthetic-visible");
                if hide {
                    view.reveal(id.clone(), cx);
                    assert!(view.revealed.is_empty());
                }
            }
            view.begin(None, window, cx);
            fill(view, 1, "new-draft", window, cx);
        })
    });
    cx.executor().advance_clock(Duration::from_secs(31));
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, cx| view.revealed.is_empty()
        && view.inputs[1].read(cx).value().as_ref() == "new-draft"));
}

#[gpui::test]
fn remove_is_only_available_in_provider_edit_and_targets_exact_id(cx: &mut TestAppContext) {
    let (view, cx) = fixture(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let id = view.entry.as_ref().unwrap().secret_refs[0].id.clone();
            view.remove(id.clone(), cx);
            assert!(view.take_request().is_none());
            view.editing_provider = true;
            view.remove(id.clone(), cx);
            let request = view.take_request().unwrap();
            assert!(matches!(request.operation, Operation::Remove(ref value) if *value == id));
            view.clear(window, cx);
            view.complete(
                Completion {
                    scope: request.scope,
                    result: Ok(Outcome::Applied),
                },
                window,
                cx,
            );
            assert!(view.notice.is_empty());
        })
    });
}

#[gpui::test]
fn subscription_credentials_never_enter_the_generic_api_key_editor(cx: &mut TestAppContext) {
    let (view, cx) = fixture(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let mut entry = Catalog::fixture().active.remove(0);
            let secret = entry.secret_refs[0].clone();
            entry.credential_kind = aipass_provider_registry::CredentialKind::OAuth;
            view.sync(Some(entry), true, false, window, cx);
            view.begin(Some(secret.clone()), window, cx);
            view.reveal(secret.id.clone(), cx);
            view.remove(secret.id, cx);
            assert!(!view.has_editor());
            assert!(view.take_request().is_none());
        })
    });
}

#[gpui::test]
fn long_revealed_keys_have_real_horizontal_scroll_extent(cx: &mut TestAppContext) {
    let (view, cx) = fixture(cx);
    cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(640.)));
    let id = cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let id = view.entry.as_ref().unwrap().secret_refs[0].id.clone();
            view.reveal(id.clone(), cx);
            let request = view.take_request().unwrap();
            view.complete(
                Completion {
                    scope: request.scope,
                    result: Ok(Outcome::Revealed(
                        "synthetic-long-value-".repeat(100).into(),
                    )),
                },
                window,
                cx,
            );
            id
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        view.read_with(cx, |view, _| view.revealed[&id].scroll.max_offset().x
            > gpui::px(100.))
    );
}

#[gpui::test]
fn protocol_picker_keyboard_commits_the_exact_option_and_dismisses(cx: &mut TestAppContext) {
    let (view, cx) = fixture(cx);
    cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(640.)));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.begin(None, window, cx);
            view.picker_focus.trigger.focus(window, cx);
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    press(cx, "enter");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(view.read_with(cx, |view, _| view.picker_open));
    press(cx, "end");
    cx.update(|window, cx| assert!(view.read(cx).picker_focus.items[5].is_focused(window)));
    press(cx, "enter");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(view.read_with(cx, |view, _| !view.picker_open
        && view.interface == InterfaceType::CustomHttp));
    press(cx, "enter");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    press(cx, "home");
    press(cx, "down");
    press(cx, "enter");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(view.read_with(cx, |view, _| !view.picker_open
        && view.interface == InterfaceType::AnthropicMessages));
    press(cx, "enter");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    press(cx, "escape");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(!view.read_with(cx, |view, _| view.picker_open));
}

#[gpui::test]
fn provider_save_waits_for_credential_commit_and_stops_on_credential_failure(
    cx: &mut TestAppContext,
) {
    use crate::providers::Form;
    let (view, cx) = fixture(cx);
    let form = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut form = Form::new(
                Some(Catalog::fixture().active.remove(0)),
                false,
                false,
                window,
                cx,
            );
            form.attach_credentials(view.clone(), cx);
            form
        })
    });
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.begin(None, window, cx);
            fill(view, 0, "new key", window, cx);
            fill(view, 1, "synthetic-composite-save", window, cx);
        })
    });
    for success in [false, true] {
        cx.update(|_, cx| {
            form.update(cx, |form, cx| {
                form.set_busy(false, cx);
                form.save(cx);
                assert!(
                    form.take_action().is_none(),
                    "provider cannot save before key acknowledgement"
                );
                form.set_busy(true, cx);
            })
        });
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                let request = view.take_request().unwrap();
                view.complete(
                    Completion {
                        scope: request.scope,
                        result: if success {
                            Ok(Outcome::Applied)
                        } else {
                            Err(AgentCommandError {
                                code: None,
                                message: "synthetic failure".into(),
                            })
                        },
                    },
                    window,
                    cx,
                );
            })
        });
        let action = cx.update(|_, cx| form.update(cx, |form, _| form.take_action()));
        assert_eq!(action.is_some(), success);
        assert_eq!(view.read_with(cx, |view, _| view.has_editor()), !success);
    }
}
