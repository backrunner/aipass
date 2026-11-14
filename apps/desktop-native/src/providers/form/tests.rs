use super::*;
use gpui::{TestAppContext, VisualTestContext};
use std::{cell::Cell, rc::Rc};

fn form(cx: &mut TestAppContext, edit: bool) -> (Entity<Form>, &mut VisualTestContext) {
    cx.update(gpui_base::init);
    let mut form = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            Form::new(
                edit.then(|| {
                    aipass_desktop_native::catalog::Catalog::fixture()
                        .active
                        .remove(0)
                }),
                false,
                false,
                window,
                cx,
            )
        });
        form = Some(view.clone());
        gpui_base::Root::new(view, window, cx)
    });
    (form.unwrap(), cx)
}

#[gpui::test]
fn editing_title_starts_focused_at_unicode_end(cx: &mut TestAppContext) {
    cx.update(gpui_base::init);
    let title = "网关 🎉";
    let mut form = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let mut entry = aipass_desktop_native::catalog::Catalog::fixture()
            .active
            .remove(0);
        entry.title = title.into();
        let view = cx.new(|cx| Form::new(Some(entry), false, false, window, cx));
        form = Some(view.clone());
        gpui_base::Root::new(view, window, cx)
    });
    let form = form.unwrap();
    cx.update(|window, cx| {
        window.activate_window();
        let input = form.read(cx).inputs[0].read(cx);
        assert!(input.focus_handle(cx).is_focused(window));
        assert_eq!(input.selected_range(), title.len()..title.len());
    });
    cx.simulate_keystrokes("x");
    form.read_with(cx, |form, cx| {
        assert_eq!(form.inputs[0].read(cx).value(), format!("{title}x"))
    });
}

#[gpui::test]
fn concurrency_steps_with_keyboard_mouse_bounds_and_undo(cx: &mut TestAppContext) {
    let (form, cx) = form(cx, true);
    cx.simulate_resize(gpui::size(px(960.), px(640.)));
    cx.update(|window, cx| {
        form.update(cx, |form, cx| {
            form.preview_edit_fields(false);
            form.capture_scroll = Some(700.);
            form.inputs[9].read(cx).focus_handle(cx).focus(window, cx);
        });
        window.draw(cx).clear(cx);
    });
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(250));
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        form.read(cx).prepare_capture_scroll();
        window.draw(cx).clear(cx);
    });
    let value = |cx: &mut VisualTestContext| {
        form.read_with(cx, |form, cx| form.inputs[9].read(cx).value().to_string())
    };
    cx.simulate_keystrokes("up");
    assert_eq!(value(cx), "5");
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(value(cx), "4");
    for (initial, key, expected) in [
        ("0", "down", "0"),
        ("4294967295", "up", "4294967295"),
        ("1.5", "up", "2"),
        ("1.5", "down", "1"),
        ("", "up", "1"),
        ("", "down", "0"),
    ] {
        cx.update(|window, cx| {
            form.read(cx).inputs[9]
                .clone()
                .update(cx, |input, cx| input.set_value(initial, window, cx));
            window.draw(cx).clear(cx);
        });
        cx.simulate_keystrokes(key);
        assert_eq!(value(cx), expected);
    }
    let arrow = cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        let b = cx.global::<crate::parity::Geometry>().0["provider-concurrency-increment"];
        assert!(
            b[1] > 100. && b[1] + b[3] < 640.,
            "stepper must be visible: {b:?}"
        );
        gpui::point(px(b[0] + b[2] / 2.), px(b[1] + b[3] / 2.))
    });
    cx.simulate_mouse_move(arrow, gpui::MouseButton::Left, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_mouse_down(arrow, gpui::MouseButton::Left, Default::default());
    cx.simulate_mouse_up(arrow, gpui::MouseButton::Left, Default::default());
    assert_eq!(value(cx), "1");
    cx.update(|window, _| window.activate_window());
    cx.simulate_mouse_down(arrow, gpui::MouseButton::Left, Default::default());
    assert_eq!(value(cx), "2");
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(500));
    cx.run_until_parked();
    assert_eq!(value(cx), "3");
    cx.simulate_mouse_up(
        gpui::point(px(0.), px(0.)),
        gpui::MouseButton::Left,
        Default::default(),
    );
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(500));
    cx.run_until_parked();
    assert_eq!(value(cx), "3");
    cx.update(|window, cx| {
        assert!(form.read(cx).inputs[9]
            .read(cx)
            .focus_handle(cx)
            .is_focused(window));
        form.update(cx, |form, cx| form.set_busy(true, cx));
        window.draw(cx).clear(cx);
    });
    cx.simulate_keystrokes("up");
    cx.simulate_mouse_down(arrow, gpui::MouseButton::Left, Default::default());
    cx.simulate_mouse_up(arrow, gpui::MouseButton::Left, Default::default());
    assert_eq!(value(cx), "3");
}

