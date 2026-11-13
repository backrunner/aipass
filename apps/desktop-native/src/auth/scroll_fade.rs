//! Repaint the same live background through narrow opacity bands at scroll edges.
//! No GPU readback, stale background screenshot or per-frame raster allocation.
use crate::backdrop::Surface;
use gpui::{canvas, div, point, prelude::*, px, size, Bounds, ContentMask, ScrollHandle};
#[derive(Default)]
pub(super) struct ScrollFade;
impl ScrollFade {
    pub fn render(&self, source: Surface, scroll: ScrollHandle) -> impl IntoElement {
        div()
            .absolute()
            .inset_0()
            .size_full()
            .children((0..32).map(move |index| {
                let source = source.clone();
                let scroll = scroll.clone();
                let top = index < 16;
                let band = index % 16;
                let paint = canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        let offset = -f32::from(scroll.offset().y);
                        let remaining = if top {
                            offset
                        } else {
                            f32::from(scroll.max_offset().y) - offset
                        };
                        if remaining <= 1. {
                            return;
                        }
                        let depth = remaining.min(16.).min(f32::from(bounds.size.height) / 4.);
                        let height = depth / 16.;
                        let y = if top {
                            bounds.origin.y + px(band as f32 * height)
                        } else {
                            bounds.bottom() - px((band + 1) as f32 * height)
                        };
                        let clip = Bounds::new(
                            point(bounds.origin.x, y),
                            size(bounds.size.width, px(height)),
                        );
                        window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
                            if let Some(frame) = source.borrow().as_ref() {
                                frame.paint(bounds, 14., false, window);
                            }
                        });
                    },
                )
                .absolute()
                .inset_0()
                .size_full();
                // Canvas paints its custom callback outside Div's opacity
                // scope. Put opacity on a parent so image primitives inherit it.
                div()
                    .absolute()
                    .inset_0()
                    .size_full()
                    .opacity(1. - (band as f32 + 0.5) / 16.)
                    .child(paint)
            }))
    }
}
