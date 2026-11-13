use super::super::*;
use gpui::{point, px, App, EmptyView, InputHandler, PlatformInputHandler, Point, UTF16Selection};
use std::{cell::Cell, ops::Range, rc::Rc};

struct TextProbe(Rc<Cell<Point<Pixels>>>);
fn caret() -> Bounds<Pixels> {
    Bounds::from_corners(point(px(10.), px(12.)), point(px(12.), px(32.)))
}
impl InputHandler for TextProbe {
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut App,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 1..1,
            reversed: false,
        })
    }
    fn marked_text_range(&mut self, _: &mut Window, _: &mut App) -> Option<Range<usize>> {
        None
    }
    fn text_for_range(
        &mut self,
        _: Range<usize>,
        _: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut App,
    ) -> Option<String> {
        Some("字".into())
    }
    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        _: &str,
        _: &mut Window,
        _: &mut App,
    ) {
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        _: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        _: &mut App,
    ) {
    }
    fn unmark_text(&mut self, _: &mut Window, _: &mut App) {}
    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: &mut Window,
        _: &mut App,
    ) -> Option<Bounds<Pixels>> {
        Some(caret())
    }
    fn selection_bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: &mut Window,
        _: &mut App,
    ) -> Vec<Bounds<Pixels>> {
        vec![caret()]
    }
    fn element_bounds(&mut self, _: &mut Window, _: &mut App) -> Option<Bounds<Pixels>> {
        Some(caret())
    }
    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut App,
    ) -> Option<usize> {
        self.0.set(point);
        Some(1)
    }
}

#[gpui::test]
fn input_transform_preserves_native_ime_and_selection_coordinates(cx: &mut gpui::TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| EmptyView);
    let observed = Rc::new(Cell::new(Point::default()));
    let transform = ElementTransform::scale_around(point(px(100.), px(50.)), 0.5);
    let mut handler = cx.update(|window, cx| {
        PlatformInputHandler::new(window.to_async(cx), Box::new(TextProbe(observed.clone())))
            .with_transform(transform)
    });
    let expected = Bounds::from_corners(point(px(55.), px(31.)), point(px(56.), px(41.)));
    assert_eq!(handler.bounds_for_range(1..1), Some(expected));
    assert_eq!(handler.ime_candidate_bounds(), Some(expected));
    assert_eq!(handler.element_bounds(), Some(expected));
    assert_eq!(handler.selection_bounds_for_range(0..1), [expected]);
    assert_eq!(
        handler.character_index_for_point(point(px(55.), px(31.))),
        Some(1)
    );
    assert_eq!(observed.get(), point(px(10.), px(12.)));
    assert_eq!(handler.selected_text_range(true).unwrap().range, 1..1);
    assert_eq!(
        cx.update(|w, cx| handler.selected_bounds(w, cx)),
        Some(expected)
    );
}
