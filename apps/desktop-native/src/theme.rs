use gpui::{rgb, rgba, Rgba};

/// Values from packages/ui/src/styles/base.scss at the migration baseline.
#[derive(Clone, Copy)]
pub struct Theme {
    pub bg: Rgba,
    pub surface: Rgba,
    pub surface2: Rgba,
    pub sidebar: Rgba,
    pub text: Rgba,
    pub secondary: Rgba,
    pub tertiary: Rgba,
    pub border: Rgba,
    pub border_strong: Rgba,
    pub divider: Rgba,
    pub accent: Rgba,
    pub accent_soft: Rgba,
    pub focus_ring: Rgba,
    pub popup_shadow: Rgba,
    pub danger: Rgba,
    pub danger_soft: Rgba,
    pub warning: Rgba,
    pub success: Rgba,
}

impl Theme {
    /// CSS outline with an offset, for controls on the standard surface.
    pub fn outline(self, color: Rgba, width: f32, offset: f32) -> Vec<gpui::BoxShadow> {
        let mut shadows = vec![gpui_base::box_shadow(
            0.,
            0.,
            0.,
            width + offset,
            color.into(),
        )];
        if offset > 0. {
            shadows.push(gpui_base::box_shadow(
                0.,
                0.,
                0.,
                offset,
                self.surface.into(),
            ));
        }
        shadows
    }
    pub fn input_style(self) -> gpui_base::input::InputEditorStyle {
        gpui_base::input::InputEditorStyle {
            foreground: self.text.into(),
            muted_foreground: self.tertiary.into(),
            background: self.surface.into(),
            border: self.border.into(),
            selection: self.accent.opacity(0.3).into(),
            caret: self.text.into(),
            ..Default::default()
        }
    }

    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                bg: rgb(0x0a0e1a),
                surface: rgb(0x131826),
                surface2: rgb(0x1a2032),
                sidebar: rgb(0x0f1422),
                text: rgb(0xf4f6fc),
                secondary: rgb(0xc8cee0),
                tertiary: rgb(0x969eb6),
                border: rgb(0x232b40),
                border_strong: rgb(0x354160),
                divider: rgb(0x1c2336),
                accent: rgb(0x6092ff),
                accent_soft: rgba(0x6092ff29),
                focus_ring: rgb(0x6092ff).opacity(0.36),
                popup_shadow: rgb(0x000000).opacity(0.36),
                danger: rgb(0xf1a6a0),
                danger_soft: rgb(0xf1a6a0).opacity(0.14),
                warning: rgb(0xe2b070),
                success: rgb(0x8ad8be),
            }
        } else {
            Self {
                bg: rgb(0xf5f6fa),
                surface: rgb(0xffffff),
                surface2: rgb(0xf1f3f8),
                sidebar: rgb(0xf8f9fc),
                text: rgb(0x08101f),
                secondary: rgb(0x383f55),
                tertiary: rgb(0x636b82),
                border: rgb(0xe1e4ed),
                border_strong: rgb(0xc4cad8),
                divider: rgb(0xedeff5),
                accent: rgb(0x2563eb),
                accent_soft: rgba(0x2563eb1a),
                focus_ring: rgb(0x2563eb).opacity(0.28),
                popup_shadow: rgb(0x0f1110).opacity(0.08),
                danger: rgb(0xb42318),
                danger_soft: rgb(0xb42318).opacity(0.08),
                warning: rgb(0xb25e09),
                success: rgb(0x18794e),
            }
        }
    }
}

pub fn columns(width: f32) -> (f32, f32) {
    if width <= 1100.0 {
        (208.0, 332.0)
    } else {
        (232.0, 368.0)
    }
}

/// Opaque CSS color-mix(in oklab, first weight%, second). Mixing cube-root LMS
/// is equivalent to mixing Oklab because the remaining Lab matrix is linear.
pub(crate) fn oklab_mix(first: Rgba, second: Rgba, weight: f64) -> Rgba {
    fn roots(c: Rgba) -> [f64; 3] {
        let linear = |v: f32| {
            let v = f64::from(v);
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        let (r, g, b) = (linear(c.r), linear(c.g), linear(c.b));
        [
            (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt(),
            (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt(),
            (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt(),
        ]
    }
    let first = roots(first);
    let second = roots(second);
    let [l, m, s] =
        std::array::from_fn(|i| (first[i] * weight + second[i] * (1. - weight)).powi(3));
    let gamma = |v: f64| {
        if v <= 0.0031308 {
            (12.92 * v) as f32
        } else {
            (1.055 * v.powf(1. / 2.4) - 0.055) as f32
        }
    };
    Rgba {
        r: gamma(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
        g: gamma(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
        b: gamma(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s),
        a: 1.,
    }
}

/// CSS box-shadow blur is twice the Gaussian sigma consumed by GPUI's shader.
pub fn shadow_blur(css_radius: f32) -> gpui::Pixels {
    gpui::px(css_radius / 2.)
}
