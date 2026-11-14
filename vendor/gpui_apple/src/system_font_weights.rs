//! Map Apple's named system faces to their CSS weights before font matching.
use core_text::font_descriptor::TraitAccessors;
use font_kit::{
    font::Font,
    properties::{Properties, Weight},
};

pub(crate) fn properties(font: &Font, system_family: bool) -> Properties {
    let mut properties = font.properties();
    if system_family {
        // AppKit/UIKit system weight constants. font-kit's generic conversion
        // maps Medium (0.23) to 530, so its CSS 400/500 fallback selects Regular
        // for a 500 request. Heavy and Black also need their distinct classes.
        // Only normalize these named system faces; arbitrary families and
        // intermediate/variable weights retain the generic conversion.
        const NAMED_WEIGHTS: [(f32, f32); 9] = [
            (-0.8, 100.),
            (-0.6, 200.),
            (-0.4, 300.),
            (0., 400.),
            (0.23, 500.),
            (0.3, 600.),
            (0.4, 700.),
            (0.56, 800.),
            (0.62, 900.),
        ];
        let native = font.native_font().all_traits().normalized_weight() as f32;
        if let Some((_, css)) = NAMED_WEIGHTS
            .iter()
            .find(|(weight, _)| (native - weight).abs() < 0.00001)
        {
            properties.weight = Weight(*css);
        }
    }
    properties
}

#[cfg(test)]
mod tests {
    use crate::AppleTextSystem;
    use gpui::{FontRun, FontWeight, PlatformTextSystem, font, px};
    use std::collections::HashSet;

    #[test]
    fn named_system_weights_select_distinct_faces() {
        let fonts = AppleTextSystem::new();
        let ids = (1..=9)
            .map(|weight| {
                let mut face = font(".AppleSystemUIFont");
                face.weight = FontWeight(weight as f32 * 100.);
                fonts.font_id(&face).unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(ids.iter().collect::<HashSet<_>>().len(), 9);
        let text = "Empty trash";
        let width = |id| {
            fonts
                .layout_line(
                    text,
                    px(13.),
                    &[FontRun {
                        font_id: id,
                        len: text.len(),
                    }],
                )
                .width
        };
        assert!(
            width(ids[4]) > width(ids[3]),
            "Medium must not shape as Regular"
        );
    }

    #[test]
    fn other_families_keep_font_kit_properties() {
        let family = font_kit::source::SystemSource::new()
            .select_family_by_name("Helvetica")
            .unwrap();
        for handle in family.fonts() {
            let font = handle.load().unwrap();
            assert_eq!(super::properties(&font, false), font.properties());
        }
    }
}
