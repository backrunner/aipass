//! Shared dropdown timing and compositor geometry from the frozen desktop styles.
use gpui::{div, point, prelude::*, px, App, IntoElement, Window};
use gpui_base::{Transition, TransitionState};
use std::time::Duration;

#[derive(Default)]
struct State {
    initialized: bool,
    opacity: TransitionState<f32>,
    scale: TransitionState<f32>,
    offset: TransitionState<f32>,
}
#[derive(Clone, Copy)]
pub(crate) struct Frame {
    pub visible: bool,
    pub opacity: f32,
    pub scale: f32,
    pub offset: f32,
}
/// Positioner resolves flips before child prepaint. Share that same origin with
/// compositor painting and hit testing so the first visible frame is correct.
#[derive(Clone)]
pub(crate) struct Origin(std::rc::Rc<std::cell::Cell<gpui::Point<f32>>>);
impl Origin {
    pub fn new(end: bool) -> Self {
        Self(std::rc::Rc::new(std::cell::Cell::new(point(
            if end { 1. } else { 0. },
            0.,
        ))))
    }
    pub fn resolve(
        &self,
        position: gpui_base::ResolvedPosition,
        trigger: gpui::Bounds<gpui::Pixels>,
    ) {
        let mut origin = self.0.get();
        let above = position
            .placement
            .map_or(position.bounds.bottom() <= trigger.top(), |placement| {
                placement == gpui_base::Placement::Top
            });
        origin.y = if above { 1. } else { 0. };
        self.0.set(origin);
    }
}
impl Frame {
    pub fn wrap_positioned(self, child: impl IntoElement, origin: Origin) -> impl IntoElement {
        crate::composite::Scale::new(div().opacity(self.opacity).child(child), self.scale)
            .origin_from(origin.0)
            .translate(point(px(0.), px(self.offset)))
    }
}
pub(crate) fn sample(
    id: impl Into<gpui::ElementId>,
    open: bool,
    window: &mut Window,
    cx: &mut App,
) -> Frame {
    let state = window.use_keyed_state(id, cx, |_, _| State::default());
    state.update(cx, |state, cx| {
        if !state.initialized && !open {
            return Frame {
                visible: false,
                opacity: 0.,
                scale: 0.96,
                offset: -4.,
            };
        }
        if !state.initialized {
            state
                .opacity
                .sample(0., Transition::new(Duration::ZERO), window, cx);
            state
                .scale
                .sample(0.96, Transition::new(Duration::ZERO), window, cx);
            state
                .offset
                .sample(-4., Transition::new(Duration::ZERO), window, cx);
            state.initialized = true;
        }
        let transition = || {
            Transition::new(Duration::from_millis(if open { 160 } else { 120 })).ease(move |time| {
                let (a, b, c, d) = if open {
                    (0.22, 1., 0.36, 1.)
                } else {
                    (0.4, 0., 0.85, 0.4)
                };
                aipass_desktop_native::motion::cubic_bezier(f64::from(time), a, b, c, d) as f32
            })
        };
        let opacity = state
            .opacity
            .sample(if open { 1. } else { 0. }, transition(), window, cx)
            .value;
        let scale = state
            .scale
            .sample(if open { 1. } else { 0.98 }, transition(), window, cx)
            .value;
        let offset = state
            .offset
            .sample(if open { 0. } else { -4. }, transition(), window, cx)
            .value;
        let visible = open || opacity > 0.;
        if !visible {
            *state = State::default();
        }
        Frame {
            visible,
            opacity,
            scale,
            offset,
        }
    })
}

#[cfg(all(test, feature = "parity-render"))]
mod tests {
    use super::*;
    use gpui::{Context, Render};
    #[test]
    fn origin_tracks_above_and_below_placement_before_child_paint() {
        let origin = Origin::new(true);
        let trigger = gpui::Bounds::new(point(px(400.), px(500.)), gpui::size(px(100.), px(32.)));
        let placement = |y| gpui_base::ResolvedPosition {
            bounds: gpui::Bounds::new(point(px(300.), px(y)), gpui::size(px(200.), px(100.))),
            placement: None,
        };
        origin.resolve(placement(396.), trigger);
        assert_eq!(origin.0.get(), point(1., 1.));
        origin.resolve(placement(536.), trigger);
        assert_eq!(origin.0.get(), point(1., 0.));
    }
    struct Probe {
        open: bool,
        frame: Option<Frame>,
    }
    impl Render for Probe {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.frame = Some(sample("menu-probe", self.open, window, cx));
            div()
        }
    }
    #[gpui::test]
    fn menu_keeps_exit_frames_reverses_continuously_and_honors_reduced_motion(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(gpui_base::init);
        let (view, cx) = cx.add_window_view(|_, _| Probe {
            open: false,
            frame: None,
        });
        let draw = |cx: &mut gpui::VisualTestContext| cx.update(|w, cx| w.draw(cx).clear(cx));
        draw(cx);
        assert_eq!(
            cx.update(|w, cx| w.simulate_next_frame(cx)),
            0,
            "a closed menu must not continuously schedule frames"
        );
        view.update(cx, |v, cx| {
            v.open = true;
            cx.notify();
        });
        draw(cx);
        assert_eq!(view.read_with(cx, |v, _| v.frame.unwrap().scale), 0.96);
        cx.executor().advance_clock(Duration::from_millis(80));
        draw(cx);
        let before = view.read_with(cx, |v, _| v.frame.unwrap());
        assert!(before.visible && before.opacity > 0. && before.opacity < 1.);
        view.update(cx, |v, cx| {
            v.open = false;
            cx.notify();
        });
        draw(cx);
        let reversed = view.read_with(cx, |v, _| v.frame.unwrap());
        assert!(reversed.visible);
        assert!((reversed.opacity - before.opacity).abs() < 0.001);
        assert!((reversed.scale - before.scale).abs() < 0.001);
        assert!((reversed.offset - before.offset).abs() < 0.001);
        cx.executor().advance_clock(Duration::from_millis(121));
        draw(cx);
        assert!(!view.read_with(cx, |v, _| v.frame.unwrap().visible));
        cx.update(|w, cx| w.simulate_next_frame(cx));
        draw(cx);
        assert_eq!(cx.update(|w, cx| w.simulate_next_frame(cx)), 0);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        view.update(cx, |v, cx| {
            v.open = true;
            cx.notify();
        });
        draw(cx);
        let frame = view.read_with(cx, |v, _| v.frame.unwrap());
        assert_eq!((frame.opacity, frame.scale, frame.offset), (1., 1., 0.));
        view.update(cx, |v, cx| {
            v.open = false;
            cx.notify();
        });
        draw(cx);
        assert!(!view.read_with(cx, |v, _| v.frame.unwrap().visible));
    }
}
