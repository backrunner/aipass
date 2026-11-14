//! Native geometry for UnlockTransition.svelte. No reference pixels or WebView.
use crate::theme::{oklab_mix, Theme};
use aipass_desktop_native::motion::{unlock_offset, Direction, Timeline, UNLOCK_DURATION_MS};
use gpui::{
    canvas, fill, point, prelude::*, px, size, App, Bounds, Context, EventEmitter, PathBuilder,
    Pixels, Rgba, Window,
};
use std::time::Instant;

#[derive(Clone, Copy)]
pub enum CurtainEvent {
    Covered,
    Done,
}

pub struct Curtain {
    pub direction: Direction,
    started: Instant,
    timeline: Timeline,
    theme: Theme,
    frozen_ms: Option<f64>,
}

impl EventEmitter<CurtainEvent> for Curtain {}

impl Curtain {
    pub(crate) fn apply_appearance(&mut self, dark: bool, cx: &mut gpui::Context<Self>) {
        self.theme = Theme::new(dark);
        cx.notify();
    }

    pub fn new(direction: Direction, theme: Theme) -> Self {
        Self {
            direction,
            started: Instant::now(),
            timeline: Timeline::default(),
            theme,
            frozen_ms: None,
        }
    }

    #[cfg(feature = "parity-render")]
    pub fn freeze_at(&mut self, elapsed_ms: f64) {
        self.frozen_ms = Some(elapsed_ms);
    }
}

impl Render for Curtain {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let elapsed_ms = self
            .frozen_ms
            .unwrap_or_else(|| self.started.elapsed().as_secs_f64() * 1000.);
        let reduced = cx.reduce_motion();
        if self.frozen_ms.is_none() {
            let milestones = self.timeline.advance(elapsed_ms, reduced);
            #[cfg(feature = "parity-render")]
            crate::parity::record_motion(self.direction, elapsed_ms, reduced, &milestones, cx);
            if milestones.covered {
                cx.emit(CurtainEvent::Covered);
            }
            if milestones.done {
                cx.emit(CurtainEvent::Done);
            }
            if elapsed_ms < UNLOCK_DURATION_MS && !reduced {
                window.request_animation_frame();
            }
        }
        let direction = self.direction;
        let theme = self.theme;
        canvas(
            |_, _, _| (),
            move |bounds, _, window, cx| {
                if !reduced {
                    paint(bounds, theme, direction, elapsed_ms, window, cx);
                }
            },
        )
        .absolute()
        .inset_0()
        .size_full()
    }
}

fn paint(
    bounds: Bounds<Pixels>,
    theme: Theme,
    direction: Direction,
    elapsed_ms: f64,
    window: &mut Window,
    cx: &mut App,
) {
    let down = direction == Direction::Lock;
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let y = f32::from(bounds.origin.y) + unlock_offset(elapsed_ms, down, false) as f32 * height;
    // WebKit layout uses 1/64px units before applying its floating transform.
    let css_layout = |value: f64| ((value * 64.).trunc() / 64.) as f32;
    let x = f32::from(bounds.origin.x) + css_layout(-f64::from(width) * 0.02);
    let slab_width = css_layout(f64::from(width) * 1.04);
    let wave_height = css_layout(f64::from(height) * 0.12);
    let wave_y = if down {
        // WebKit's flipped outer SVG uses its device-pixel-snapped height
        // for the transform origin while retaining its CSS layout height.
        let scale = window.scale_factor();
        y + height - 1. + (wave_height * scale).round() / scale - wave_height
    } else {
        y - wave_height + 1.
    };
    let slab = Bounds::new(point(px(x), px(y)), size(px(slab_width), px(height)));
    crate::parity::record_bounds("transition-block", slab, cx);
    crate::parity::record_bounds(
        "transition-wave",
        Bounds::new(
            point(px(x), px(wave_y)),
            size(px(slab_width), px(wave_height)),
        ),
        cx,
    );

    let position = |sx: f32, sy: f32| {
        point(
            px(x + sx / 1440. * slab_width),
            px(wave_y + if down { 1. - sy / 120. } else { sy / 120. } * wave_height),
        )
    };
    // Paint in the SVG's back/front order, including the original 1px overlap.
    for (values, alpha) in [
        ([70., 18., 98., 46., -6., 86., 32.], 0.55),
        ([86., 42., 108., 62., 18., 98., 52.], 1.),
    ] {
        let mut path = PathBuilder::fill();
        path.move_to(position(0., values[0]));
        path.cubic_bezier_to(
            position(720., values[3]),
            position(220., values[1]),
            position(460., values[2]),
        );
        path.cubic_bezier_to(
            position(1440., values[6]),
            position(980., values[4]),
            position(1220., values[5]),
        );
        path.line_to(position(1440., 120.));
        path.line_to(position(0., 120.));
        path.close();
        window.paint_path(
            path.build().expect("finite curtain path"),
            theme.accent.opacity(alpha),
        );
    }
    paint_gradient(slab, bounds, theme, down, window);
}

/// The platform gradient shader adds RGB and alpha noise. This CSS gradient
/// has an opaque sRGB ramp to an Oklab-mixed endpoint. Native solid row runs
/// preserve its color/alpha semantics without changing GPUI globally or
/// postprocessing screenshots. WebKit's own quantization still differs.
fn paint_gradient(
    slab: Bounds<Pixels>,
    viewport: Bounds<Pixels>,
    theme: Theme,
    down: bool,
    window: &mut Window,
) {
    let scale = window.scale_factor();
    let top = f32::from(slab.origin.y);
    let height = f32::from(slab.size.height);
    let first = (top.max(f32::from(viewport.origin.y)) * scale).round() as i32;
    let last = ((top + height).min(f32::from(viewport.bottom())) * scale).round() as i32;
    let mut run = None;
    let end = oklab_mix(theme.accent, theme.bg, 0.78);
    let paint_run = |start: i32, end: i32, color, window: &mut Window| {
        window.paint_quad(fill(
            Bounds::new(
                point(slab.origin.x, px(start as f32 / scale)),
                size(slab.size.width, px((end - start) as f32 / scale)),
            ),
            color,
        ));
    };
    for row in first..last {
        let t = ((f64::from(row) + 0.5) / f64::from(scale) - f64::from(top)) / f64::from(height);
        let t = if down { 1. - t } else { t }.clamp(0., 1.);
        let interpolate =
            |a: f32, b: f32| (f64::from(a) + (f64::from(b) - f64::from(a)) * t) as f32;
        let mut color = Rgba {
            r: interpolate(theme.accent.r, end.r),
            g: interpolate(theme.accent.g, end.g),
            b: interpolate(theme.accent.b, end.b),
            a: 1.,
        };
        for channel in [&mut color.r, &mut color.g, &mut color.b] {
            *channel = (*channel * 255.).round() / 255.;
        }
        match run {
            Some((start, previous)) if previous != color => {
                paint_run(start, row, previous, window);
                run = Some((row, color));
            }
            None => run = Some((row, color)),
            _ => {}
        }
    }
    if let Some((start, color)) = run {
        paint_run(start, last, color, window);
    }
}
