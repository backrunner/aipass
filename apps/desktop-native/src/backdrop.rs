//! Native SVG painting of the original CSS background geometry. No browser or
//! captured reference pixels are used. Animated transforms are still pending.
use gpui::{div, img, prelude::*, App, Asset, Image, ImageFormat, RenderImage, Window};
use std::{future::Future, sync::Arc};

pub struct Backdrop {
    dark: bool,
    hero: bool,
    cached: Option<((u32, u32, u32), Arc<Image>)>,
    readiness: Result<bool, String>,
}

impl Backdrop {
    pub fn new(dark: bool, hero: bool) -> Self {
        Self {
            dark,
            hero,
            cached: None,
            readiness: Ok(false),
        }
    }

    pub fn source(&self) -> Option<Arc<Image>> {
        self.cached.as_ref().map(|(_, image)| image.clone())
    }

    #[cfg(feature = "parity-render")]
    pub fn ready(&self) -> Result<bool, String> {
        self.readiness.clone()
    }

    pub fn render(&mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let size = window.viewport_size();
        let width = f32::from(size.width);
        let height = f32::from(size.height);
        let scale = window.scale_factor();
        let key = (width.to_bits(), height.to_bits(), scale.to_bits());
        let source = if let Some((cached_key, source)) = &self.cached {
            if key == *cached_key {
                Some(source.clone())
            } else {
                None
            }
        } else {
            None
        };
        let source = source.unwrap_or_else(|| {
            let body = if self.hero { hero(width, height, self.dark) } else { workspace(width, height, self.dark) };
            let document = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {width} {height}">{body}</svg>"#, width * scale, height * scale);
            let source = Arc::new(Image::from_bytes(ImageFormat::Svg, document.into_bytes()));
            if let Some((_, old)) = &self.cached { cx.remove_asset::<BackdropAsset>(old); }
            self.cached = Some((key, source.clone()));
            source
        });
        let image = window.use_asset::<BackdropAsset>(&source, cx);
        self.readiness = match &image {
            None => Ok(false),
            Some(Ok(_)) => Ok(true),
            Some(Err(error)) => Err(error.clone()),
        };
        div()
            .absolute()
            .inset_0()
            .size_full()
            .when_some(image.and_then(Result::ok), |el, image| {
                el.child(img(image).size_full())
            })
    }
}

pub(crate) struct BackdropAsset;
impl Asset for BackdropAsset {
    type Source = Arc<Image>;
    type Output = Result<Arc<RenderImage>, String>;
    fn load(
        source: Self::Source,
        cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        let renderer = cx.svg_renderer();
        cx.background_executor().spawn(async move {
            source
                .to_image_data(renderer)
                .map_err(|error| error.to_string())
        })
    }
}