#[gpui::test]
fn editing_notes_resize_preserves_text_and_clears_on_cancel(cx: &mut TestAppContext) {
    let (form, cx) = form(cx, true);
    cx.simulate_resize(gpui::size(px(960.), px(640.)));
    cx.update(|window, cx| {
        form.update(cx, |form, _| form.capture_scroll = Some(500.));
        window.draw(cx).clear(cx);
        form.read(cx).prepare_capture_scroll();
        window.draw(cx).clear(cx);
    });
    let before = form.read_with(cx, |form, cx| form.notes.read(cx).value().to_string());
    for dy in [80., -200.] {
        let start = cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            let b = cx.global::<crate::parity::Geometry>().0["provider-edit-notes-resize"];
            gpui::point(px(b[0] + b[2] / 2.), px(b[1] + b[3] / 2.))
        });
        cx.simulate_mouse_down(start, gpui::MouseButton::Left, Default::default());
        for offset in [dy / 2., dy] {
            cx.simulate_mouse_move(
                start + gpui::point(px(0.), px(offset)),
                gpui::MouseButton::Left,
                Default::default(),
            );
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }
        cx.simulate_mouse_up(
            start + gpui::point(px(0.), px(dy)),
            gpui::MouseButton::Left,
            Default::default(),
        );
        form.read_with(cx, |form, cx| {
            assert_eq!(form.notes.read(cx).value().as_ref(), before);
            let expected = if dy > 0. { px(146.6) } else { px(64.) };
            assert!((form.notes_height - expected).abs() < px(0.001));
        });
    }
    cx.update(|window, cx| form.update(cx, |form, cx| form.clear(window, cx)));
    form.read_with(cx, |form, cx| {
        assert_eq!(form.notes_height, px(66.6));
        assert!(form.notes.read(cx).value().is_empty());
    });
}

#[gpui::test]
fn edit_aliases_refresh_then_preserve_typed_draft_and_clear_headers_on_cancel(
    cx: &mut TestAppContext,
) {
    let (form, cx) = form(cx, true);
    cx.update(|window, cx| {
        form.update(cx, |view, cx| {
            assert_eq!(view.inputs[10].read(cx).value(), "fast=fixture-model");
            assert!(view.inputs[11].read(cx).value().is_empty());
            let mut fresh = view.draft.original.clone().unwrap();
            fresh.model_aliases = vec![("background".into(), "current".into())];
            view.refresh_original(&fresh, window, cx);
            assert_eq!(view.inputs[10].read(cx).value(), "background=current");
            assert!(!view.draft.aliases_touched);
            view.inputs[10].update(cx, |input, cx| {
                input.set_value("", window, cx);
                input.focus_handle(cx).focus(window, cx);
            });
        })
    });
    cx.update(|window, cx| {
        form.read(cx).inputs[10]
            .clone()
            .update(cx, |input, cx| input.insert("fast=new", window, cx))
    });
    cx.update(|window, cx| {
        form.update(cx, |view, cx| {
            assert!(view.draft.aliases_touched);
            let mut fresh = view.draft.original.clone().unwrap();
            fresh.model_aliases.clear();
            view.refresh_original(&fresh, window, cx);
            assert_eq!(view.inputs[10].read(cx).value(), "fast=new");
            view.inputs[11].update(cx, |input, cx| {
                input.insert("x-version=fixture", window, cx)
            });
            view.save(cx);
            let request = view
                .take_action()
                .unwrap()
                .into_request(|_| Ok(vec![("x-keep".into(), "original".into())]))
                .unwrap();
            let aipass_agent_protocol::AgentRequest::ProviderUpdate { input, .. } = request else {
                panic!("update expected")
            };
            assert_eq!(input.model_aliases, [("fast".into(), "new".into())]);
            assert_eq!(input.headers.unwrap().len(), 2);
            view.set_busy(false, cx);
            view.clear(window, cx);
            assert!(view.inputs[11].read(cx).value().is_empty());
            view.inputs[11].read(cx).focus_handle(cx).focus(window, cx);
        })
    });
    cx.simulate_keystrokes("cmd-z");
    form.read_with(cx, |view, cx| {
        assert!(view.inputs[11].read(cx).value().is_empty())
    });
}

