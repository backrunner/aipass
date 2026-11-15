use super::*;
use gpui::Focusable;
use gpui::{point, size, KeyDownEvent, KeyUpEvent, Keystroke, TestAppContext, VisualTestContext};
use std::{cell::Cell, rc::Rc};

fn workspace(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(gpui_base::init);
    let mut workspace = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Workspace::new(false, false, window, cx));
        workspace = Some(view.clone());
        gpui_base::Root::new(view, window, cx)
    });
    (workspace.unwrap(), cx)
}

fn press(cx: &mut VisualTestContext, key: &str) {
    let keystroke = Keystroke::parse(key).unwrap();
    cx.simulate_event(KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.simulate_event(KeyUpEvent { keystroke });
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn appearance_preserves_search_editor_and_open_form(cx: &mut TestAppContext) {
    let (view, cx) = workspace(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.preview_providers("providers-edit", window, cx);
            let form = view.provider_form.as_ref().unwrap().entity_id();
            let search = view.search.entity_id();
            view.search.update(cx, |input, cx| {
                input.set_value("draft search", window, cx);
                input.focus(window, cx);
            });
            for dark in [true, false, true] {
                view.apply_appearance(dark, cx);
                assert_eq!(view.theme.text, Theme::new(dark).text);
                assert_eq!(view.provider_form.as_ref().unwrap().entity_id(), form);
                assert_eq!(view.search.entity_id(), search);
                assert_eq!(view.search.read(cx).value(), "draft search");
                assert!(view.search.read(cx).focus_handle(cx).is_focused(window));
            }
        })
    });
}

#[gpui::test]
fn public_endpoint_copy_uses_metadata_and_rejects_a_retired_provider(cx: &mut TestAppContext) {
    let (view, cx) = workspace(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.preview_providers("providers", window, cx);
            let entry = view.selected_provider.unwrap();
            let value = aipass_desktop_native::catalog::encode_list([
                "https://example.invalid/?models=a,b",
                "https://second.invalid/v1",
            ]);
            view.copy_endpoint(entry, value.clone(), window, cx);
            assert_eq!(
                cx.read_from_clipboard().and_then(|item| item.text()),
                Some(value.clone())
            );
            assert!(view.provider_action.is_none());
            view.selected_provider = Some(view.catalog.active[1].id);
            view.copy_endpoint(entry, "retired metadata".into(), window, cx);
            assert_eq!(
                cx.read_from_clipboard().and_then(|item| item.text()),
                Some(value)
            );
            view.clear_providers(window, cx);
            assert!(view.copied_endpoint.is_none());
        })
    });
}

#[gpui::test]
fn provider_hover_is_neutral_and_selection_stays_exclusive(cx: &mut TestAppContext) {
    use std::time::Duration;
    let (view, cx) = workspace(cx);
    cx.simulate_resize(size(px(960.), px(640.)));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.preview_providers("providers", window, cx)
        })
    });
    let draw = |cx: &mut VisualTestContext| cx.update(|window, cx| window.draw(cx).clear(cx));
    let color = |cx: &mut VisualTestContext, index| {
        cx.update(|_, cx| {
            cx.global::<crate::parity::Styles>().0[&format!("provider-list-row-{index}")]
                ["background"]["Color"]["solid"]
                .as_str()
                .unwrap()
                .to_owned()
        })
    };
    draw(cx);
    assert_eq!(color(cx, 0), "#2563eb1a");
    assert_eq!(color(cx, 1), "#00000000");
    let second = cx.update(|_, cx| {
        let b = cx.global::<crate::parity::Geometry>().0["provider-list-row-1"];
        point(px(b[0] + b[2] / 2.), px(b[1] + b[3] / 2.))
    });
    cx.simulate_event(gpui::MouseMoveEvent {
        position: second,
        ..Default::default()
    });
    draw(cx);
    cx.executor().advance_clock(Duration::from_millis(40));
    draw(cx);
    let middle = color(cx, 1);
    let alpha = u8::from_str_radix(&middle[middle.len() - 2..], 16).unwrap();
    assert!(alpha > 0 && alpha < 255, "hover must fade rather than snap");
    cx.executor().advance_clock(Duration::from_millis(50));
    draw(cx);
    assert_eq!(color(cx, 1), "#f1f3f8ff");
    assert_eq!(color(cx, 0), "#2563eb1a");
    assert!(view.read_with(cx, |view, _| view.selected_provider
        == Some(view.catalog.active[0].id)));
    cx.simulate_click(second, Default::default());
    draw(cx);
    cx.executor().advance_clock(Duration::from_millis(140));
    draw(cx);
    assert_eq!(color(cx, 1), "#2563eb1a");
    assert_eq!(color(cx, 0), "#00000000");
    assert!(view.read_with(cx, |view, _| view.selected_provider
        == Some(view.catalog.active[1].id)));
}

