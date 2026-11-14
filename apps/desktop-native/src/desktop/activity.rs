//! Capture before nested scroll areas consume wheel input.
use super::Desktop;
use gpui::{canvas, prelude::*, Context, IntoElement, ScrollWheelEvent};

pub(super) fn capture_scroll(cx: &Context<Desktop>) -> impl IntoElement {
    let view = cx.entity().downgrade();
    canvas(
        |_, _, _| (),
        move |_, _, window, _| {
            window.on_mouse_event(move |_: &ScrollWheelEvent, phase, _, cx| {
                if phase.capture() {
                    let _ = view.update(cx, |view, _| {
                        if let Some(session) = &mut view.session {
                            session.record_activity();
                        }
                    });
                }
            });
        },
    )
    .absolute()
    .inset_0()
}
