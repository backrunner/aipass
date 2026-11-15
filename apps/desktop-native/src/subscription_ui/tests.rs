use super::*;
use gpui::{point, px, size, TestAppContext, VisualTestContext};

pub(super) fn view(cx: &mut TestAppContext) -> (Entity<Subscriptions>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_base::init(cx);
        crate::menu::init(cx);
    });
    let mut entity = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Subscriptions::new(false, false, window, cx));
        entity = Some(view.clone());
        gpui_base::Root::new(view, window, cx)
    });
    cx.simulate_resize(size(px(960.), px(640.)));
    (entity.unwrap(), cx)
}
fn login(flow: Flow) -> Login {
    Login {
        flow,
        pending: true,
        entry_id: None,
        url: Some("https://github.com/login/device".into()),
        code_required: true,
        user_code: None,
        instructions: None,
        error: None,
    }
}

#[gpui::test]
fn close_and_reopen_cancels_late_start_without_populating_new_dialog(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            s.fixture("subscription-api", window, cx);
            let old = s.generation;
            s.api_key.update(cx, |input, cx| {
                input.set_value("synthetic-only", window, cx)
            });
            s.start(window, cx);
            assert_eq!(s.pending, Some(Kind::Start));
            s.close(window, cx);
            assert!(s.api_key.read(cx).value().is_empty());
            s.show(None, None, window, cx);
            s.outbox.clear();
            let flow = Flow::Community(Uuid::new_v4());
            s.complete(
                Completion {
                    generation: old,
                    kind: Kind::Start,
                    result: Ok(Outcome::Login(login(flow))),
                },
                window,
                cx,
            );
            assert!(s.open);
            assert!(s.login.is_none());
            assert!(matches!(s.take_request().unwrap().operation,Operation::Cancel(f) if f==flow));
        })
    });
}

#[gpui::test]
fn retained_region_and_reconnect_id_survive_start_but_key_is_erased(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            s.fixture("subscription-region", window, cx);
            let id = Uuid::new_v4();
            s.reconnect_entry = Some(id);
            s.choices.insert("region".into(), "intl".into());
            s.api_key
                .update(cx, |i, cx| i.set_value("synthetic-key", window, cx));
            s.start(window, cx);
            let Operation::Start(input) = s.take_request().unwrap().operation else {
                panic!("expected typed start")
            };
            assert_eq!(input.entry_id, Some(id));
            assert_eq!(input.inputs["region"].expose(), "intl");
            assert_eq!(input.api_key.unwrap().expose(), "synthetic-key");
            assert!(s.api_key.read(cx).value().is_empty());
            assert_eq!(s.choices["region"], "intl");
            s.choose("codex".into(), window, cx);
            assert_eq!(s.provider, "bailian-coding-plan");
        })
    });
}

#[gpui::test]
fn cancel_honors_an_account_committed_before_cancellation(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            s.fixture("subscription-device", window, cx);
            s.cancel(cx);
            assert_eq!(s.pending, Some(Kind::Cancel));
            let mut completed = login(Flow::Community(Uuid::nil()));
            completed.pending = false;
            completed.entry_id = Some(Uuid::new_v4());
            s.complete(
                Completion {
                    generation: s.generation,
                    kind: Kind::Cancel,
                    result: Ok(Outcome::Login(completed)),
                },
                window,
                cx,
            );
            assert!(!s.open);
            assert!(s.error.is_empty());
        })
    });
}

#[gpui::test]
fn refresh_and_console_completions_are_scoped_across_a_b_a_selection(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            let a = fixtures::entry("subscription-usage");
            let id = a.id;
            s.sync_entry(Some(a.clone()), window, cx);
            s.refresh_usage(cx);
            let old = s.usage_generation;
            let mut b = a.clone();
            b.id = Uuid::new_v4();
            s.sync_entry(Some(b), window, cx);
            s.sync_entry(Some(a), window, cx);
            s.complete(
                Completion {
                    generation: old,
                    kind: Kind::Refresh(id),
                    result: Err(aipass_agent::AgentCommandError {
                        code: None,
                        message: "stale failure".into(),
                    }),
                },
                window,
                cx,
            );
            assert!(!s.usage_pending);
            assert!(s.usage_error.is_empty());
            s.complete(
                Completion {
                    generation: old,
                    kind: Kind::Console(id),
                    result: Err(aipass_agent::AgentCommandError {
                        code: None,
                        message: "stale console failure".into(),
                    }),
                },
                window,
                cx,
            );
            assert!(s.usage_error.is_empty());
        })
    });
}

#[gpui::test]
fn import_retry_keeps_successful_sources_out_and_lock_clears_all_requests(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            s.fixture("subscription-import", window, cx);
            s.retry_import(cx);
            let Operation::Import(input) = s.take_request().unwrap().operation else {
                panic!()
            };
            let retry = input.retry.unwrap();
            assert_eq!(retry.source_ids.len(), 3);
            s.queue(Operation::Catalog, cx);
            s.clear(window, cx);
            assert!(!s.open);
            assert!(s.take_request().is_none());
            assert!(s.entry.is_none());
        })
    });
}

#[gpui::test]
fn method_and_region_have_distinct_focus_and_escape_keeps_dialog_open(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| view.update(cx, |s, cx| s.fixture("subscription-region", window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            assert_ne!(s.picker_triggers["method"], s.picker_triggers["region"]);
            s.picker_triggers["region"].focus(window, cx);
        })
    });
    cx.simulate_keystrokes("down");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(view.read_with(cx, |s, _| s.picker.as_deref() == Some("region")));
    cx.simulate_keystrokes("escape");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(view.read_with(cx, |s, _| s.open && s.picker.is_none()));
    // A click within the popup must not hit the modal backdrop.
    cx.simulate_click(point(px(450.), px(220.)), Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(view.read_with(cx, |s, _| s.open));
}

#[test]
fn catalog_methods_prompts_names_and_artwork_cover_both_locales() {
    let providers = model::presentation::catalog();
    assert!(providers.len() >= 41);
    for chinese in [false, true] {
        let locale = Locale::new(chinese);
        for provider in providers {
            assert!(!provider.methods.is_empty(), "{}", provider.id);
            for key in provider
                .name_key
                .iter()
                .chain(provider.methods.iter().flat_map(|m| {
                    m.label_key
                        .iter()
                        .chain(m.hint_key.iter())
                        .chain(m.prompts.iter().flat_map(|p| {
                            p.message_key
                                .iter()
                                .chain(p.options.iter().flat_map(|o| o.label_key.iter()))
                        }))
                }))
            {
                assert_ne!(locale.text(key), *key);
            }
            let name = crate::provider_icon::name(
                &provider.id,
                aipass_provider_registry::CredentialKind::OAuth,
            );
            assert!(
                crate::provider_icon::asset(&format!("provider-brand/{name}.svg")).is_some(),
                "missing {}",
                provider.id
            );
        }
    }
}

#[gpui::test]
fn modal_first_layout_is_visible_and_selected_provider_is_scrolled(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| {
        cx.set_reduce_motion(true);
        view.update(cx, |s, cx| s.fixture("subscription-region", window, cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|_, cx| {
        let header = cx.global::<crate::parity::Geometry>().0["subscription-header"];
        assert!(header[0] >= 0. && header[0] + header[2] <= 960.);
        assert!(header[1] >= 0. && header[1] + header[3] < 640.);
        let s = view.read(cx);
        assert!(
            s.provider_scroll.offset().y < px(-100.),
            "selected provider must be visible after first layout"
        );
    });
}