#[gpui::test]
fn app_menu_dismisses_with_escape_outside_click_and_trigger(cx: &mut TestAppContext) {
    let (view, cx) = workspace(cx);
    cx.simulate_resize(size(px(960.), px(640.)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    for dismissal in ["escape", "outside", "trigger"] {
        cx.simulate_click(point(px(535.), px(25.)), Default::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(view.read_with(cx, |view, _| view.menu_open));
        match dismissal {
            "escape" => cx.simulate_keystrokes("escape"),
            "outside" => cx.simulate_click(point(px(700.), px(300.)), Default::default()),
            _ => cx.simulate_click(point(px(535.), px(25.)), Default::default()),
        }
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(!view.read_with(cx, |view, _| view.menu_open), "{dismissal}");
    }
}

#[gpui::test]
fn keyboard_activates_a_menu_item_once_and_closes_the_menu(cx: &mut TestAppContext) {
    let (view, cx) = workspace(cx);
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    let _subscription = cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event, _| {
            if matches!(event, WorkspaceEvent::Settings) {
                observed.set(observed.get() + 1);
            }
        })
    });
    cx.simulate_resize(size(px(960.), px(640.)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_click(point(px(535.), px(25.)), Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    press(cx, "down");
    press(cx, "enter");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(calls.get(), 1);
    assert!(!view.read_with(cx, |view, _| view.menu_open));
}

#[gpui::test]
fn menu_arrows_stop_at_edges_and_tab_returns_to_workspace_order(cx: &mut TestAppContext) {
    let (view, cx) = workspace(cx);
    cx.simulate_resize(size(px(960.), px(640.)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_click(point(px(535.), px(25.)), Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    for (key, index) in [
        ("down", 0),
        ("down", 2),
        ("down", 2),
        ("home", 0),
        ("up", 0),
        ("end", 2),
    ] {
        press(cx, key);
        cx.update(|window, cx| {
            assert!(
                view.read(cx).menu_focus.items[index].is_focused(window),
                "{key}"
            )
        });
    }
    press(cx, "tab");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| {
        assert!(!view.read(cx).menu_open);
        assert!(view.read(cx).nav_focus[0].is_focused(window));
    });
}

#[gpui::test]
fn favorites_disables_filter_and_other_popovers_are_mutually_exclusive(cx: &mut TestAppContext) {
    let (view, cx) = workspace(cx);
    cx.simulate_resize(size(px(960.), px(640.)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_click(point(px(90.), px(101.)), Default::default());
    cx.simulate_click(point(px(360.), px(64.)), Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(view.read_with(cx, |view, _| view.selected == 1 && !view.filter_open));
    cx.simulate_click(point(px(90.), px(67.)), Default::default());
    cx.simulate_click(point(px(360.), px(64.)), Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(view.read_with(cx, |view, _| view.filter_open));
    cx.simulate_click(point(px(535.), px(25.)), Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(view.read_with(cx, |view, _| view.menu_open && !view.filter_open));
}

#[gpui::test]
fn extended_filter_options_are_reachable_and_dismiss_after_keyboard_selection(
    cx: &mut TestAppContext,
) {
    let (view, cx) = workspace(cx);
    cx.simulate_resize(size(px(960.), px(640.)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_click(point(px(360.), px(64.)), Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    press(cx, "end");
    press(cx, "enter");
    assert!(view.read_with(cx, |view, _| view.selected == 14 && !view.filter_open));
    // Reopening must preserve the API filter rather than fall back to All.
    cx.simulate_click(point(px(360.), px(64.)), Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(view.read_with(cx, |view, _| view.selected == 14 && view.filter_open));
    press(cx, "home");
    press(cx, "enter");
    assert!(view.read_with(cx, |view, _| view.selected == 0 && !view.filter_open));
}

#[gpui::test]
fn provider_add_button_opens_a_dismissible_dialog_at_minimum_size(cx: &mut TestAppContext) {
    let (view, cx) = workspace(cx);
    cx.simulate_resize(size(px(960.), px(640.)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_click(point(px(500.), px(64.)), Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(view.read_with(cx, |view, _| view.provider_form.is_some()));
    cx.simulate_keystrokes("escape");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(view.read_with(cx, |view, _| view.provider_form.is_none()));
}

#[gpui::test]
fn successful_add_clears_a_hiding_query_and_successful_edit_preserves_the_filter(
    cx: &mut TestAppContext,
) {
    let (view, cx) = workspace(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let catalog = aipass_desktop_native::catalog::Catalog::fixture();
            view.catalog = catalog.clone();
            view.selected = 1;
            view.search
                .update(cx, |input, cx| input.set_value("unmatched", window, cx));
            view.open_provider_form(None, window, cx);
            view.sync_providers(
                catalog.clone(),
                ProviderFeedback {
                    selected: Some(catalog.active[0].id),
                    saved: true,
                    ..Default::default()
                },
                window,
                cx,
            );
            assert_eq!(view.selected, 0);
            assert!(view.search.read(cx).value().is_empty());
            assert!(view.provider_form.is_none());
            view.selected = 3;
            view.open_provider_form(Some(catalog.active[1].clone()), window, cx);
            view.sync_providers(
                catalog.clone(),
                ProviderFeedback {
                    selected: Some(catalog.active[1].id),
                    saved: true,
                    ..Default::default()
                },
                window,
                cx,
            );
            assert_eq!(view.selected, 3);
            assert!(view.provider_form.is_none());
            assert_eq!(view.selected_provider, Some(catalog.active[1].id));
        })
    });
}

#[gpui::test]
fn lock_clears_drafts_search_and_catalog_before_any_later_refresh(cx: &mut TestAppContext) {
    let (view, cx) = workspace(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.catalog = aipass_desktop_native::catalog::Catalog::fixture();
            view.selected_provider = Some(view.catalog.active[0].id);
            view.open_provider_form(Some(view.catalog.active[0].clone()), window, cx);
            view.search
                .update(cx, |input, cx| input.set_value("gateway", window, cx));
            view.clear_providers(window, cx);
            assert!(view.provider_form.is_none());
            assert!(view.form_subscription.is_none());
            assert!(view.selected_provider.is_none());
            assert!(view.provider_action.is_none());
            assert!(view.search.read(cx).value().is_empty());
            assert!(
                view.catalog.active.is_empty()
                    && view.catalog.archived.is_empty()
                    && view.catalog.trash.is_empty()
            );
        })
    });
}

#[gpui::test]
fn search_and_background_removal_close_the_old_provider_and_credential_editors(
    cx: &mut TestAppContext,
) {
    let (view, cx) = workspace(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let catalog = aipass_desktop_native::catalog::Catalog::fixture();
            view.catalog = catalog.clone();
            view.select_visible(window, cx);
            view.open_provider_form(Some(catalog.active[0].clone()), window, cx);
            view.credentials.update(cx, |keys, cx| {
                keys.preview("providers-key-edit", window, cx)
            });
            assert!(view.credentials.read(cx).has_editor());
            view.search
                .update(cx, |input, cx| input.set_value("Anthropic", window, cx));
            view.select_visible(window, cx);
            assert_eq!(view.selected_provider, Some(catalog.active[1].id));
            assert!(view.provider_form.is_none());
            assert!(!view.credentials.read(cx).has_editor());
            view.open_provider_form(Some(catalog.active[1].clone()), window, cx);
            view.credentials.update(cx, |keys, cx| {
                keys.preview("providers-key-reveal", window, cx)
            });
            view.sync_providers(Default::default(), Default::default(), window, cx);
            assert!(view.provider_form.is_none());
            assert_eq!(
                view.credentials.read(cx).presentation_state()["revealed"],
                0
            );
            assert!(view.selected_provider.is_none());
        })
    });
}

#[gpui::test]
fn tag_filter_uses_tag_identity_and_clears_when_switching_navigation(cx: &mut TestAppContext) {
    let (view, cx) = workspace(cx);
    cx.simulate_resize(size(px(960.), px(640.)));
    cx.update(|window, cx| {
        view.update(cx, |v, cx| v.preview_providers("providers", window, cx));
        window.draw(cx).clear(cx);
    });
    cx.simulate_click(point(px(360.), px(64.)), Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    press(cx, "end");
    press(cx, "enter");
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            assert_eq!(v.tag_filter.as_deref(), Some("work"));
            assert!(!v.filter_open);
            assert!(v
                .visible_providers(cx)
                .iter()
                .all(|e| e.tags.contains(&"work".to_owned())));
            // A newly inserted tag must not change the chosen filter by index.
            v.catalog.active[0].tags.insert(0, "aaa".into());
            v.select_visible(window, cx);
            assert_eq!(v.tag_filter.as_deref(), Some("work"));
            v.nav_focus[1].focus(window, cx);
            cx.notify();
        });
    });
    press(cx, "enter");
    assert!(view.read_with(cx, |v, _| v.tag_filter.is_none() && v.selected == 1));
}

#[gpui::test]
fn provider_action_menu_uses_keyboard_and_dispatches_once(cx: &mut TestAppContext) {
    let (view, cx) = workspace(cx);
    cx.simulate_resize(size(px(960.), px(640.)));
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.preview_providers("providers", window, cx);
            v.provider_menu_focus.trigger.focus(window, cx);
        });
        window.draw(cx).clear(cx);
    });
    press(cx, "enter");
    press(cx, "down");
    press(cx, "down");
    press(cx, "down");
    press(cx, "enter");
    cx.update(|_,cx| view.update(cx,|v,_| {
        assert!(!v.provider_menu_open);
        assert!(v.provider_busy);
        assert!(matches!(v.take_provider_action(),Some(aipass_desktop_native::catalog::Action::Archive(id)) if Some(id)==v.selected_provider));
    }));
    press(cx, "enter");
    assert!(view.update(cx, |v, _| v.take_provider_action().is_none()));
}

#[gpui::test]
fn editing_preserves_runtime_and_integration_panels(cx: &mut TestAppContext) {
    let (view, cx) = workspace(cx);
    cx.simulate_resize(size(px(960.), px(640.)));
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.preview_providers("providers-edit", window, cx);
            v.sync_credentials(window, cx);
        });
        window.draw(cx).clear(cx);
        let geometry = &cx.global::<crate::parity::Geometry>().0;
        assert!(geometry.contains_key("subscription-runtime-card"));
        assert!(geometry.contains_key("provider-edit-notes"));
        assert!(geometry.contains_key("provider-integration-card"));
        let integrations = view.read(cx).integrations.read(cx);
        assert_eq!(integrations.presentation(cx)["blocked"], true);
        assert!(!integrations.has_request());
    });
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.close_provider_form(window, cx);
            v.sync_credentials(window, cx);
            assert_eq!(v.integrations.read(cx).presentation(cx)["blocked"], false);
        });
    });
}

