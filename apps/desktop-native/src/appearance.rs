//! System appearance and a single composited transition for the entire window.
//! Views keep their input/focus identities; no pixels leave the renderer.
use gpui::{App, Pixels, SpanId, Window, WindowAppearance};
use std::time::{Duration, Instant};
#[cfg(all(test, feature = "parity-render"))]
mod tests;

pub(crate) use aipass_desktop_platform::preferences::ThemePreference;
pub(crate) const DURATION: Duration = Duration::from_millis(220);

pub(crate) fn is_dark(appearance: WindowAppearance) -> bool {
    matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}
pub(crate) fn resolve(preference: ThemePreference, appearance: WindowAppearance) -> bool {
    match preference {
        ThemePreference::System => is_dark(appearance),
        ThemePreference::Light => false,
        ThemePreference::Dark => true,
    }
}
pub(crate) fn forced(preference: ThemePreference) -> Option<WindowAppearance> {
    match preference {
        ThemePreference::System => None,
        ThemePreference::Light => Some(WindowAppearance::Light),
        ThemePreference::Dark => Some(WindowAppearance::Dark),
    }
}

pub(crate) struct Appearance {
    pub preference: ThemePreference,
    pub dark: bool,
    pub requested: bool,
    slot: usize,
    started: Option<Instant>,
    viewport: Option<gpui::Size<Pixels>>,
    workspace: bool,
    #[cfg(feature = "parity-render")]
    pub frozen: Option<Duration>,
    #[cfg(feature = "parity-render")]
    pub next_frozen: Option<Duration>,
}
impl Appearance {
    pub fn new(dark: bool) -> Self {
        Self {
            preference: if dark {
                ThemePreference::Dark
            } else {
                ThemePreference::Light
            },
            dark,
            requested: dark,
            slot: 0,
            started: None,
            viewport: None,
            workspace: false,
            #[cfg(feature = "parity-render")]
            frozen: None,
            #[cfg(feature = "parity-render")]
            next_frozen: None,
        }
    }
    pub fn begin(&mut self, reduced: bool) {
        self.dark = self.requested;
        self.slot = 1 - self.slot;
        self.started = (!reduced).then(Instant::now);
        #[cfg(feature = "parity-render")]
        if let Some(elapsed) = self.next_frozen.take() {
            self.frozen = Some(elapsed);
        }
    }
    pub fn cancel(&mut self) {
        self.started = None;
    }
    pub fn progress(&self) -> f32 {
        let Some(started) = self.started else {
            return 1.;
        };
        let elapsed = started.elapsed();
        #[cfg(feature = "parity-render")]
        let elapsed = self.frozen.unwrap_or(elapsed);
        let t = (elapsed.as_secs_f32() / DURATION.as_secs_f32()).clamp(0., 1.);
        // Symmetric easing keeps both directions and interrupted changes smooth.
        t * t * (3. - 2. * t)
    }
    pub fn frame(&mut self, workspace: bool, covered: bool, window: &mut Window, cx: &App) -> f32 {
        let size = window.viewport_size();
        if cx.reduce_motion()
            || self.viewport.is_some_and(|old| old != size)
            || workspace != self.workspace
            || covered
        {
            self.cancel();
        }
        self.viewport = Some(size);
        self.workspace = workspace;
        let progress = self.progress();
        if progress < 1. {
            window.request_animation_frame();
        } else {
            self.cancel();
        }
        window.set_frame_crossfade(span(self.slot), span(1 - self.slot), progress);
        progress
    }
}

fn span(slot: usize) -> SpanId {
    0xa1_aa_00 + slot as u64
}
