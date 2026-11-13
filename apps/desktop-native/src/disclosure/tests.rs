use super::*;
use gpui::{canvas, Context, TestAppContext};
use std::{cell::Cell, rc::Rc};

struct Probe {
    progress: f32,
    prepaints: Rc<Cell<usize>>,
    paints: Rc<Cell<usize>>,
}
impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let prepaints = self.prepaints.clone();
        let paints = self.paints.clone();
        Frame {
            height: self.progress,
            fade: self.progress,
            open: self.progress > 0.,
        }
        .body(
            "disclosure-probe",
            canvas(
                move |_, _, _| prepaints.set(prepaints.get() + 1),
                move |_, _, _, _| paints.set(paints.get() + 1),
            )
            .w_full()
            .h(px(100.)),
        )
    }
}

#[gpui::test]
fn closed_body_measures_without_prepaint_or_paint(cx: &mut TestAppContext) {
    let prepaints = Rc::new(Cell::new(0));
    let paints = Rc::new(Cell::new(0));
    let (view, cx) = cx.add_window_view(|_, _| Probe {
        progress: 0.,
        prepaints: prepaints.clone(),
        paints: paints.clone(),
    });
    let draw = |cx: &mut gpui::VisualTestContext| cx.update(|window, cx| window.draw(cx).clear(cx));
    draw(cx);
    draw(cx);
    assert_eq!((prepaints.get(), paints.get()), (0, 0));
    view.update(cx, |view, cx| {
        view.progress = 0.5;
        cx.notify();
    });
    draw(cx);
    assert!(prepaints.get() > 0 && paints.get() > 0);
    let counts = (prepaints.get(), paints.get());
    view.update(cx, |view, cx| {
        view.progress = 0.;
        cx.notify();
    });
    draw(cx);
    draw(cx);
    assert_eq!((prepaints.get(), paints.get()), counts);
}
