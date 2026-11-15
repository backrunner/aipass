use super::*;
use gpui::TestAppContext;
fn build(window: &mut Window, cx: &mut Context<Agents>) -> Agents {
    let subscriptions =
        cx.new(|cx| crate::subscription_ui::Subscriptions::new(false, false, window, cx));
    Agents::new(subscriptions, false, false, cx)
}
#[gpui::test]
fn catalog_refresh_retires_preview_capability_and_late_completions(cx: &mut TestAppContext) {
    cx.update(gpui_base::init);
    let (view, cx) = cx.add_window_view(build);
    cx.update(|w, cx| {
        view.update(cx, |v, cx| {
            v.fixture("agents-ready", w, cx);
            v.prepare(w, cx);
            let request = v.take_request().unwrap();
            assert_eq!(v.pending, Some(Kind::Preview));
            let mut entries = v.entries.clone();
            entries[0].secret_refs.remove(0);
            v.sync(&entries, cx);
            assert!(v.plan.is_none());
            assert!(v.pending.is_none());
            v.complete(
                Completion {
                    generation: request.generation,
                    kind: Kind::Preview,
                    result: Err(aipass_agent::AgentCommandError {
                        code: None,
                        message: "stale".into(),
                    }),
                },
                w,
                cx,
            );
            assert!(v.preview.is_none());
            assert!(v.error.is_empty());
            assert!(v.picker.read(cx).selected.is_empty());
        })
    });
}
#[gpui::test]
fn switching_requires_available_runtime_and_nonconflicting_status(cx: &mut TestAppContext) {
    cx.update(gpui_base::init);
    let (view, cx) = cx.add_window_view(build);
    cx.update(|w, cx| {
        view.update(cx, |v, cx| {
            v.fixture("agents-ready", w, cx);
            assert!(v.can_switch(cx));
            for scenario in [
                "agents-conflict",
                "agents-unavailable",
                "agents-cursor",
                "agents-scanning",
            ] {
                v.fixture(scenario, w, cx);
                assert!(!v.can_switch(cx), "{scenario}");
                v.prepare(w, cx);
                assert!(v.take_request().is_none());
            }
        })
    });
}
#[gpui::test]
fn appearance_preserves_selection_and_lock_discards_management_state(cx: &mut TestAppContext) {
    cx.update(gpui_base::init);
    let (view, cx) = cx.add_window_view(build);
    cx.update(|w, cx| {
        view.update(cx, |v, cx| {
            v.fixture("agents-ready", w, cx);
            let id = v.picker.entity_id();
            let value = v.picker.read(cx).selected.clone();
            v.appearance(true, cx);
            assert_eq!(v.picker.entity_id(), id);
            assert_eq!(v.picker.read(cx).selected, value);
            assert_eq!(v.picker.read(cx).theme.surface, Theme::new(true).surface);
            v.scan(cx);
            let request = v.take_request().unwrap();
            v.clear(cx);
            v.complete(
                Completion {
                    generation: request.generation,
                    kind: Kind::Scan,
                    result: Ok(Outcome::Scanned {
                        detections: vec![],
                        statuses: vec![],
                    }),
                },
                w,
                cx,
            );
            assert!(!v.active);
            assert!(!v.loaded);
            assert!(v.entries.is_empty());
            assert!(v.summary.is_none());
            assert!(v.preview.is_none());
            assert!(v.setup.is_none());
        })
    });
}
#[gpui::test]
fn manual_setup_has_no_write_plan_and_closes_if_key_is_removed(cx: &mut TestAppContext) {
    cx.update(gpui_base::init);
    let (view, cx) = cx.add_window_view(build);
    cx.update(|w, cx| {
        view.update(cx, |v, cx| {
            v.fixture("agents-setup", w, cx);
            assert!(v.setup.is_some());
            let generation = v.generation;
            v.focused(cx);
            assert_eq!(v.generation, generation);
            assert!(v.plan.is_none());
            assert!(v.take_request().is_none());
            let mut entries = v.entries.clone();
            entries[0].secret_refs.clear();
            v.sync(&entries, cx);
            assert!(v.setup.is_none());
        })
    });
}
