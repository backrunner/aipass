use super::*;
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
    let keystroke =
        cx.update(|_, cx| Keystroke::parse_with_os(key, cx.platform_info().os).unwrap());
    cx.simulate_event(KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.simulate_event(KeyUpEvent { keystroke });
    cx.update(|window, cx| window.draw(cx).clear(cx));
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
        ("down", 1),
        ("down", 1),
        ("home", 0),
        ("up", 0),
        ("end", 1),
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
