use super::*;
use aipass_desktop_native::{
    catalog::Catalog,
    session::{Command, Reply, Session, Transport},
};
use std::time::{Duration, Instant};

struct ReadOnly;
impl Transport for ReadOnly {
    fn execute(&mut self, command: Command) -> Result<Reply, aipass_agent::AgentCommandError> {
        assert!(matches!(command, Command::Refresh | Command::Lock));
        Ok(Reply::Snapshot {
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
            entries: Catalog::fixture().active,
        })
    }
}

fn setup(cx: &mut gpui::TestAppContext) -> (gpui::Entity<Desktop>, &mut gpui::VisualTestContext) {
    cx.update(|cx| {
        gpui_base::init(cx);
        crate::desktop::init(cx);
        cx.set_global(Inbox::default());
    });
    let mut view = None;
    let (_, cx) = cx.add_window_view(|w, cx| {
        let desktop = cx.new(|cx| {
            Desktop::new(
                Some(Session::new(ReadOnly).unwrap()),
                "empty",
                false,
                false,
                w,
                cx,
            )
        });
        view = Some(desktop.clone());
        gpui_base::Root::new(desktop, w, cx)
    });
    cx.simulate_resize(gpui::size(px(960.), px(640.)));
    let view = view.unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        cx.update(|w, cx| view.update(cx, |s, cx| s.update_session(w, cx)));
        if view.read_with(cx, |s, _| !s.session.as_ref().unwrap().pending) {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    (view, cx)
}

fn receive(title: &str, cx: &gpui::App) {
    let mut url = url::Url::parse("ccswitch://v1/import").unwrap();
    url.query_pairs_mut().extend_pairs([
        ("resource", "provider"),
        ("app", "codex"),
        ("name", title),
        ("apiKey", "synthetic-key"),
    ]);
    cx.global::<Inbox>()
        .receive(provider_links::parse(url.into()).unwrap());
}

#[gpui::test]
fn links_wait_for_unlock_and_preserve_existing_edits_then_retire_on_lock(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = setup(cx);
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            receive("First", cx);
            s.pump_links(w, cx);
            assert!(!s.workspace.read(cx).has_provider_form());
            assert_eq!(cx.global::<Inbox>().0.borrow().0.len(), 1);
            s.session.as_mut().unwrap().status.as_mut().unwrap().locked = false;
            s.session.as_mut().unwrap().catalog = Catalog::fixture();
            s.show_workspace = true;
            s.focus.focus(w, cx);
        })
    });
    cx.update(|w, cx| {
        w.draw(cx).clear(cx);
        view.update(cx, |s, cx| {
            s.pump_links(w, cx);
            assert!(s.workspace.read(cx).has_provider_form());
            receive("Second", cx);
            s.pump_links(w, cx);
            assert_eq!(cx.global::<Inbox>().0.borrow().0.len(), 1);
            s.lock(w, cx);
            assert!(!s.workspace.read(cx).has_provider_form());
            assert!(cx.global::<Inbox>().0.borrow().0.is_empty());
            assert!(s.links.duplicate.is_none());
        });
    });
}

#[gpui::test]
fn duplicate_requires_confirmation_and_cancel_does_not_write(cx: &mut gpui::TestAppContext) {
    let (view, cx) = setup(cx);
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            s.session.as_mut().unwrap().status.as_mut().unwrap().locked = false;
            s.session.as_mut().unwrap().catalog = Catalog::fixture();
            s.show_workspace = true;
            s.focus.focus(w, cx);
        })
    });
    cx.update(|w, cx| {
        w.draw(cx).clear(cx);
        view.update(cx, |s, cx| {
            let title = s.session.as_ref().unwrap().catalog.active[0].title.clone();
            receive(&title, cx);
            s.pump_links(w, cx);
            assert!(s.links.duplicate.is_some());
            assert!(!s.workspace.read(cx).has_provider_form());
            assert!(s.links.cancel_focus.is_focused(w));
        });
        w.draw(cx).clear(cx);
    });
    cx.simulate_keystrokes("escape");
    view.read_with(cx, |s, cx| {
        assert!(s.links.duplicate.is_none());
        assert!(!s.workspace.read(cx).has_provider_form());
    });
    cx.update(|w, cx| {
        w.draw(cx).clear(cx);
        view.update(cx, |s, cx| {
            let title = s.session.as_ref().unwrap().catalog.active[0].title.clone();
            receive(&title, cx);
            s.pump_links(w, cx);
            assert!(s.links.duplicate.is_some());
            s.dismiss_link(true, w, cx);
            assert!(s.workspace.read(cx).has_provider_form());
            assert!(s.pending_command.is_none());
        });
    });
}

#[test]
fn inbox_is_bounded_and_clear_drops_queued_secret_payloads() {
    let inbox = Inbox::default();
    for _ in 0..32 {
        inbox.receive(
            provider_links::parse(
                "ccswitch://v1/import?resource=provider&name=Fixture&apiKey=synthetic".into(),
            )
            .unwrap(),
        );
    }
    assert_eq!(inbox.0.borrow().0.len(), provider_links::QUEUE_LIMIT);
    inbox.clear();
    assert!(inbox.0.borrow().0.is_empty());
}
