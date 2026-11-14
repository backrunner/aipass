use super::*;
use gpui::{Entity, Point, StyleRefinement};

struct Child {
    clicks: Rc<Cell<usize>>,
    renders: Rc<Cell<usize>>,
}
impl Render for Child {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders.set(self.renders.get() + 1);
        let clicks = self.clicks.clone();
        div().w(px(200.)).h(px(100.)).child(
            div()
                .id("cached-button")
                .w(px(20.))
                .h(px(20.))
                .on_mouse_down(MouseButton::Left, move |_, _, _| {
                    clicks.set(clicks.get() + 1)
                }),
        )
    }
}
struct Cached {
    factor: f32,
    child: Entity<Child>,
}
impl Render for Cached {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Scale::new(
            self.child
                .clone()
                .cached(StyleRefinement::default().w(px(200.)).h(px(100.))),
            self.factor,
        )
    }
}

#[gpui::test]
fn cached_subtree_rebuilds_hit_regions_when_only_its_parent_scale_changes(
    cx: &mut gpui::TestAppContext,
) {
    let clicks = Rc::new(Cell::new(0));
    let renders = Rc::new(Cell::new(0));
    let (view, cx) = cx.add_window_view(|_, cx| Cached {
        factor: 0.5,
        child: cx.new(|_| Child {
            clicks: clicks.clone(),
            renders: renders.clone(),
        }),
    });
    cx.update(|w, cx| w.draw(cx).clear(cx));
    cx.simulate_click(point(px(55.), px(30.)), Default::default());
    assert_eq!(clicks.get(), 1);
    let before = renders.get();
    view.update(cx, |v, cx| {
        v.factor = 0.8;
        cx.notify();
    });
    cx.update(|w, cx| w.draw(cx).clear(cx));
    assert!(renders.get() > before);
    cx.simulate_click(point(px(28.), px(18.)), Default::default());
    cx.simulate_click(point(px(55.), px(30.)), Default::default());
    assert_eq!(clicks.get(), 2);
}

struct Nested(Rc<Cell<Point<Pixels>>>);
impl Render for Nested {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let point = self.0.clone();
        Scale::new(
            Scale::new(
                div().w(px(200.)).h(px(100.)).child(
                    div()
                        .id("nested-button")
                        .w(px(20.))
                        .h(px(20.))
                        .on_mouse_down(MouseButton::Left, move |event, w, _| {
                            assert_eq!(event.position, w.mouse_position());
                            point.set(event.position);
                        }),
                ),
                0.5,
            ),
            0.8,
        )
    }
}

#[gpui::test]
fn nested_compositors_map_pointer_coordinates_once_per_ancestor(cx: &mut gpui::TestAppContext) {
    let position = Rc::new(Cell::new(point(px(-1.), px(-1.))));
    let (_, cx) = cx.add_window_view(|_, _| Nested(position.clone()));
    cx.update(|w, cx| w.draw(cx).clear(cx));
    cx.simulate_click(point(px(64.), px(34.)), Default::default());
    assert!((position.get().x - px(10.)).abs() < px(0.001));
    assert!((position.get().y - px(10.)).abs() < px(0.001));
}

struct Clip(Rc<Cell<usize>>);
impl Render for Clip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let clicks = self.0.clone();
        Scale::new(
            div().w(px(200.)).h(px(100.)).child(
                div()
                    .absolute()
                    .left(px(10.))
                    .top(px(10.))
                    .w(px(20.))
                    .h(px(20.))
                    .overflow_hidden()
                    .child(
                        div()
                            .id("clipped-button")
                            .w(px(40.))
                            .h(px(40.))
                            .flex_shrink_0()
                            .on_mouse_down(MouseButton::Left, move |_, _, _| {
                                clicks.set(clicks.get() + 1)
                            }),
                    ),
            ),
            0.5,
        )
    }
}

#[gpui::test]
fn clipped_part_of_a_scaled_child_cannot_receive_clicks(cx: &mut gpui::TestAppContext) {
    let clicks = Rc::new(Cell::new(0));
    let (_, cx) = cx.add_window_view(|_, _| Clip(clicks.clone()));
    cx.update(|w, cx| w.draw(cx).clear(cx));
    cx.simulate_click(point(px(60.), px(35.)), Default::default());
    assert_eq!(clicks.get(), 1);
    cx.simulate_click(point(px(70.), px(35.)), Default::default());
    assert_eq!(clicks.get(), 1);
}

struct Portal(Rc<Cell<Option<Bounds<Pixels>>>>);
impl Render for Portal {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let observed = self.0.clone();
        Scale::new(
            div().w(px(200.)).h(px(100.)).child(
                gpui_base::Popup::new("scaled-popup", div().w(px(20.)).h(px(20.)))
                    .w(px(20.))
                    .h(px(20.))
                    .content(div().w(px(40.)).h(px(20.)))
                    .on_position(move |_, trigger| observed.set(Some(trigger))),
            ),
            0.5,
        )
    }
}

#[gpui::test]
fn portal_popups_anchor_to_the_scaled_trigger(cx: &mut gpui::TestAppContext) {
    let bounds = Rc::new(Cell::new(None));
    let (_, cx) = cx.add_window_view(|_, _| Portal(bounds.clone()));
    cx.update(|w, cx| {
        w.draw(cx).clear(cx);
        w.draw(cx).clear(cx);
    });
    assert_eq!(
        bounds.get(),
        Some(Bounds::from_corners(
            point(px(50.), px(25.)),
            point(px(60.), px(35.))
        ))
    );
}