#[gpui::test]
fn optional_fields_add_remove_focus_and_agent_payload(cx: &mut TestAppContext) {
    let (form, cx) = form(cx, false);
    cx.update(|window, cx| {
        form.update(cx, |view, cx| {
            for index in super::super::optional_fields::OPTIONAL {
                view.choose("add-field", index, window, cx);
            }
            assert!(!view.has_optional_fields());
            assert!(view.advanced);
            assert!(view.inputs[13].read(cx).focus_handle(cx).is_focused(window));
            for (index, value) in [
                (2, "fixture-key"),
                (10, "fast=gpt-4o-mini"),
                (11, "x-version=1"),
                (12, "fixture-group"),
                (13, "1x"),
                (14, "USD"),
                (15, "$0.002 / 1k"),
            ] {
                view.inputs[index].update(cx, |input, cx| input.set_value(value, window, cx));
            }
            view.save(cx);
            let Some(Action::Add(input)) = view.take_action() else {
                panic!("expected Add DTO");
            };
            assert_eq!(input.model_aliases, [("fast".into(), "gpt-4o-mini".into())]);
            assert_eq!(input.headers, [("x-version".into(), "1".into())]);
            assert_eq!(
                input.secret_metadata.group.as_deref(),
                Some("fixture-group")
            );
            let billing = input.secret_metadata.billing.unwrap();
            assert_eq!(billing.rate.as_deref(), Some("1x"));
            assert_eq!(billing.currency.as_deref(), Some("USD"));
            assert_eq!(billing.unit_price.as_deref(), Some("$0.002 / 1k"));
            view.set_busy(false, cx);
            view.remove_optional(13, window, cx);
            assert!(view.has_optional_fields());
            assert!(view.field_menu.trigger.is_focused(window));
            assert!((13..=15).all(|i| view.inputs[i].read(cx).value().is_empty()));
            view.clear(window, cx);
            assert!(view.inputs[11].read(cx).value().is_empty());
        })
    });
}

#[gpui::test]
fn add_field_keyboard_selects_visible_items_and_escape_only_closes_the_menu(
    cx: &mut TestAppContext,
) {
    let (form, cx) = form(cx, false);
    cx.update(|window, _| window.activate_window());
    let closed = Rc::new(Cell::new(0));
    let observed = closed.clone();
    let _subscription = cx.update(|_, cx| {
        cx.subscribe(&form, move |_, event, _| {
            if matches!(event, FormEvent::Close) {
                observed.set(observed.get() + 1);
            }
        })
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        window.draw(cx).clear(cx);
        form.read(cx).prepare_capture_scroll();
        window.draw(cx).clear(cx);
        form.read(cx).field_menu.trigger.clone().focus(window, cx);
    });
    cx.simulate_keystrokes("enter");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(cx.update(|_, cx| form.read(cx).picker), Some("add-field"));
    cx.simulate_keystrokes("down enter");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| {
        let view = form.read(cx);
        assert!(view.visible.contains(&4));
        assert_eq!(view.picker, None);
        assert!(view.inputs[4].read(cx).focus_handle(cx).is_focused(window));
        view.field_menu.trigger.clone().focus(window, cx);
    });
    cx.simulate_keystrokes("enter");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_keystrokes("escape");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(closed.get(), 0);
    assert!(cx.update(|window, cx| form.read(cx).field_menu.trigger.is_focused(window)));
    assert_eq!(cx.update(|_, cx| form.read(cx).picker), None);

    // An advanced input is initially absent from the focus tree. Opening its
    // disclosure must retain the requested focus through the zero-height frame.
    cx.simulate_keystrokes("enter");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_keystrokes("end enter");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(240));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.update(|window, cx| form.read(cx).inputs[13]
        .read(cx)
        .focus_handle(cx)
        .is_focused(window)));
    cx.simulate_keystrokes("x");
    assert_eq!(
        cx.update(|_, cx| form.read(cx).inputs[13].read(cx).value()),
        "x"
    );
}

