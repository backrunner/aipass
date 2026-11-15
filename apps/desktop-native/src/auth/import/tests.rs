use super::*;
use gpui::TestAppContext;
#[gpui::test]
fn source_validation_and_secret_retirement(cx: &mut TestAppContext) {
    cx.update(gpui_base::init);
    let (view, cx) = cx.add_window_view(|w, cx| ImportForm::new(false, false, w, cx));
    cx.update(|w, cx| {
        view.update(cx, |v, cx| {
            v.inputs[1].update(cx, |s, cx| s.set_value("fixture-only", w, cx));
            assert!(!v.ready(cx));
            v.inputs[0].update(cx, |s, cx| s.set_value("/synthetic/backup", w, cx));
            assert!(v.ready(cx));
            v.preview("webdav", w, cx);
            assert!(!v.ready(cx));
            v.inputs[2].update(cx, |s, cx| {
                s.set_value("https://fixture.invalid/dav", w, cx)
            });
            assert!(v.ready(cx));
            v.inputs[4].update(cx, |s, cx| s.set_value("fixture-dav-only", w, cx));
            v.set_busy(true, cx);
            assert!(!v.ready(cx));
            v.set_busy(false, cx);
            v.submit(w, cx);
            assert!(v.busy);
            assert!(v.inputs[1].read(cx).value().is_empty());
            assert!(v.inputs[4].read(cx).value().is_empty());
            v.clear(w, cx);
            assert!(!v.busy);
            assert_eq!(v.source.read(cx).selected, "backup");
            for input in &v.inputs {
                assert!(input.read(cx).value().is_empty());
            }
        })
    });
}
#[gpui::test]
fn import_appearance_retains_values_and_editor_identity(cx: &mut TestAppContext) {
    cx.update(gpui_base::init);
    let (view, cx) = cx.add_window_view(|w, cx| ImportForm::new(false, false, w, cx));
    cx.update(|w, cx| {
        view.update(cx, |v, cx| {
            let id = v.inputs[0].entity_id();
            v.inputs[0].update(cx, |s, cx| s.set_value("/synthetic", w, cx));
            v.appearance(true, cx);
            assert_eq!(id, v.inputs[0].entity_id());
            assert_eq!(v.inputs[0].read(cx).value(), "/synthetic");
            assert_eq!(v.source.read(cx).theme.surface, Theme::new(true).surface);
        })
    });
}

#[gpui::test]
fn source_menu_tab_targets_the_first_visible_field(cx: &mut TestAppContext) {
    use gpui::Focusable;
    cx.update(gpui_base::init);
    let (view, cx) = cx.add_window_view(|w, cx| ImportForm::new(false, false, w, cx));
    cx.update(|w, cx| {
        view.update(cx, |v, cx| {
            for (source, index) in [
                ("backup", 0),
                ("vault", 0),
                ("local", 0),
                ("icloud", 1),
                ("webdav", 2),
            ] {
                v.preview(source, w, cx);
                assert_eq!(
                    v.source.read(cx).next.as_ref().unwrap(),
                    &v.inputs[index].read(cx).focus_handle(cx)
                );
            }
        })
    });
}
