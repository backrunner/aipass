//! Baseline modal opacity, offset and compositor scale; no frame readback.
use gpui::{
    point, px, AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, Window,
};
use gpui_base::{Transition, TransitionState};
use std::time::Duration;

#[derive(Default)]
pub(super) struct ModalMotion {
    initialized: bool,
    popup: TransitionState<f32>,
    overlay: TransitionState<f32>,
    offset: TransitionState<f32>,
    scale: TransitionState<f32>,
}
#[derive(Clone, Copy)]
pub(super) struct Frame {
    pub popup: f32,
    pub overlay: f32,
    pub visible: bool,
    pub offset: f32,
    pub scale: f32,
}
impl ModalMotion {
    pub fn sample(&mut self, open: bool, window: &mut Window, cx: &mut App) -> Frame {
        if !self.initialized && !open {
            return Frame {
                popup: 0.,
                overlay: 0.,
                visible: false,
                offset: 0.04,
                scale: 0.96,
            };
        }
        if !self.initialized {
            self.popup
                .sample(0., Transition::new(Duration::ZERO), window, cx);
            self.overlay
                .sample(0., Transition::new(Duration::ZERO), window, cx);
            self.offset
                .sample(0.04, Transition::new(Duration::ZERO), window, cx);
            self.scale
                .sample(0.96, Transition::new(Duration::ZERO), window, cx);
            self.initialized = true;
        }
        let popup = self
            .popup
            .sample(
                if open { 1. } else { 0. },
                Transition::new(Duration::from_millis(if open { 260 } else { 200 })).ease(
                    move |t| {
                        let (a, b, c, d) = if open {
                            (0.22, 1., 0.36, 1.)
                        } else {
                            (0.4, 0., 0.85, 0.4)
                        };
                        aipass_desktop_native::motion::cubic_bezier(f64::from(t), a, b, c, d) as f32
                    },
                ),
                window,
                cx,
            )
            .value;
        let overlay = self
            .overlay
            .sample(
                if open { 1. } else { 0. },
                Transition::new(Duration::from_millis(if open { 220 } else { 200 })).ease(|t| {
                    aipass_desktop_native::motion::cubic_bezier(f64::from(t), 0.4, 0., 0.2, 1.)
                        as f32
                }),
                window,
                cx,
            )
            .value;
        // Animate position independently: multiplying by an enter/exit factor
        // made a mid-enter dismissal jump to half of its previous offset.
        let offset = self
            .offset
            .sample(
                if open { 0. } else { 0.02 },
                Transition::new(Duration::from_millis(if open { 260 } else { 200 })).ease(
                    move |t| {
                        let (a, b, c, d) = if open {
                            (0.22, 1., 0.36, 1.)
                        } else {
                            (0.4, 0., 0.85, 0.4)
                        };
                        aipass_desktop_native::motion::cubic_bezier(f64::from(t), a, b, c, d) as f32
                    },
                ),
                window,
                cx,
            )
            .value;
        let scale = self
            .scale
            .sample(
                if open { 1. } else { 0.97 },
                Transition::new(Duration::from_millis(if open { 260 } else { 200 })).ease(
                    move |t| {
                        let (a, b, c, d) = if open {
                            (0.22, 1., 0.36, 1.)
                        } else {
                            (0.4, 0., 0.85, 0.4)
                        };
                        aipass_desktop_native::motion::cubic_bezier(f64::from(t), a, b, c, d) as f32
                    },
                ),
                window,
                cx,
            )
            .value;
        let visible = open || popup > 0. || overlay > 0.;
        if !visible {
            *self = Self::default();
        }
        Frame {
            popup,
            overlay,
            visible,
            offset,
            scale,
        }
    }
}
pub(super) struct Slide {
    child: AnyElement,
    fraction: f32,
}
impl Slide {
    pub fn new(child: impl IntoElement, fraction: f32) -> Self {
        Self {
            child: child.into_any_element(),
            fraction,
        }
    }
}
impl IntoElement for Slide {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for Slide {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        // The child shares our LayoutId, which already includes its layout
        // position. Only add the animation offset, never bounds.origin twice.
        window.with_element_offset(
            point(px(0.), bounds.size.height * self.fraction),
            |window| {
                self.child.prepaint(window, cx);
            },
        );
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

#[cfg(all(test, feature = "parity-render"))]
mod tests {
    use super::*;
    use gpui::{div, Context, Render};
    struct Probe {
        motion: ModalMotion,
        open: bool,
        frame: Option<Frame>,
    }
    impl Render for Probe {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.frame = Some(self.motion.sample(self.open, window, cx));
            div()
        }
    }
    #[gpui::test]
    fn modal_reverses_continuously_and_reduced_motion_finishes(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_base::init);
        let (view, cx) = cx.add_window_view(|_, _| Probe {
            motion: ModalMotion::default(),
            open: false,
            frame: None,
        });
        let draw = |cx: &mut gpui::VisualTestContext| cx.update(|w, cx| w.draw(cx).clear(cx));
        draw(cx);
        view.update(cx, |v, cx| {
            v.open = true;
            cx.notify();
        });
        draw(cx);
        assert_eq!(view.read_with(cx, |v, _| v.frame.unwrap().scale), 0.96);
        cx.executor().advance_clock(Duration::from_millis(80));
        draw(cx);
        let before = view.read_with(cx, |v, _| v.frame.unwrap());
        assert!(before.popup > 0. && before.popup < 1.);
        assert!(before.scale > 0.96 && before.scale < 1.);
        view.update(cx, |v, cx| {
            v.open = false;
            cx.notify();
        });
        draw(cx);
        let reversed = view.read_with(cx, |v, _| v.frame.unwrap());
        assert!((before.popup - reversed.popup).abs() < 0.001);
        assert!((before.offset - reversed.offset).abs() < 0.0001);
        assert!((before.scale - reversed.scale).abs() < 0.0001);
        cx.executor().advance_clock(Duration::from_millis(201));
        draw(cx);
        let closed = view.read_with(cx, |v, _| v.frame.unwrap());
        assert!(!closed.visible);
        assert_eq!(closed.scale, 0.97);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        view.update(cx, |v, cx| {
            v.open = true;
            cx.notify();
        });
        draw(cx);
        let reduced = view.read_with(cx, |v, _| v.frame.unwrap());
        assert_eq!(
            (
                reduced.popup,
                reduced.overlay,
                reduced.offset,
                reduced.scale
            ),
            (1., 1., 0., 1.)
        );
    }
}

#[cfg(all(test, feature = "parity-render"))]
mod idle_tests;
