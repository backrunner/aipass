use super::*;
use crate::parity::Observe;
use gpui::{div, point, prelude::*, Context, MouseButton, Render, ScrollDelta, ScrollWheelEvent};
use std::{cell::Cell, rc::Rc};

struct Probe {
    scale: f32,
    origin: gpui::Point<f32>,
    offset: gpui::Point<Pixels>,
    relative_offset: gpui::Point<f32>,
    clicks: usize,
    position: Rc<Cell<gpui::Point<Pixels>>>,
    scroll: Rc<Cell<f32>>,
}
impl Render for Probe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let position = self.position.clone();
        let scroll = self.scroll.clone();
        Scale::new(
            div().w(px(200.)).h(px(100.)).child(
                div()
                    .id("scaled-button")
                    .w(px(20.))
                    .h(px(20.))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, window, _| {
                            this.clicks += 1;
                            position.set(event.position);
                            assert_eq!(event.position, window.mouse_position());
                        }),
                    )
                    .on_scroll_wheel(move |event, _, _| {
                        if let ScrollDelta::Pixels(delta) = event.delta {
                            scroll.set(f32::from(delta.y));
                        }
                    })
                    .observe("scaled-button"),
            ),
            self.scale,
        )
        .origin(self.origin)
        .translate(self.offset)
        .translate_relative(self.relative_offset)
    }
}

#[gpui::test]
fn scaled_buttons_receive_visual_coordinates_and_unscaled_scroll(cx: &mut gpui::TestAppContext) {
    let position = Rc::new(Cell::new(point(px(0.), px(0.))));
    let scroll = Rc::new(Cell::new(0.));
    let (view, cx) = cx.add_window_view(|_, _| Probe {
        scale: 0.5,
        origin: point(0.5, 0.5),
        offset: point(px(0.), px(0.)),
        relative_offset: point(0., 0.),
        clicks: 0,
        position: position.clone(),
        scroll: scroll.clone(),
    });
    cx.update(|w, cx| w.draw(cx).clear(cx));
    assert_eq!(
        cx.update(|_, cx| cx.global::<crate::parity::Geometry>().0["scaled-button"]),
        [50., 25., 10., 10.]
    );
    // The original [0,20] button is painted at [50,60] x [25,35].
    cx.simulate_click(point(px(55.), px(30.)), Default::default());
    assert_eq!(view.read_with(cx, |v, _| v.clicks), 1);
    assert_eq!(position.get(), point(px(10.), px(10.)));
    cx.simulate_click(point(px(5.), px(5.)), Default::default());
    assert_eq!(view.read_with(cx, |v, _| v.clicks), 1);
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(55.), px(30.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(8.))),
        ..Default::default()
    });
    assert_eq!(scroll.get(), 16.);
    view.update(cx, |v, cx| {
        v.scale = 1.;
        cx.notify();
    });
    cx.update(|w, cx| w.draw(cx).clear(cx));
    cx.simulate_click(point(px(5.), px(5.)), Default::default());
    assert_eq!(view.read_with(cx, |v, _| v.clicks), 2);
    assert_eq!(position.get(), point(px(5.), px(5.)));
}

#[cfg(target_os = "macos")]
mod gpu;

mod containers;
mod ime;

#[gpui::test]
fn dropdown_origin_and_translation_match_pointer_geometry(cx: &mut gpui::TestAppContext) {
    let position = Rc::new(Cell::new(point(px(0.), px(0.))));
    let (view, cx) = cx.add_window_view(|_, _| Probe {
        scale: 0.5,
        origin: point(0., 0.),
        offset: point(px(15.), px(7.)),
        relative_offset: point(0., 0.),
        clicks: 0,
        position: position.clone(),
        scroll: Rc::new(Cell::new(0.)),
    });
    cx.update(|w, cx| w.draw(cx).clear(cx));
    assert_eq!(
        cx.update(|_, cx| cx.global::<crate::parity::Geometry>().0["scaled-button"]),
        [15., 7., 10., 10.]
    );
    cx.simulate_click(point(px(20.), px(12.)), Default::default());
    assert_eq!(view.read_with(cx, |v, _| v.clicks), 1);
    assert_eq!(position.get(), point(px(10.), px(10.)));
}

#[gpui::test]
fn position_feedback_updates_input_origin_before_the_same_frame_is_painted(
    cx: &mut gpui::TestAppContext,
) {
    struct Tracked {
        clicks: Rc<Cell<usize>>,
    }
    impl Render for Tracked {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let clicks = self.clicks.clone();
            let origin = Rc::new(Cell::new(point(0., 0.)));
            let content = Scale::new(
                div().w(px(200.)).h(px(100.)).child(
                    div()
                        .w(px(20.))
                        .h(px(20.))
                        .on_mouse_down(MouseButton::Left, move |_, _, _| {
                            clicks.set(clicks.get() + 1)
                        })
                        .observe("positioned-button"),
                ),
                0.5,
            )
            .origin_from(origin.clone());
            // Positioner resolves after content creation, before child prepaint.
            origin.set(point(1., 1.));
            content
        }
    }
    let clicks = Rc::new(Cell::new(0));
    let (_, cx) = cx.add_window_view(|_, _| Tracked {
        clicks: clicks.clone(),
    });
    cx.update(|w, cx| w.draw(cx).clear(cx));
    assert_eq!(
        cx.update(|_, cx| cx.global::<crate::parity::Geometry>().0["positioned-button"]),
        [100., 50., 10., 10.]
    );
    cx.simulate_click(point(px(105.), px(55.)), Default::default());
    assert_eq!(clicks.get(), 1);
    cx.simulate_click(point(px(5.), px(5.)), Default::default());
    assert_eq!(clicks.get(), 1);
}

#[gpui::test]
fn relative_dialog_translation_uses_current_layout_for_hit_testing(cx: &mut gpui::TestAppContext) {
    let position = Rc::new(Cell::new(point(px(0.), px(0.))));
    let (view, cx) = cx.add_window_view(|_, _| Probe {
        scale: 0.96,
        origin: point(0.5, 0.5),
        offset: point(px(0.), px(0.)),
        relative_offset: point(0., 0.04),
        clicks: 0,
        position: position.clone(),
        scroll: Rc::new(Cell::new(0.)),
    });
    cx.update(|w, cx| w.draw(cx).clear(cx));
    // 4% of the current 100px popup height, plus scaling around its center.
    cx.simulate_click(point(px(13.6), px(15.6)), Default::default());
    assert_eq!(view.read_with(cx, |v, _| v.clicks), 1);
    assert!((f32::from(position.get().x) - 10.).abs() < 0.001);
    assert!((f32::from(position.get().y) - 10.).abs() < 0.001);
}
