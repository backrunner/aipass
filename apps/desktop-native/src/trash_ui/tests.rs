use super::*;
use aipass_agent::AgentCommandError;
use gpui::TestAppContext;

#[gpui::test]
fn confirmation_freezes_target_blocks_duplicate_submit_and_recovers_after_failure(
    cx: &mut TestAppContext,
) {
    cx.update(gpui_base::init);
    let (view, cx) = cx.add_window_view(|_, cx| Confirm::new(false, false, cx));
    let id = Uuid::new_v4();
    let origin = cx.update(|window, cx| {
        let handle = cx.focus_handle();
        handle.focus(window, cx);
        handle
    });
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.show(Some((id, "First provider".into())), window, cx);
            view.show(Some((Uuid::new_v4(), "Other provider".into())), window, cx);
            assert!(view.title().contains("First provider"));
            view.confirm(cx);
            let request = view.take_request().unwrap();
            assert_eq!(request.target, Some(id));
            view.confirm(cx);
            assert!(view.take_request().is_none());
            view.close(window, cx);
            assert!(view.open);
            view.complete(
                Completion {
                    generation: request.generation,
                    result: Err(AgentCommandError {
                        code: None,
                        message: "synthetic failure".into(),
                    }),
                },
                window,
                cx,
            );
            assert!(view.open);
            assert!(!view.busy);
            assert_eq!(view.error.as_deref(), Some("synthetic failure"));
            view.confirm(cx);
            let retry = view.take_request().unwrap();
            assert_eq!(retry.target, Some(id));
            view.complete(
                Completion {
                    generation: request.generation,
                    result: Ok(()),
                },
                window,
                cx,
            );
            assert!(view.open && view.busy);
            view.complete(
                Completion {
                    generation: retry.generation,
                    result: Ok(()),
                },
                window,
                cx,
            );
            assert!(!view.open);
            assert!(origin.is_focused(window));
        })
    });
}

#[gpui::test]
fn lock_clears_pending_confirmation_and_late_results_cannot_close_a_new_one(
    cx: &mut TestAppContext,
) {
    cx.update(gpui_base::init);
    let (view, cx) = cx.add_window_view(|_, cx| Confirm::new(true, true, cx));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.show(None, window, cx);
            assert_eq!(view.title(), "永久删除回收站中的所有内容？");
            view.confirm(cx);
            let request = view.take_request().unwrap();
            assert_eq!(request.target, None);
            view.clear(cx);
            assert!(!view.open);
            assert!(view.take_request().is_none());
            view.show(Some((Uuid::new_v4(), "Synthetic".into())), window, cx);
            view.complete(
                Completion {
                    generation: request.generation,
                    result: Ok(()),
                },
                window,
                cx,
            );
            assert!(view.open);
            view.close(window, cx);
            assert!(!view.open);
            assert!(view.take_request().is_none());
        })
    });
}

#[gpui::test]
fn rendered_dialog_traps_escape_while_busy_and_dismisses_after_failure(cx: &mut TestAppContext) {
    cx.update(gpui_base::init);
    let mut view = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let entity = cx.new(|cx| Confirm::new(false, false, cx));
        view = Some(entity.clone());
        gpui_base::Root::new(entity, window, cx)
    });
    let view = view.unwrap();
    cx.update(|window, cx| {
        view.update(cx, |view, cx| view.show(None, window, cx));
        window.draw(cx).clear(cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        view.update(cx, |view, cx| view.confirm(cx));
        window.draw(cx).clear(cx);
    });
    cx.simulate_keystrokes("escape");
    assert!(view.read_with(cx, |view, _| view.open && view.busy));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let request = view.take_request().unwrap();
            view.complete(
                Completion {
                    generation: request.generation,
                    result: Err(AgentCommandError {
                        code: None,
                        message: "synthetic error".into(),
                    }),
                },
                window,
                cx,
            );
        });
        window.draw(cx).clear(cx);
    });
    cx.simulate_keystrokes("escape");
    assert!(!view.read_with(cx, |view, _| view.open));
}
