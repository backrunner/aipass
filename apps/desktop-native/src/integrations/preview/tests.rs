use super::*;
use aipass_agent_protocol::ToolConfigTool;
use gpui::TestAppContext;

#[gpui::test]
fn diff_has_scroll_extent_and_file_mode_navigation_resets_scroll(cx: &mut TestAppContext) {
    cx.update(gpui_base::init);
    let mut preview = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            Preview::new(
                ToolConfigPreviewResponse {
                    preview_id: Some("fixture".into()),
                    tool: ToolConfigTool::Codex,
                    mode: ToolConfigMode::Plaintext,
                    entry_id: uuid::Uuid::nil(),
                    entry_title: "fixture".into(),
                    target_path: String::new(),
                    summary: String::new(),
                    preview: String::new(),
                    files: serde_json::from_str(include_str!(
                        "../../../../../tests/desktop-parity/reference/business-preview-files.json"
                    ))
                    .unwrap(),
                },
                true,
                false,
                false,
                window,
                cx,
            )
        });
        preview = Some(view.clone());
        gpui_base::Root::new(view, window, cx)
    });
    let preview = preview.unwrap();
    cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(640.)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    for (side, key) in [(0, "right"), (1, "pagedown")] {
        cx.update(|window, cx| preview.read(cx).panes[side].clone().focus(window, cx));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let keystroke =
            cx.update(|_, cx| gpui::Keystroke::parse_with_os(key, cx.platform_info().os).unwrap());
        cx.simulate_event(gpui::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(gpui::KeyUpEvent { keystroke });
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    assert!(
        preview.read_with(cx, |view, _| view.scroll.offset().x < px(0.)
            && view.scroll.offset().y < px(0.))
    );
    cx.update(|_, cx| {
        preview.update(cx, |view, cx| {
            assert!(view.scroll.max_offset().x > gpui::px(100.));
            assert!(view.scroll.max_offset().y > gpui::px(100.));
            view.navigate(true, cx);
            assert!(view.scroll.offset().y < gpui::px(0.));
            view.choose_file(1, cx);
            assert_eq!(
                view.scroll.offset(),
                gpui::point(gpui::px(0.), gpui::px(0.))
            );
            view.choose_mode(true, cx);
            assert!(view.full);
            assert_eq!(view.change, 0);
            view.busy = true;
            assert!(!view.can_confirm());
        })
    });
    cx.update(|window, cx| {
        preview.update(cx, |view, cx| {
            view.choose_file(0, cx);
            view.panes[0].focus(window, cx);
        });
        window.draw(cx).clear(cx);
    });
    // The complete-file view also needs real keyboard scrolling, not just the
    // shared diff pane handler or direct writes to the scroll handle.
    let keystroke = cx
        .update(|_, cx| gpui::Keystroke::parse_with_os("pagedown", cx.platform_info().os).unwrap());
    cx.simulate_event(gpui::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.simulate_event(gpui::KeyUpEvent { keystroke });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(preview.read_with(cx, |view, _| view.full && view.scroll.offset().y < px(0.)));
}
