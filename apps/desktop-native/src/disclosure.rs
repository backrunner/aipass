//! Shared disclosure timing from Collapsible.svelte. Height and
//! opacity/chevron have independent clocks, including interrupted reversals.
use gpui::{div, percentage, prelude::*, px, svg, App, ElementId, Rgba, Transformation, Window};
use gpui_base::{MotionReveal, Transition, TransitionState};
use std::time::Duration;

mod measured_body;

#[derive(Default)]
pub(crate) struct Disclosure {
    height: TransitionState<f32>,
    fade: TransitionState<f32>,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Frame {
    pub height: f32,
    pub fade: f32,
    open: bool,
}

impl Disclosure {
    pub fn sample(&self, open: bool, window: &mut Window, cx: &mut App) -> Frame {
        let target = if open { 1.0 } else { 0.0 };
        let policy = |duration| {
            Transition::new(Duration::from_millis(duration)).ease(|time| {
                aipass_desktop_native::motion::cubic_bezier(f64::from(time), 0.25, 0.1, 0.25, 1.0)
                    as f32
            })
        };
        Frame {
            height: self.height.sample(target, policy(220), window, cx).value,
            fade: self.fade.sample(target, policy(180), window, cx).value,
            open,
        }
    }
}

impl Frame {
    pub fn chevron(self, color: Rgba, size: f32) -> impl IntoElement {
        svg()
            .path("chevron-right")
            .size(px(size))
            .text_color(color)
            .with_transformation(Transformation::rotate(percentage(self.fade * 0.25)))
    }

    pub fn body(self, id: impl Into<ElementId>, body: impl IntoElement) -> impl IntoElement {
        // Keep the layout probe mounted at zero to measure natural height before
        // expansion. The opacity must live on Div, which owns the paint scope.
        // An opening zero-height frame must already register the newly focused
        // input; only a fully closed body skips prepaint and paint.
        MotionReveal::new(
            id,
            self.height,
            measured_body::MeasuredBody::new(
                self.open || self.height > 0.,
                div().opacity(self.fade).child(body).into_any_element(),
            )
            .into_any_element(),
        )
    }
}

#[cfg(all(test, feature = "parity-render"))]
mod tests;
