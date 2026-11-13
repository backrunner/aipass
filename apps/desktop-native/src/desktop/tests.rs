use super::*;
use crate::curtain::CurtainEvent;

struct RejectPassword;
impl aipass_desktop_native::session::Transport for RejectPassword {
    fn execute(
        &mut self,
        command: Command,
    ) -> Result<aipass_desktop_native::session::Reply, aipass_agent::AgentCommandError> {
        if matches!(command, Command::Unlock(_)) {
            return Err(aipass_agent::AgentCommandError {
                code: Some(aipass_agent_protocol::AgentErrorCode::InvalidPassword),
                message: "synthetic password rejection".into(),
            });
        }
        Ok(aipass_desktop_native::session::Reply::Snapshot {
            status: aipass_agent_protocol::SessionStatus {
                exists: true,
                locked: true,
                policy: Default::default(),
                last_lock_reason: None,
                vault_namespace: None,
                initial_sync_pending: false,
                initial_sync_failed: false,
                sync_revision: 0,
                sync_status: None,
            },
            entries: vec![],
        })
    }
}

#[gpui::test]
fn password_error_survives_a_background_refresh_due_on_completion(cx: &mut gpui::TestAppContext) {
    let (desktop, cx) = workspace(cx);
    cx.update(|_, cx| {
        desktop.update(cx, |view, _| {
            let mut session = Session::new(RejectPassword).unwrap();
            assert!(session.submit(Command::Unlock("synthetic-password".into())));
            view.session = Some(session);
            view.show_workspace = false;
            view.last_refresh = Instant::now() - Duration::from_secs(2);
        })
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let completed = cx.update(|window, cx| {
            desktop.update(cx, |view, cx| {
                view.update_session(window, cx);
                !view.session.as_ref().unwrap().foreground_pending
            })
        });
        if completed {
            break;
        }
        assert!(Instant::now() < deadline, "auth worker failed to complete");
        std::thread::yield_now();
    }
    cx.update(|_, cx| {
        let view = desktop.read(cx);
        assert_eq!(
            view.auth.read(cx).error,
            view.locale.text("error.incorrectMasterPassword")
        );
        assert!(!view.auth.read(cx).busy);
        assert!(!view.show_workspace);
    });
}

fn workspace(cx: &mut gpui::TestAppContext) -> (Entity<Desktop>, &mut gpui::VisualTestContext) {
    cx.update(|cx| {
        gpui_base::init(cx);
        init(cx);
    });
    let mut desktop = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Desktop::new(None, "empty", false, false, window, cx));
        desktop = Some(view.clone());
        gpui_base::Root::new(view, window, cx)
    });
    (desktop.unwrap(), cx)
}

#[gpui::test]
fn lock_keeps_workspace_until_cover_and_repeated_lock_does_not_restart(
    cx: &mut gpui::TestAppContext,
) {
    let (desktop, cx) = workspace(cx);
    let curtain = cx.update(|window, cx| {
        desktop.update(cx, |view, cx| {
            view.lock(window, cx);
            let curtain = view.curtain.clone().unwrap();
            curtain.update(cx, |curtain, _| curtain.freeze_at(0.));
            assert!(view.show_workspace);
            view.lock(window, cx);
            assert_eq!(
                view.curtain.as_ref().unwrap().entity_id(),
                curtain.entity_id()
            );
            curtain
        })
    });
    cx.update(|_, cx| curtain.update(cx, |_, cx| cx.emit(CurtainEvent::Covered)));
    assert!(!desktop.read_with(cx, |view, _| view.show_workspace));
    cx.update(|_, cx| curtain.update(cx, |_, cx| cx.emit(CurtainEvent::Done)));
    assert!(desktop.read_with(cx, |view, _| view.curtain.is_none()));
}

#[gpui::test]
fn superseded_unlock_events_cannot_reveal_or_finish_the_later_lock(cx: &mut gpui::TestAppContext) {
    let (desktop, cx) = workspace(cx);
    let unlock = cx.update(|window, cx| {
        desktop.update(cx, |view, cx| {
            view.show_workspace = false;
            view.start_transition(Direction::Unlock, window, cx);
            let curtain = view.curtain.clone().unwrap();
            curtain.update(cx, |curtain, _| curtain.freeze_at(0.));
            curtain
        })
    });
    cx.update(|_, cx| unlock.update(cx, |_, cx| cx.emit(CurtainEvent::Covered)));
    assert!(desktop.read_with(cx, |view, _| view.show_workspace));
    let lock = cx.update(|window, cx| {
        desktop.update(cx, |view, cx| {
            view.lock(window, cx);
            let curtain = view.curtain.clone().unwrap();
            curtain.update(cx, |curtain, _| curtain.freeze_at(0.));
            curtain
        })
    });
    cx.update(|_, cx| lock.update(cx, |_, cx| cx.emit(CurtainEvent::Covered)));
    cx.update(|_, cx| {
        unlock.update(cx, |_, cx| {
            cx.emit(CurtainEvent::Covered);
            cx.emit(CurtainEvent::Done);
        })
    });
    assert!(desktop.read_with(cx, |view, _| !view.show_workspace
        && view.curtain.as_ref().unwrap().entity_id() == lock.entity_id()));
}

#[gpui::test]
fn initial_workspace_focus_routes_window_actions(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        gpui_base::init(cx);
        init(cx);
        cx.set_reduce_motion(true);
    });
    let mut desktop = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Desktop::new(None, "empty", false, false, window, cx));
        desktop = Some(view.clone());
        gpui_base::Root::new(view, window, cx)
    });
    let desktop = desktop.unwrap();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(desktop.read_with(cx, |desktop, _| desktop.show_workspace));
    cx.simulate_keystrokes("cmd-l");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(!desktop.read_with(cx, |desktop, _| desktop.show_workspace));
    cx.update(|window, cx| {
        let view = desktop.read(cx);
        assert!(view.focus.contains_focused(window, cx));
        assert!(!view.focus.is_focused(window));
    });
}
