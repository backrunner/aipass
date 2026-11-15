//! Solid-background equivalent of the shared 16px scroll mask.
use gpui::{canvas, div, fill, point, prelude::*, px, size, Bounds, Rgba, ScrollHandle};
pub(crate) fn fade(color: Rgba, scroll: ScrollHandle) -> impl IntoElement {
    rounded_bottom(color, scroll, 0.)
}
/// Fade long selectable values toward their parent surface without intercepting input.
pub(crate) fn horizontal(color: Rgba, scroll: ScrollHandle) -> impl IntoElement {
    div().absolute().inset_0().size_full().child(
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let offset = -f32::from(scroll.offset().x);
                for left in [true, false] {
                    let remaining = if left {
                        offset
                    } else {
                        f32::from(scroll.max_offset().x) - offset
                    };
                    if remaining <= 1. {
                        continue;
                    }
                    let depth = remaining.min(16.).min(f32::from(bounds.size.width) / 4.);
                    for band in 0..16 {
                        let width = px(depth / 16.);
                        let x = if left {
                            bounds.origin.x + width * band as f32
                        } else {
                            bounds.right() - width * (band + 1) as f32
                        };
                        window.paint_quad(fill(
                            Bounds::new(point(x, bounds.origin.y), size(width, bounds.size.height)),
                            color.opacity(1. - (band as f32 + 0.5) / 16.),
                        ));
                    }
                }
            },
        )
        .size_full(),
    )
}
/// Keep a drawer's fading edge inside its rounded bottom corners.
pub(crate) fn rounded_bottom(color: Rgba, scroll: ScrollHandle, radius: f32) -> impl IntoElement {
    div().absolute().inset_0().size_full().child(
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let offset = -f32::from(scroll.offset().y);
                for top in [true, false] {
                    let remaining = if top {
                        offset
                    } else {
                        f32::from(scroll.max_offset().y) - offset
                    };
                    if remaining <= 1. {
                        continue;
                    }
                    let depth = remaining.min(16.).min(f32::from(bounds.size.height) / 4.);
                    for band in 0..16 {
                        let height = px(depth / 16.);
                        let y = if top {
                            bounds.origin.y + height * band as f32
                        } else {
                            bounds.bottom() - height * (band + 1) as f32
                        };
                        let band_bounds =
                            Bounds::new(point(bounds.origin.x, y), size(bounds.size.width, height));
                        let color = color.opacity(1. - (band as f32 + 0.5) / 16.);
                        if radius > 0. {
                            let mut quad = fill(bounds, color);
                            quad.corner_radii.bottom_left = px(radius);
                            quad.corner_radii.bottom_right = px(radius);
                            window.with_content_mask(
                                Some(gpui::ContentMask {
                                    bounds: band_bounds,
                                }),
                                |w| w.paint_quad(quad),
                            );
                        } else {
                            window.paint_quad(fill(band_bounds, color));
                        }
                    }
                }
            },
        )
        .size_full(),
    )
}
