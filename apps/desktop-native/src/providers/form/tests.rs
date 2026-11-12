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
