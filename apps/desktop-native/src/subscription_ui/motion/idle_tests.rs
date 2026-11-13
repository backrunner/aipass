use super::*;
use gpui::{div, Context, Render};
#[derive(Default)]
struct Probe {
    open: bool,
    motion: ModalMotion,
}
impl Render for Probe {
    fn render(&mut self, w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.motion.sample(self.open, w, cx);
        div()
    }
}
#[gpui::test]
fn hidden_modal_stops_requesting_frames_before_open_and_after_close(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_base::init);
    let (view, cx) = cx.add_window_view(|_, _| Probe::default());
    let draw = |cx: &mut gpui::VisualTestContext| cx.update(|w, cx| w.draw(cx).clear(cx));
    draw(cx);
    assert_eq!(cx.update(|w, cx| w.simulate_next_frame(cx)), 0);
    view.update(cx, |v, cx| {
        v.open = true;
        cx.notify();
    });
    draw(cx);
    assert!(cx.update(|w, cx| w.simulate_next_frame(cx)) > 0);
    cx.executor().advance_clock(Duration::from_millis(300));
    draw(cx);
    view.update(cx, |v, cx| {
        v.open = false;
        cx.notify();
    });
    draw(cx);
    assert!(cx.update(|w, cx| w.simulate_next_frame(cx)) > 0);
    cx.executor().advance_clock(Duration::from_millis(210));
    draw(cx);
    cx.update(|w, cx| w.simulate_next_frame(cx));
    draw(cx);
    assert_eq!(cx.update(|w, cx| w.simulate_next_frame(cx)), 0);
}
