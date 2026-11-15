//! Cached vector-derived layers. Animation changes only native paint bounds.
use super::glass::glass;
use gpui::{
    point, px, size, Bounds, Corners, Image, ImageFormat, Pixels, RenderImage, SvgRenderer, Window,
};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct Source {
    pub width: u32,
    pub height: u32,
    pub dark: bool,
    pub hero: bool,
}
impl Source {
    pub fn dimensions(self) -> (f32, f32) {
        (f32::from_bits(self.width), f32::from_bits(self.height))
    }
}
struct Layer {
    image: Arc<RenderImage>,
    glass: Arc<RenderImage>,
    bounds: [f32; 4],
    motion: Option<usize>,
}
pub(super) struct Scene {
    pub source: Source,
    base: Arc<RenderImage>,
    layers: Vec<Layer>,
    shadow: Option<Arc<RenderImage>>,
}
fn raster(
    body: &str,
    w: f32,
    h: f32,
    max_pixels: f32,
    renderer: SvgRenderer,
) -> Result<Arc<RenderImage>, String> {
    let scale = (max_pixels / w.max(h)).min(1.);
    let text = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {w} {h}" preserveAspectRatio="none">{body}</svg>"#,
        (w * scale).ceil(),
        (h * scale).ceil()
    );
    Image::from_bytes(ImageFormat::Svg, text.into_bytes())
        .to_image_data(renderer)
        .map_err(|e| e.to_string())
}

