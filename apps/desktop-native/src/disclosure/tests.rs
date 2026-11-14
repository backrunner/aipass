use super::*;
use crate::parity::Observe;
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

struct FractionalStack;
impl Render for FractionalStack {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .children((0..10).map(|i| {
                Frame {
                    height: 1.,
                    fade: 1.,
                    open: true,
                }
                .body(
                    format!("fractional-reveal-{i}"),
                    div()
                        .flex()
                        .flex_col()
                        .text_size(px(12.))
                        .line_height(px(16.8))
                        .children((0..3).map(|_| div().flex_shrink_0().child("A line"))),
                )
            }))
            .child(div().h(px(1.)).observe("fractional-reveal-end"))
    }
}

#[gpui::test]
fn stacked_disclosures_do_not_accumulate_rounded_line_heights(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| FractionalStack);
    cx.simulate_resize(gpui::size(px(960.), px(640.)));
    for scale in [1., 2., 1.5] {
        cx.simulate_scale_factor_change(scale);
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }
        cx.update(|_, cx| {
            let end = cx.global::<crate::parity::Geometry>().0["fractional-reveal-end"][1];
            assert!(
                (end - 504.).abs() <= 0.5 / scale,
                "natural height drifted at scale {scale}: {end}"
            );
        });
    }
}
