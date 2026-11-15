//! Filter cached background textures in premultiplied space, then restore GPUI's
//! straight BGRA representation. Foreground text is never part of these layers.
use gpui::RenderImage;
use std::sync::Arc;

pub(super) fn glass(image: &Arc<RenderImage>, sigma: f32, saturation: f32) -> Arc<RenderImage> {
    let extent = image.size(0);
    let mut data = image::RgbaImage::from_raw(
        extent.width.0 as u32,
        extent.height.0 as u32,
        image
            .as_bytes(0)
            .expect("cached background pixels")
            .to_vec(),
    )
    .expect("background dimensions");
    premultiply(&mut data);
    let small = image::imageops::resize(
        &data,
        data.width().div_ceil(4),
        data.height().div_ceil(4),
        image::imageops::FilterType::Triangle,
    );
    let mut data = image::imageops::fast_blur(&small, sigma / 4.);
    for p in data.pixels_mut() {
        let alpha = p[3] as f32 / 255.;
        if alpha == 0. {
            p.0 = [0; 4];
            continue;
        }
        let luma = (0.0722 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.2126 * p[2] as f32) / alpha;
        for v in &mut p.0[..3] {
            *v = (luma + (*v as f32 / alpha - luma) * saturation)
                .clamp(0., 255.)
                .round() as u8;
        }
    }
    Arc::new(RenderImage::new([image::Frame::new(data)]))
}
fn premultiply(data: &mut image::RgbaImage) {
    for p in data.pixels_mut() {
        let alpha = p[3] as f32 / 255.;
        for v in &mut p.0[..3] {
            *v = (*v as f32 * alpha).round() as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invisible_color_cannot_bleed_into_frosted_edges() {
        let make = |transparent: [u8; 4]| {
            let mut pixels = image::RgbaImage::from_pixel(64, 32, image::Rgba(transparent));
            for y in 0..32 {
                for x in 0..32 {
                    pixels.put_pixel(x, y, image::Rgba([255, 64, 0, 255]));
                }
            }
            Arc::new(RenderImage::new([image::Frame::new(pixels)]))
        };
        let black = glass(&make([0, 0, 0, 0]), 8., 1.);
        let white = glass(&make([255, 255, 255, 0]), 8., 1.);
        assert_eq!(black.as_bytes(0), white.as_bytes(0));
        assert!(black
            .as_bytes(0)
            .unwrap()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 16 && p[3] < 240)
            .all(|p| p[0] > 248 && p[2] == 0));
    }
}