fn workspace(w: f32, h: f32, dark: bool) -> String {
    let (bg, accent) = if dark {
        ("#0a0e1a", "#6092ff")
    } else {
        ("#f5f6fa", "#2563eb")
    };
    let mut gradients = String::new();
    let mut layers = String::new();
    for (i, (rx, ry, x, y, opacity, stop)) in [
        (1000., 420., w * 0.1, h * -0.08, 0.22, 0.6),
        (820., 380., w, h * 1.1, 0.16, 0.6),
        (520., 280., w * 0.6, h * 0.5, 0.06, 0.7),
    ]
    .into_iter()
    .enumerate()
    .rev()
    {
        gradients.push_str(&format!(r#"<radialGradient id="g{i}" gradientUnits="userSpaceOnUse" cx="0" cy="0" r="1" gradientTransform="translate({x} {y}) scale({rx} {ry})"><stop stop-color="{accent}" stop-opacity="{opacity}"/><stop offset="{stop}" stop-color="{accent}" stop-opacity="0"/></radialGradient>"#));
        layers.push_str(&format!(
            r#"<rect width="{w}" height="{h}" fill="url(#g{i})"/>"#
        ));
    }
    let (sidebar, list) = crate::theme::columns(w);
    let mut shadows = String::new();
    // CSS outer box shadows exclude the element's interior, even when its
    // background is transparent. GPUI's unmasked shadow fills that interior.
    gradients.push_str(r#"<filter id="pane-shadow" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="16"/><feOffset dy="12"/></filter>"#);
    for (i, (x, width)) in [
        (8., sidebar),
        (sidebar + 16., list),
        (sidebar + list + 24., w - sidebar - list - 32.),
    ]
    .into_iter()
    .enumerate()
    {
        gradients.push_str(&format!(r#"<mask id="outside{i}" maskUnits="userSpaceOnUse" x="0" y="0" width="{w}" height="{h}"><rect width="{w}" height="{h}" fill="white"/><rect x="{x}" y="8" width="{width}" height="{}" rx="14" fill="black"/></mask>"#, h-16.));
        shadows.push_str(&format!(r##"<g mask="url(#outside{i})"><rect x="{x}" y="8" width="{width}" height="{}" rx="14" fill="#080c18" opacity="0.05" filter="url(#pane-shadow)"/></g>"##, h-16.));
    }
    format!(
        r#"<defs>{gradients}</defs><rect width="{w}" height="{h}" fill="{bg}"/><g opacity="0.75">{layers}</g>{shadows}"#
    )
}

fn hero(w: f32, h: f32, dark: bool) -> String {
    let (bg, surface, accent, hover) = if dark {
        ("#0a0e1a", "#1a2032", "#6092ff", "#76a2ff")
    } else {
        ("#f5f6fa", "#f1f3f8", "#2563eb", "#1f4fd0")
    };
    let vmax = w.max(h) / 100.;
    let mut defs = format!(
        r#"<linearGradient id="base" gradientUnits="userSpaceOnUse" x1="{}" y1="{}" x2="{}" y2="{}"><stop stop-color="{bg}"/><stop offset="1" stop-color="{surface}"/></linearGradient><filter id="blur" x="-100%" y="-100%" width="300%" height="300%" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="80"/></filter>"#,
        (w - h) / 4.,
        (h - w) / 4.,
        (3. * w + h) / 4.,
        (w + 3. * h) / 4.
    );
    let mut layers = String::new();
    for (i, (diameter, x, y, opacity, color)) in [
        (56. * vmax, -10. * vmax, -10. * vmax, 0.35, accent),
        (48. * vmax, w - 36. * vmax, 18. * vmax, 0.28, hover),
        (38. * vmax, 22. * vmax, h - 28. * vmax, 0.22, accent),
    ]
    .into_iter()
    .enumerate()
    {
        defs.push_str(&format!(r#"<radialGradient id="blob{i}"><stop stop-color="{color}"/><stop offset="0.7" stop-color="{color}" stop-opacity="0"/></radialGradient>"#));
        layers.push_str(&format!(r#"<circle cx="{}" cy="{}" r="{}" fill="url(#blob{i})" opacity="{opacity}" filter="url(#blur)"/>"#, x+diameter/2., y+diameter/2., diameter/2.));
    }
    defs.push_str(&format!(r#"<linearGradient id="wave1" x1="0" y1="0" x2="1" y2="0.6"><stop stop-color="{accent}" stop-opacity="0.32"/><stop offset="0.5" stop-color="{accent}" stop-opacity="0.18"/><stop offset="1" stop-color="{accent}" stop-opacity="0.08"/></linearGradient><linearGradient id="wave2" x1="0" y1="0" x2="1" y2="0.7"><stop stop-color="{accent}" stop-opacity="0.18"/><stop offset="1" stop-color="{accent}" stop-opacity="0.32"/></linearGradient>"#));
    format!(
        r#"<defs>{defs}</defs><rect width="{w}" height="{h}" fill="url(#base)"/>{layers}<g transform="translate({} {}) scale({} {})"><path fill="url(#wave1)" d="M0,160 C240,90 360,210 720,170 C1080,130 1200,240 1440,180 L1440,320 L0,320 Z"/><path fill="url(#wave2)" d="M0,210 C240,150 480,260 720,220 C960,180 1200,260 1440,220 L1440,320 L0,320 Z"/></g>"#,
        -0.2 * w,
        h * 0.5,
        w * 1.4 / 1440.,
        h * 0.6 / 320.
    )
}
