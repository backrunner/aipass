//! Match the reference's `-webkit-font-smoothing: antialiased` without changing
//! the user's persisted defaults. GPUI reads this process-local Foundation
//! setting before caching its glyph dilation policy.
use gpui::{px, FontWeight, Pixels, Window};

/// CSS normal line boxes include fallback font metrics for mixed-script text.
pub fn normal_line_height(window: &Window, text: &str, font_size: f32) -> Pixels {
    let line = window.text_system().shape_line(
        text.to_owned().into(),
        px(font_size),
        &[gpui::TextRun {
            len: text.len(),
            font: window.text_style().font(),
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    );
    line.ascent.round() + line.descent.abs().round()
}

/// CSS inline decorations surround the font's ascent/descent box, while their
/// parent positions that box on its own line's baseline. Centering a flex pill
/// instead changes its height and vertical position, especially at small sizes.
pub fn inline_badge_metrics(
    window: &Window,
    font_size: f32,
    parent_font_size: f32,
    parent_line_height: f32,
    edge: f32,
) -> (Pixels, Pixels) {
    let text = window.text_system();
    let mut font = window.text_style().font();
    font.weight = FontWeight::NORMAL;
    let id = text.resolve_font(&font);
    let ascent = text.ascent(id, px(font_size));
    // Font-kit/OpenType descent is signed below the baseline.
    let height = ascent + text.descent(id, px(font_size)).abs();
    let parent_ascent = text.ascent(id, px(parent_font_size));
    let parent_height = parent_ascent + text.descent(id, px(parent_font_size)).abs();
    let baseline = (px(parent_line_height) - parent_height) / 2. + parent_ascent;
    (height, baseline - ascent - px(edge))
}

pub fn init() {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{
            ns_string, NSArgumentDomain, NSMutableCopying, NSNumber, NSUserDefaults,
        };
        let defaults = NSUserDefaults::standardUserDefaults();
        // SAFETY: Foundation supplies this NSString constant. The existing
        // domain is copied so unrelated launch arguments remain intact; values
        // are valid Foundation property-list objects. Volatile domains are
        // discarded at process exit and never synchronized to persistent state.
        unsafe {
            let domain = defaults
                .volatileDomainForName(NSArgumentDomain)
                .mutableCopy();
            domain.insert(
                ns_string!("AppleFontSmoothing"),
                &NSNumber::numberWithInt(0),
            );
            defaults.setVolatileDomain_forName(&domain, NSArgumentDomain);
        }
    }
}

#[cfg(all(test, feature = "parity-render"))]
mod tests;
