use crate::parity::Observe;
use gpui::{div, prelude::*, px, Context, FontWeight, Render, TextRun, Window};

const SAMPLES: [(&str, &str, f32, FontWeight); 6] = [
    (
        "Edit price rules",
        ".AppleSystemUIFont",
        15.,
        FontWeight::SEMIBOLD,
    ),
    (
        "编辑价格规则",
        ".AppleSystemUIFont",
        15.,
        FontWeight::SEMIBOLD,
    ),
    ("Edit rules", ".AppleSystemUIFont", 11., FontWeight::MEDIUM),
    ("••••fixture-a", "Menlo", 11., FontWeight::NORMAL),
    (
        "OpenAI-compatible",
        ".AppleSystemUIFont",
        10.,
        FontWeight::MEDIUM,
    ),
    ("A", ".AppleSystemUIFont", 12.25, FontWeight::NORMAL),
];

struct TextProbe;
impl Render for TextProbe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .items_start()
            .children(
                SAMPLES
                    .iter()
                    .enumerate()
                    .map(|(i, &(text, family, size, weight))| {
                        div()
                            .flex_shrink_0()
                            .font_family(family)
                            .font_weight(weight)
                            .text_size(px(size))
                            .line_height(px(20.))
                            .child(text)
                            .observe(format!("text-{i}"))
                    }),
            )
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .font_family(".AppleSystemUIFont")
                    .text_size(px(12.25))
                    .line_height(px(20.))
                    .children((0..8).map(|_| div().flex_shrink_0().child("A")))
                    .observe("fractional-sequence"),
            )
    }
}

#[gpui::test]
fn intrinsic_text_preserves_fractional_advances_through_layout(cx: &mut gpui::TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| TextProbe);
    for scale in [2., 1., 1.5, 2.] {
        cx.simulate_scale_factor_change(scale);
        cx.update(|w, cx| {
            w.draw(cx).clear(cx);
            assert_eq!(w.scale_factor(), scale);
            for (i, &(text, family, size, weight)) in SAMPLES.iter().enumerate() {
                let mut font = gpui::font(family);
                font.weight = weight;
                let line = w.text_system().shape_line(
                    text.into(),
                    px(size),
                    &[TextRun {
                        len: text.len(),
                        font,
                        color: gpui::black(),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }],
                    None,
                );
                let width = cx.global::<crate::parity::Geometry>().0[&format!("text-{i}")][2];
                let padding = width - f32::from(line.width());
                assert!(
                    padding.abs() < 1. / scale + 0.001,
                    "{text:?}: layout width {width}, shaped width {:?}, padding {padding}",
                    line.width()
                );
                if text == "A" {
                    let actual = cx.global::<crate::parity::Geometry>().0["fractional-sequence"][2];
                    let expected = f32::from(line.width()) * 8. + 6. * 7.;
                    assert!(
                        (actual - expected).abs() < 1. / scale + 0.001,
                        "adjacent labels accumulated rounding: {actual} vs {expected}"
                    );
                }
            }
        });
    }
}

const WRAPPED: &str = "A fractional label and a second clause";
struct WrappingProbe {
    width: f32,
    truncate: bool,
    layout: Option<gpui::TextLayout>,
}
impl Render for WrappingProbe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let text = gpui::StyledText::new(WRAPPED);
        self.layout = Some(text.layout().clone());
        div()
            .w(px(self.width))
            .font_family(".AppleSystemUIFont")
            .text_size(px(12.25))
            .line_height(px(20.))
            .when(self.truncate, |el| el.whitespace_nowrap().text_ellipsis())
            .child(text)
    }
}

#[gpui::test]
fn fractional_intrinsics_preserve_fit_wrapping_and_ellipsis(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, _| WrappingProbe {
        width: 600.,
        truncate: true,
        layout: None,
    });
    cx.update(|w, cx| w.draw(cx).clear(cx));
    let width = view.read_with(cx, |v, _| {
        f32::from(
            v.layout.as_ref().unwrap().line_layouts()[0]
                .unwrapped_layout
                .width,
        )
    });
    for (limit, truncate, has_end, wraps) in [
        ((width * 2.).ceil() / 2., true, true, false),
        (width / 2., true, false, false),
        (width / 2., false, true, true),
        ((width * 2.).ceil() / 2., true, true, false),
    ] {
        view.update(cx, |v, cx| {
            v.width = limit;
            v.truncate = truncate;
            cx.notify();
        });
        cx.update(|w, cx| w.draw(cx).clear(cx));
        view.read_with(cx, |v, _| {
            let layout = v.layout.as_ref().unwrap();
            assert_eq!(layout.position_for_index(WRAPPED.len()).is_some(), has_end);
            assert_eq!(layout.bounds().size.height > px(20.), wraps);
        });
    }
}
