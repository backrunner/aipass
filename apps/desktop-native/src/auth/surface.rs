//! CSS outer shadows exclude the rounded card interior. Render that material
//! from native vector geometry; no reference screenshots are used by the app.
use gpui::{canvas, prelude::*, px, Corners, Image, ImageFormat, RenderImage};
use std::{cell::RefCell, rc::Rc, sync::Arc};

#[derive(Default)]
struct Cache {
    image: Option<((u32, u32, u32), Arc<RenderImage>)>,
    ready: bool,
}

#[derive(Default)]
pub(super) struct CardShadow(Rc<RefCell<Cache>>);

impl CardShadow {
    #[cfg(feature = "parity-render")]
    pub fn ready(&self) -> bool {
        self.0.borrow().ready
    }

    pub fn render(&self) -> impl IntoElement {
        let cache = self.0.clone();
        canvas(
            move |bounds, window, cx| {
                crate::parity::record_bounds("auth-shadow", bounds, cx);
                let width = f32::from(bounds.size.width);
                let height = f32::from(bounds.size.height);
                let scale = window.scale_factor();
                let key = (width.to_bits(), height.to_bits(), scale.to_bits());
                let margin = 120.;
                let w = width + 2. * margin;
                let h = height + 2. * margin;
                let mut cache = cache.borrow_mut();
                if cache.image.as_ref().is_none_or(|(old, _)| *old != key) {
                    // SVG Gaussian blur uses sigma = CSS blur radius / 2.
                    let svg = format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {w} {h}"><defs><filter id="blur" x="-100%" y="-100%" width="300%" height="300%"><feGaussianBlur stdDeviation="30"/><feOffset dy="20"/></filter><mask id="outside" maskUnits="userSpaceOnUse" x="0" y="0" width="{w}" height="{h}"><rect width="{w}" height="{h}" fill="white"/><rect x="{margin}" y="{margin}" width="{width}" height="{height}" rx="14" fill="black"/></mask></defs><g mask="url(#outside)"><rect x="{margin}" y="{margin}" width="{width}" height="{height}" rx="14" fill="#0f1110" opacity="0.25" filter="url(#blur)"/></g></svg>"##, w * 0.5, h * 0.5);
                    match Image::from_bytes(ImageFormat::Svg, svg.into_bytes()).to_image_data(cx.svg_renderer()) {
                        Ok(image) => {
                            if let Some((_, previous)) = cache.image.replace((key, image)) {
                                if let Err(error) = window.drop_image(previous) {
                                    eprintln!("Card shadow texture release failed: {error}");
                                }
                            }
                        }
                        Err(error) => eprintln!("Card shadow preparation failed: {error}"),
                    }
                }
                cache.ready = cache.image.is_some();
                (bounds.dilate(px(margin)), cache.image.as_ref().map(|(_, image)| image.clone()))
            },
            |_, (bounds, image), window, _| {
                if let Some(image) = image {
                    if let Err(error) = window.paint_image(bounds, bounds, Corners::default(), image, 0, false) {
                        eprintln!("Card shadow rendering failed: {error}");
                    }
                }
            },
        )
        .absolute()
        .inset_0()
        .size_full()
    }
}