#[gpui::test]
fn disclosure_reverses_without_jumping_and_reduced_motion_finishes(cx: &mut TestAppContext) {
    use std::time::Duration;
    let (form, cx) = form(cx, false);
    let draw = |cx: &mut VisualTestContext| cx.update(|window, cx| window.draw(cx).clear(cx));
    draw(cx);
    cx.update(|_, cx| {
        form.update(cx, |view, cx| {
            view.advanced = true;
            cx.notify();
        })
    });
    draw(cx);
    cx.executor().advance_clock(Duration::from_millis(90));
    draw(cx);
    let before = cx.update(|_, cx| form.read(cx).disclosure_frame);
    assert!(before.height > 0. && before.height < 1.);
    assert!(before.fade > before.height);
    cx.update(|_, cx| {
        form.update(cx, |view, cx| {
            view.advanced = false;
            cx.notify();
        })
    });
    draw(cx);
    let reversal = cx.update(|_, cx| form.read(cx).disclosure_frame);
    assert!((before.height - reversal.height).abs() < 0.001);
    cx.executor().advance_clock(Duration::from_millis(220));
    draw(cx);
    assert_eq!(cx.update(|_, cx| form.read(cx).disclosure_frame.height), 0.);
    cx.update(|_, cx| {
        cx.set_reduce_motion(true);
        form.update(cx, |view, cx| {
            view.advanced = true;
            cx.notify();
        });
    });
    draw(cx);
    let reduced = cx.update(|_, cx| form.read(cx).disclosure_frame);
    assert_eq!((reduced.height, reduced.fade), (1., 1.));
}

#[gpui::test]
fn validation_failure_keeps_the_draft_and_success_is_submitted_only_once(cx: &mut TestAppContext) {
    let (form, cx) = form(cx, false);
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    let _subscription = cx.update(|_, cx| {
        cx.subscribe(&form, move |_, event, _| {
            if matches!(event, FormEvent::Save) {
                observed.set(observed.get() + 1);
            }
        })
    });
    cx.update(|window, cx| {
        form.update(cx, |view, cx| {
            view.save(cx);
            assert!(!view.busy);
            assert_eq!(view.error, view.locale.text("providerForm.apiKeyRequired"));
            view.inputs[0].update(cx, |input, cx| {
                input.set_value("Fixture provider", window, cx)
            });
            view.inputs[2].update(cx, |input, cx| input.set_value("fixture-key", window, cx));
            view.inputs[3].update(cx, |input, cx| input.set_value("not-a-url", window, cx));
            view.save(cx);
            assert_eq!(view.error, view.locale.text("providers.invalidEndpoint"));
            assert!(view.take_action().is_none());
            view.inputs[3].update(cx, |input, cx| {
                input.set_value("https://example.invalid/v1", window, cx)
            });
            view.save(cx);
            view.save(cx);
            assert!(view.busy);
            assert!(matches!(view.take_action(), Some(Action::Add(_))));
            assert!(view.take_action().is_none());
        })
    });
    assert_eq!(calls.get(), 1);
}

