use super::*;
use std::{cell::RefCell, rc::Rc};

fn fixture(cx: &mut gpui::TestAppContext) -> (Entity<Login>, &mut gpui::VisualTestContext) {
    cx.update(gpui_base::init);
    let mut login = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Login::new(ToolConfigTool::ClaudeCode, false, false, window, cx));
        login = Some(view.clone());
        gpui_base::Root::new(view, window, cx)
    });
    (login.unwrap(), cx)
}
fn pending() -> ToolConfigLoginStatus {
    ToolConfigLoginStatus {
        ticket: Uuid::new_v4(),
        status: "pending".into(),
        url: Some("https://claude.ai/oauth/authorize".into()),
        user_code: None,
        requires_code: true,
        message: None,
        result: None,
    }
}

#[gpui::test]
fn submitted_code_is_single_use_and_erased_from_undo(cx: &mut gpui::TestAppContext) {
    let (view, cx) = fixture(cx);
    let events = Rc::new(RefCell::new(vec![]));
    let recorded = events.clone();
    let _subscription = cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &Event, _| {
            if let Event::Code(code) = event {
                recorded.borrow_mut().push(code.expose().to_owned());
            }
        })
    });
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.receive(pending(), window, cx);
            view.code.update(cx, |input, cx| {
                input.set_value("synthetic-code", window, cx)
            });
            view.submit(window, cx);
            view.submit(window, cx);
            assert!(view.code.read(cx).value().is_empty());
            assert!(view.timer.is_none());
            view.set_busy(false, cx);
            view.code.read(cx).focus_handle(cx).focus(window, cx);
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(events.borrow().as_slice(), &["synthetic-code"]);
    assert!(view.read_with(cx, |view, cx| view.code.read(cx).value().is_empty()));
}

#[gpui::test]
fn terminal_login_states_stop_polling_and_close_erases_draft(cx: &mut gpui::TestAppContext) {
    let (view, cx) = fixture(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            for state in ["failed", "error", "expired", "cancelled", "complete"] {
                let mut status = pending();
                status.status = state.into();
                view.receive(status, window, cx);
                assert!(view.timer.is_none());
                assert!(!view.error.is_empty());
                assert!(!view.busy);
            }
            view.code.update(cx, |input, cx| {
                input.set_value("synthetic-draft", window, cx)
            });
            view.close(window, cx);
            assert!(view.code.read(cx).value().is_empty());
            assert!(view.timer.is_none());
        })
    });
}
