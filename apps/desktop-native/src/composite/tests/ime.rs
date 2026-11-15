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
    for scale_y in [0.5, 0.8] {
        let transform =
            ElementTransform::scale_axes_around(point(px(100.), px(50.)), point(0.5, scale_y));
        let mut handler = cx.update(|window, cx| {
            PlatformInputHandler::new(window.to_async(cx), Box::new(TextProbe(observed.clone())))
                .with_transform(transform)
        });
        let expected = Bounds::from_corners(
            point(px(55.), px(50. - 38. * scale_y)),
            point(px(56.), px(50. - 18. * scale_y)),
        );
        let near = |actual: Bounds<Pixels>| {
            for (actual, expected) in [
                actual.origin.x,
                actual.origin.y,
                actual.size.width,
                actual.size.height,
            ]
            .into_iter()
            .zip([
                expected.origin.x,
                expected.origin.y,
                expected.size.width,
                expected.size.height,
            ]) {
                assert!((f32::from(actual) - f32::from(expected)).abs() < 0.0001);
            }
        };
        near(handler.bounds_for_range(1..1).unwrap());
        near(handler.ime_candidate_bounds().unwrap());
        near(handler.element_bounds().unwrap());
        let selection = handler.selection_bounds_for_range(0..1);
        assert_eq!(selection.len(), 1);
        near(selection[0]);
        assert_eq!(handler.character_index_for_point(expected.origin), Some(1));
        assert!(
            (f32::from(observed.get().x) - 10.).abs() < 0.0001
                && (f32::from(observed.get().y) - 12.).abs() < 0.0001
        );
        assert_eq!(handler.selected_text_range(true).unwrap().range, 1..1);
        near(cx.update(|w, cx| handler.selected_bounds(w, cx)).unwrap());
    }
}