#[gpui::test]
fn failed_save_can_retry_without_discarding_fields_and_clear_erases_undo_history(
    cx: &mut TestAppContext,
) {
    let (form, cx) = form(cx, false);
    cx.update(|window, cx| {
        form.update(cx, |view, cx| {
            view.inputs[2].update(cx, |input, cx| input.set_value("fixture-key", window, cx));
            view.notes
                .update(cx, |input, cx| input.set_value("one\ntwo", window, cx));
            view.save(cx);
            view.take_action();
            view.set_busy(false, cx);
            view.error = "Synthetic save failed".into();
            view.save(cx);
            let Some(Action::Add(input)) = view.take_action() else {
                panic!("retry request expected")
            };
            assert_eq!(input.notes.as_deref(), Some("one\ntwo"));
            assert!(view.error.is_empty());
            view.clear(window, cx);
            view.set_busy(false, cx);
            view.inputs[2].read(cx).focus_handle(cx).focus(window, cx);
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_keystrokes("cmd-z");
    cx.update(|_, cx| {
        let view = form.read(cx);
        assert!(view
            .inputs
            .iter()
            .all(|input| input.read(cx).value().is_empty()));
        assert!(view.notes.read(cx).value().is_empty());
    });
}

#[gpui::test]
fn changing_protocol_updates_auth_and_busy_picker_cannot_mutate_pending_payload(
    cx: &mut TestAppContext,
) {
    let (form, cx) = form(cx, true);
    cx.update(|window, cx| {
        form.update(cx, |view, cx| {
            view.choose("interface", 2, window, cx);
            assert_eq!(view.draft.auth, AuthScheme::GoogleApiKey);
            view.save(cx);
            view.choose("interface", 0, window, cx);
            assert_eq!(view.draft.interface, InterfaceType::Gemini);
            let Some(Action::Update(_, input)) = view.take_action() else {
                panic!("update expected")
            };
            assert_eq!(input.interface_type, InterfaceType::Gemini);
            assert!(input.api_key.is_none());
            assert!(input.headers.is_none());
            assert_eq!(
                input.notes.as_deref(),
                Some("Synthetic provider.\nTwo lines of notes.")
            );
        })
    });
}

#[gpui::test]
fn refreshed_metadata_preserves_edits_and_follows_untouched_runtime_settings(
    cx: &mut TestAppContext,
) {
    let (form, cx) = form(cx, true);
    cx.update(|window, cx| {
        form.update(cx, |view, cx| {
            view.inputs[0].update(cx, |input, cx| {
                input.set_value("My pending edit", window, cx)
            });
            let mut entry = view.draft.original.clone().unwrap();
            entry.max_concurrent_requests = Some(7);
            entry.supports_websockets = Some(true);
            entry.model_aliases.clear();
            view.refresh_original(&entry, window, cx);
            assert_eq!(view.inputs[0].read(cx).value().as_ref(), "My pending edit");
            assert_eq!(view.inputs[9].read(cx).value().as_ref(), "7");
            assert!(!view.draft.concurrency_touched);
            assert!(view.draft.websockets);
            view.draft.concurrency_touched = true;
            view.draft.websocket_touched = true;
            view.draft.websockets = false;
            view.inputs[9].update(cx, |input, cx| input.set_value("12", window, cx));
            entry.max_concurrent_requests = Some(9);
            view.refresh_original(&entry, window, cx);
            view.save(cx);
            let Some(Action::Update(_, input)) = view.take_action() else {
                panic!("update expected")
            };
            assert_eq!(input.title, "My pending edit");
            assert_eq!(input.max_concurrent_requests, Some(12));
            assert_eq!(input.supports_websockets, Some(false));
            assert!(input.model_aliases.is_empty());
            assert!(input.account_identity.is_none());
            view.set_busy(false, cx);
            view.set_error("websocket_probe_unconfirmed: status=404; fixture".into());
            assert!(view.error.contains("status=404; fixture"));
            assert!(!view.error.contains("websocket_probe_unconfirmed"));
            assert_eq!(view.inputs[0].read(cx).value().as_ref(), "My pending edit");
        });
    });
}

#[gpui::test]
fn provider_picker_starts_at_selection_restores_focus_and_ignores_exit_clicks(
    cx: &mut TestAppContext,
) {
    let (form, cx) = form(cx, false);
    cx.simulate_resize(gpui::size(px(960.), px(640.)));
    let press = |cx: &mut VisualTestContext, key: &str| {
        let keystroke = gpui::Keystroke::parse(key).unwrap();
        cx.simulate_event(gpui::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(gpui::KeyUpEvent { keystroke });
        cx.update(|w, cx| w.draw(cx).clear(cx));
    };
    cx.update(|window, cx| {
        form.update(cx, |form, cx| {
            form.choose("provider", 2, window, cx);
            form.picker_focus["provider"].trigger.focus(window, cx);
        });
        window.draw(cx).clear(cx);
    });
    press(cx, "down");
    cx.update(|window, cx| {
        window.simulate_next_frame(cx);
        window.draw(cx).clear(cx);
    });
    cx.update(|window, cx| {
        assert_eq!(form.read(cx).picker, Some("provider"));
        assert!(form.read(cx).picker_focus["provider"].items[2].is_focused(window));
    });
    press(cx, "down");
    press(cx, "enter");
    cx.update(|window, cx| {
        let form = form.read(cx);
        assert_eq!(
            form.draft.provider_id,
            aipass_provider_registry::default_provider_definitions()[3].id
        );
        assert_eq!(form.picker, None);
        assert!(form.picker_focus["provider"].trigger.is_focused(window));
    });
    press(cx, "enter");
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(161));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    press(cx, "end");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|_, cx| {
        let geometry = &cx.global::<crate::parity::Geometry>().0;
        let last = aipass_provider_registry::default_provider_definitions().len() - 1;
        let row = geometry[&format!("menu-form-provider-option-{last}")];
        let menu = geometry["menu-form-provider-content"];
        assert!(
            row[1] >= menu[1] + 4. && row[1] + row[3] <= menu[1] + menu[3] - 4.,
            "last option must scroll into view: {row:?} in {menu:?}"
        );
    });
    press(cx, "home");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let box_ =
        cx.update(|_, cx| cx.global::<crate::parity::Geometry>().0["menu-form-provider-option-0"]);
    press(cx, "escape");
    cx.simulate_click(
        gpui::point(px(box_[0] + 8.), px(box_[1] + 8.)),
        Default::default(),
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(
        form.read_with(cx, |v, _| v.draft.provider_id.clone()),
        aipass_provider_registry::default_provider_definitions()[3].id
    );
}
