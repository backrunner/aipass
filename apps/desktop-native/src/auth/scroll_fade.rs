//! The auth backdrop is opaque. Blending it over the scroll edges with 1-mask
//! opacity is equivalent to masking the card over that backdrop. The pixels
//! come from the app's own vector background, never a screenshot/reference.
use crate::backdrop::BackdropAsset;
use gpui::{
    canvas, point, prelude::*, px, size, Bounds, Corners, Image, Pixels, RenderImage, ScrollHandle,
};
use image::{Frame, RgbaImage};
use std::{cell::RefCell, rc::Rc, sync::Arc};

type Strip = (Bounds<Pixels>, Arc<RenderImage>);
type Key = (gpui::ImageId, [u32; 7]);
type Cache = Option<(Key, Vec<Strip>)>;

#[derive(Default)]
pub(super) struct ScrollFade(Rc<RefCell<Cache>>);

fn depth(remaining: f32, height: f32) -> f32 {
    if remaining > 1. {
        remaining.min(16.).min(height / 4.)
    } else {
        0.
    }
}

impl ScrollFade {
    pub fn render(&self, source: Option<Arc<Image>>, scroll: ScrollHandle) -> impl IntoElement {
        let cache = self.0.clone();
        canvas(
            move |bounds, window, cx| {
                let Some(source) = &source else {
                    return Vec::new();
                };
                let Some(Ok(image)) = window.use_asset::<BackdropAsset>(source, cx) else {
                    return Vec::new();
                };
                let height = f32::from(bounds.size.height);
                let offset = -f32::from(scroll.offset().y);
                let maximum = f32::from(scroll.max_offset().y);
                let top = depth(offset, height);
                let bottom = depth(maximum - offset, height);
                let scale = window.scale_factor();
                let key = (
                    image.id,
                    [
                        f32::from(bounds.origin.x).to_bits(),
                        f32::from(bounds.origin.y).to_bits(),
                        f32::from(bounds.size.width).to_bits(),
                        height.to_bits(),
                        scale.to_bits(),
                        top.to_bits(),
                        bottom.to_bits(),
                    ],
                );
                let mut cache = cache.borrow_mut();
                if cache.as_ref().is_none_or(|(prior, _)| *prior != key) {
                    let mut strips = Vec::new();
                    let image_width = image.size(0).width.0.max(0) as u32;
                    let image_height = image.size(0).height.0.max(0) as u32;
                    let data = image.as_bytes(0).expect("loaded background frame");
                    for (fade, is_top) in [(top, true), (bottom, false)] {
                        if fade <= 0. {
                            continue;
                        }
                        let left = (f32::from(bounds.origin.x) * scale).round().max(0.) as u32;
                        let right = (f32::from(bounds.right()) * scale).round().max(0.) as u32;
                        let y = if is_top {
                            f32::from(bounds.origin.y)
                        } else {
                            f32::from(bounds.bottom()) - fade
                        };
                        let first = (y * scale).round().max(0.) as u32;
                        let last = ((y + fade) * scale).round().max(0.) as u32;
                        let width = right.min(image_width).saturating_sub(left);
                        let rows = last.min(image_height).saturating_sub(first);
                        if width == 0 || rows == 0 {
                            continue;
                        }
                        let mut output = RgbaImage::new(width, rows);
                        for row in 0..rows {
                            let progress = (row as f32 + 0.5) / rows as f32;
                            let alpha = if is_top { 1. - progress } else { progress };
                            for column in 0..width {
                                let index =
                                    (((first + row) * image_width + left + column) * 4) as usize;
                                // RenderImage bytes are BGRA; retain their channel order.
                                output.put_pixel(
                                    column,
                                    row,
                                    image::Rgba([
                                        data[index],
                                        data[index + 1],
                                        data[index + 2],
                                        (data[index + 3] as f32 * alpha).round() as u8,
                                    ]),
                                );
                            }
                        }
                        strips.push((
                            Bounds::new(
                                point(px(left as f32 / scale), px(first as f32 / scale)),
                                size(px(width as f32 / scale), px(rows as f32 / scale)),
                            ),
                            Arc::new(RenderImage::new([Frame::new(output)])),
                        ));
                    }
                    *cache = Some((key, strips));
                }
                cache.as_ref().expect("initialized fade").1.clone()
            },
            |_, strips, window, _| {
                for (bounds, image) in strips {
                    if let Err(error) =
                        window.paint_image(bounds, bounds, Corners::default(), image, 0, false)
                    {
                        eprintln!("Auth scroll fade failed: {error}");
                    }
                }
            },
        )
        .absolute()
        .inset_0()
        .size_full()
    }
}
