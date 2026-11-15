use super::*;
use gpui::{point, size, Entity, TestAppContext, VisualTestContext};

fn setup(cx: &mut TestAppContext) -> (Entity<ErrorToast>, &mut VisualTestContext) {
    cx.update(gpui_base::init);
    let mut toast = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| ErrorToast::new(false, false, window, cx));
        view.update(cx, |view, cx| {
            view.show(fixture_message(false, false), window, cx)
        });
        toast = Some(view.clone());
        gpui_base::Root::new(view, window, cx)
    });
    cx.simulate_resize(size(px(960.), px(640.)));
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    (toast.unwrap(), cx)
}

fn close_center(cx: &mut VisualTestContext) -> gpui::Point<gpui::Pixels> {
    cx.update(|_, cx| {
        let [x, y, w, h] = cx.global::<crate::parity::Geometry>().0["toast-close"];
        point(px(x + w / 2.), px(y + h / 2.))
    })
}

#[gpui::test]
fn hovering_and_keyboard_focus_both_pause_the_owned_timer(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    let center = close_center(cx);
    cx.simulate_mouse_move(center, None, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(view.read_with(cx, |view, _| view.task.is_none()));
    cx.update(|window, cx| view.read(cx).focus.clone().focus(window, cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    cx.update(|window, cx| assert!(view.read(cx).focus.is_focused(window)));
    cx.simulate_mouse_move(point(px(0.), px(0.)), None, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        view.read_with(cx, |view, _| view.task.is_none()),
        "{}",
        view.read_with(cx, |view, _| format!("{:?}", view.timer))
    );
    cx.update(|window, cx| window.blur(cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.task.is_some()));
    assert!(view.read_with(cx, |view, _| view
        .timer
        .remaining(view.epoch.elapsed())
        .unwrap()
        .as_secs_f32()
        > 5.9));
}

#[gpui::test]
fn close_button_dismisses_and_removes_its_pending_timer(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    let center = close_center(cx);
    cx.simulate_click(center, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(view.read_with(cx, |view, _| !view.timer.visible
        && view.task.is_none()
        && view.message.is_empty()));
}

#[gpui::test]
fn replacement_resets_long_message_scroll(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.show(fixture_message(false, true), window, cx)
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        view.read_with(cx, |view, _| view.scroll.max_offset().y > px(160.)),
        "long toast must actually scroll"
    );
    cx.update(|_, cx| {
        assert_eq!(
            cx.global::<crate::parity::Geometry>().0["toast-fade"][3],
            160.
        )
    });
    cx.update(|_, cx| view.read(cx).scroll.set_offset(point(px(0.), px(-100.))));
    cx.update(|window, cx| view.update(cx, |view, cx| view.show("replacement".into(), window, cx)));
    assert_eq!(view.read_with(cx, |view, _| view.scroll.offset().y), px(0.));
}
