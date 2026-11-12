use super::*;
use gpui::TestAppContext;
use std::{cell::Cell, rc::Rc};

#[gpui::test]
fn background_status_refresh_preserves_in_progress_creation(cx: &mut TestAppContext) {
    cx.update(gpui_base::init);
    let (view, cx) =
        cx.add_window_view(|window, cx| AuthView::new(Mode::Create, false, false, window, cx));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.password
                .update(cx, |input, cx| input.set_value("fixture-only", window, cx));
            view.sync_exists(false, window, cx);
            assert!(view.mode == Mode::Create);
            assert_eq!(view.password.read(cx).value(), "fixture-only");
            // A vault arriving through sync does invalidate this creation form.
            view.sync_exists(true, window, cx);
            assert!(view.mode == Mode::Unlock);
            assert!(view.password.read(cx).value().is_empty());
        })
    });
}

#[gpui::test]
fn confirmation_and_pending_state_gate_authentication_events(cx: &mut TestAppContext) {
    cx.update(gpui_base::init);
    let (view, cx) =
        cx.add_window_view(|window, cx| AuthView::new(Mode::Create, false, false, window, cx));
    let events = Rc::new(Cell::new(0));
    let observed = events.clone();
    let _subscription = cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event, _| {
            assert!(matches!(event, AuthEvent::Create(_)));
            observed.set(observed.get() + 1);
        })
    });
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.password
                .update(cx, |input, cx| input.set_value("fixture-only", window, cx));
            view.confirmation
                .update(cx, |input, cx| input.set_value("mismatch", window, cx));
            view.submit(window, cx);
            assert!(!view.busy);
        })
    });
    assert_eq!(events.get(), 0);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.confirmation
                .update(cx, |input, cx| input.set_value("fixture-only", window, cx));
            view.submit(window, cx);
            assert!(view.busy);
            assert_eq!(view.password.read(cx).value(), "fixture-only");
            assert_eq!(view.confirmation.read(cx).value(), "fixture-only");
            view.submit(window, cx);
        })
    });
    assert_eq!(events.get(), 1);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.set_busy(false, cx);
            view.error = "fixture failed request".into();
            assert!(view.ready(cx));
            assert_eq!(view.password.read(cx).value(), "fixture-only");
            view.clear(window, cx);
            assert!(view.password.read(cx).value().is_empty());
            assert!(view.confirmation.read(cx).value().is_empty());
        })
    });
}

#[gpui::test]
fn changing_auth_mode_clears_inputs_and_reveal_state(cx: &mut TestAppContext) {
    cx.update(gpui_base::init);
    let (view, cx) =
        cx.add_window_view(|window, cx| AuthView::new(Mode::Unlock, false, false, window, cx));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            assert!(view.password.read(cx).focus_handle(cx).is_focused(window));
            view.password
                .update(cx, |input, cx| input.set_value("fixture-only", window, cx));
            view.revealed = true;
            view.change_mode(Mode::Recover, window, cx);
            assert!(!view.revealed);
            assert!(view.password.read(cx).value().is_empty());
            assert!(view.recovery.read(cx).value().is_empty());
            view.set_busy(true, cx);
            view.change_mode(Mode::Unlock, window, cx);
            assert!(view.mode == Mode::Recover);
        })
    });
}
