//! The reference drawer grows from the right edge with independent axes.
use gpui::{App, Window};
use gpui_base::{Transition, TransitionState};
use std::time::Duration;
#[derive(Default)]
pub(super) struct DrawerMotion {
    initialized: bool,
    shape: TransitionState<f32>,
    opacity: TransitionState<f32>,
    overlay: TransitionState<f32>,
}
pub(super) struct Frame {
    pub visible: bool,
    pub popup: f32,
    pub overlay: f32,
    pub scale: f32,
    pub scale_y: f32,
    pub offset: f32,
}
impl DrawerMotion {
    pub fn sample(&mut self, open: bool, window: &mut Window, cx: &mut App) -> Frame {
        if !self.initialized && open {
            self.shape
                .sample(0., Transition::new(Duration::ZERO), window, cx);
            self.opacity
                .sample(0., Transition::new(Duration::ZERO), window, cx);
            self.overlay
                .sample(0., Transition::new(Duration::ZERO), window, cx);
            self.initialized = true;
        }
        let target = if open { 1. } else { 0. };
        let shape = self
            .shape
            .sample(
                target,
                Transition::new(Duration::from_millis(if open { 380 } else { 280 })).ease(
                    move |t| {
                        let (a, b, c, d) = if open {
                            (0.22, 1., 0.36, 1.)
                        } else {
                            (0.55, 0., 0.7, 0.2)
                        };
                        aipass_desktop_native::motion::cubic_bezier(f64::from(t), a, b, c, d) as f32
                    },
                ),
                window,
                cx,
            )
            .value;
        let popup = self
            .opacity
            .sample(
                target,
                Transition::new(Duration::from_millis(if open { 133 } else { 280 })),
                window,
                cx,
            )
            .value;
        let overlay = self
            .overlay
            .sample(
                target,
                Transition::new(Duration::from_millis(if open { 240 } else { 220 })),
                window,
                cx,
            )
            .value;
        let visible = open || popup > 0. || overlay > 0.;
        if !visible {
            self.initialized = false;
        }
        Frame {
            visible,
            popup,
            overlay,
            scale: 0.04 + 0.96 * shape,
            scale_y: 0.6 + 0.4 * shape,
            offset: 0.2 * (1. - shape),
        }
    }
}
