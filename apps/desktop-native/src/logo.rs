//! Resize the original brand artwork to physical pixels before uploading it.
//! The GPU's bilinear minification alone drops fine details at 16/28 logical px.
use gpui::{img, prelude::*, px, Window};
use image::{imageops::FilterType, DynamicImage, ImageFormat};
use std::io::Cursor;

const ORIGINAL: &[u8] = include_bytes!("../../desktop/public/aipass-logo.png");

pub fn logo(size: f32, window: &Window) -> impl IntoElement {
    let pixels = (size * window.scale_factor()).ceil().max(1.) as u32;
    img(format!("aipass-logo/{pixels}"))
        .size(px(size))
        .flex_shrink_0()
        .rounded(px(size * 0.22))
}

pub fn asset(path: &str) -> anyhow::Result<Option<Vec<u8>>> {
    let Some(pixels) = path.strip_prefix("aipass-logo/") else {
        return Ok(None);
    };
    let pixels: u32 = pixels.parse()?;
    anyhow::ensure!((1..=1024).contains(&pixels), "Invalid logo dimensions");
    let mut source = image::load_from_memory_with_format(ORIGINAL, ImageFormat::Png)?.to_rgba32f();
    // Filtering transparent pixels in straight alpha introduces dark fringes.
    // Resample associated alpha, then restore straight RGBA for the PNG decoder.
    for pixel in source.pixels_mut() {
        for channel in 0..3 {
            pixel[channel] *= pixel[3];
        }
    }
    let mut resized = image::imageops::resize(&source, pixels, pixels, FilterType::Lanczos3);
    for pixel in resized.pixels_mut() {
        let alpha = pixel[3].clamp(0., 1.);
        for channel in 0..3 {
            pixel[channel] = if alpha > 0. {
                (pixel[channel] / alpha).clamp(0., 1.)
            } else {
                0.
            };
        }
        pixel[3] = alpha;
    }
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba32F(resized)
        .to_rgba8()
        .write_to(&mut bytes, ImageFormat::Png)?;
    Ok(Some(bytes.into_inner()))
}