#[gpui::test]
fn reset_from_edit_keeps_draft_on_cancel_and_reopens_fresh_metadata_on_success(
    cx: &mut TestAppContext,
) {
    let (view, cx) = workspace(cx);
    cx.simulate_resize(size(px(960.), px(640.)));
    let original = cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.preview_providers("providers-edit", window, cx);
            let id = v.provider_form.as_ref().unwrap().entity_id();
            v.reset_provider(window, cx);
            id
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    press(cx, "escape");
    assert!(
        view.read_with(cx, |v, cx| v.provider_form.as_ref().unwrap().entity_id()
            == original
            && !v.tools.read(cx).has_request())
    );
    cx.update(|window, cx| view.update(cx, |v, cx| v.reset_provider(window, cx)));
    let confirm = cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        let b = cx.global::<crate::parity::Geometry>().0["provider-reset-confirm"];
        point(px(b[0] + b[2] / 2.), px(b[1] + b[3] / 2.))
    });
    cx.simulate_click(confirm, Default::default());
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            let request = v.take_tools_request(cx).unwrap();
            assert!(matches!(
                request.operation,
                aipass_desktop_native::provider_tools::Operation::Reset { .. }
            ));
            let mut fresh = v.catalog.active[0].clone();
            fresh.secret_refs.clear();
            v.complete_tools(
                aipass_desktop_native::provider_tools::Completion {
                    scope: request.scope,
                    mutated: true,
                    result: Ok(aipass_desktop_native::provider_tools::Outcome::Reset(
                        Box::new(fresh),
                    )),
                },
                window,
                cx,
            );
            assert_ne!(v.provider_form.as_ref().unwrap().entity_id(), original);
            assert!(v.catalog.active[0].secret_refs.is_empty());
            assert!(v.tools.read(cx).notice().is_some());
            assert!(v.form_is_editing(cx));
        })
    });
}
