//! Solid-background equivalent of the shared 16px scroll mask.
use gpui::{canvas, div, fill, point, prelude::*, px, size, Bounds, Rgba, ScrollHandle};
pub(crate) fn fade(color: Rgba, scroll: ScrollHandle) -> impl IntoElement {
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
                        window.paint_quad(fill(
                            Bounds::new(point(bounds.origin.x, y), size(bounds.size.width, height)),
                            color.opacity(1. - (band as f32 + 0.5) / 16.),
                        ));
                    }
                }
            },
        )
        .size_full(),
    )
}