impl Scene {
    pub fn release_atlas(&self, window: &mut Window) {
        for image in std::iter::once(&self.base)
            .chain(
                self.layers
                    .iter()
                    .flat_map(|layer| [&layer.image, &layer.glass]),
            )
            .chain(self.shadow.iter())
        {
            if let Err(error) = window.drop_image(image.clone()) {
                eprintln!("Background texture release failed: {error}");
            }
        }
    }
    pub fn build(source: Source, renderer: SvgRenderer) -> Result<Arc<Self>, String> {
        let (w, h) = source.dimensions();
        let (bg, surface, accent, hover) = if source.dark {
            ("#0a0e1a", "#1a2032", "#6092ff", "#76a2ff")
        } else {
            ("#f5f6fa", "#f1f3f8", "#2563eb", "#1f4fd0")
        };
        let base_svg = if source.hero {
            format!(
                r#"<defs><linearGradient id="base" gradientUnits="userSpaceOnUse" x1="{}" y1="{}" x2="{}" y2="{}"><stop stop-color="{bg}"/><stop offset="1" stop-color="{surface}"/></linearGradient></defs><rect width="{w}" height="{h}" fill="url(#base)"/>"#,
                (w - h) / 4.,
                (h - w) / 4.,
                (3. * w + h) / 4.,
                (w + 3. * h) / 4.
            )
        } else {
            format!(r#"<rect width="{w}" height="{h}" fill="{bg}"/>"#)
        };
        // Opaque viewport edges must not magnify the atlas tile's sampling
        // border. Unlike blurred blobs, this base keeps the window resolution.
        let base = raster(&base_svg, w, h, w.max(h), renderer.clone())?;
        let mut layers = Vec::new();
        if source.hero {
            let v = w.max(h) / 100.;
            for (index, (diameter, x, y, alpha, color)) in [
                (56. * v, -10. * v, -10. * v, 0.35, accent),
                (48. * v, w - 36. * v, 18. * v, 0.28, hover),
                (38. * v, 22. * v, h - 28. * v, 0.22, accent),
            ]
            .into_iter()
            .enumerate()
            {
                let margin = 240.;
                let extent = diameter + 2. * margin;
                let center = extent / 2.;
                let radius = diameter / 2.;
                let body = format!(
                    r#"<defs><radialGradient id="blob"><stop stop-color="{color}"/><stop offset="0.7" stop-color="{color}" stop-opacity="0"/></radialGradient><filter id="blur" x="-100%" y="-100%" width="300%" height="300%" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="80"/></filter></defs><circle cx="{center}" cy="{center}" r="{radius}" fill="url(#blob)" opacity="{alpha}" filter="url(#blur)"/>"#
                );
                let image = raster(&body, extent, extent, 128., renderer.clone())?;
                layers.push(Layer {
                    glass: glass(&image, 20. * image.size(0).width.0 as f32 / extent, 1.4),
                    image,
                    bounds: [x - margin, y - margin, extent, extent],
                    motion: Some(index),
                });
            }
            for (index, (stops, path)) in [
                (format!(r#"<stop stop-color="{accent}" stop-opacity="0.32"/><stop offset="0.5" stop-color="{accent}" stop-opacity="0.18"/><stop offset="1" stop-color="{accent}" stop-opacity="0.08"/>"#), "M0,160 C240,90 360,210 720,170 C1080,130 1200,240 1440,180 L1440,320 L0,320 Z"),
                (format!(r#"<stop stop-color="{accent}" stop-opacity="0.18"/><stop offset="1" stop-color="{accent}" stop-opacity="0.32"/>"#), "M0,210 C240,150 480,260 720,220 C960,180 1200,260 1440,220 L1440,320 L0,320 Z"),
            ].into_iter().enumerate() {
                let body = format!(r#"<defs><linearGradient id="wave" x1="0" y1="0" x2="1" y2="{}">{stops}</linearGradient></defs><path fill="url(#wave)" d="{path}"/>"#, if index == 0 {0.6} else {0.7});
                let (width,height) = (1.4*w,0.6*h);
                let body = format!(r#"<svg width="{width}" height="{height}" viewBox="0 0 1440 320" preserveAspectRatio="none">{body}</svg>"#);
                let image = raster(&body, width, height, width, renderer.clone())?;
                // Glass is already low-frequency. Rasterize its own small vector
                // input instead of filtering millions of Retina edge pixels.
                let frost = raster(&body, width, height, 192., renderer.clone())?;
                layers.push(Layer { glass: glass(&frost, 20.*frost.size(0).width.0 as f32/width, 1.4), image, bounds: [-0.2*w,0.5*h,width,height], motion: Some(index+3) });
            }
        } else {
            for (rx, ry, x, y, alpha, stop) in [
                (1000., 420., w * 0.1, h * -0.08, 0.22 * 0.75, 0.6),
                (820., 380., w, h * 1.1, 0.16 * 0.75, 0.6),
                (520., 280., w * 0.6, h * 0.5, 0.06 * 0.75, 0.7),
            ]
            .into_iter()
            .rev()
            {
                let body = format!(
                    r#"<defs><radialGradient id="radial"><stop stop-color="{accent}" stop-opacity="{alpha}"/><stop offset="{stop}" stop-color="{accent}" stop-opacity="0"/></radialGradient></defs><rect width="{}" height="{}" fill="url(#radial)"/>"#,
                    2. * rx,
                    2. * ry
                );
                let image = raster(&body, 2. * rx, 2. * ry, 384., renderer.clone())?;
                layers.push(Layer {
                    glass: glass(&image, 8. * image.size(0).width.0 as f32 / (2. * rx), 1.),
                    image,
                    bounds: [x - rx, y - ry, 2. * rx, 2. * ry],
                    motion: None,
                });
            }
        }
        let shadow = if source.hero {
            None
        } else {
            Some(raster(&super::shadows(w, h), w, h, 512., renderer)?)
        };
        Ok(Arc::new(Self {
            source,
            base,
            layers,
            shadow,
        }))
    }
    pub fn paint(
        &self,
        frame: &super::Frame,
        clip: Bounds<Pixels>,
        radius: f32,
        frosted: bool,
        window: &mut Window,
    ) {
        let (viewport, elapsed, reduced) = (frame.viewport, frame.elapsed, frame.reduced);
        let corners = Corners::all(px(radius));
        let paint = |image: Arc<RenderImage>, bounds, window: &mut Window| {
            if let Err(error) = window.paint_image(clip, bounds, corners, image, 0, false) {
                eprintln!("Background paint failed: {error}");
            }
        };
        paint(self.base.clone(), viewport, window);
        let (w, h) = self.source.dimensions();
        let sx = f32::from(viewport.size.width) / w;
        let sy = f32::from(viewport.size.height) / h;
        for layer in &self.layers {
            let [mut x, mut y, mut width, mut height] = layer.bounds;
            if let Some(index) = layer.motion {
                let (dx, dy, scale) = super::movement(index, elapsed, reduced, w, h);
                x += dx - width * (scale - 1.) / 2.;
                y += dy - height * (scale - 1.) / 2.;
                width *= scale;
                height *= scale;
            }
            let bounds = Bounds::new(
                viewport.origin + point(px(x * sx), px(y * sy)),
                size(px(width * sx), px(height * sy)),
            );
            paint(
                if frosted {
                    layer.glass.clone()
                } else {
                    layer.image.clone()
                },
                bounds,
                window,
            );
        }
        if !frosted {
            if let Some(shadow) = &self.shadow {
                paint(shadow.clone(), viewport, window);
            }
        }
    }
}
