//! Original HeroBackground CSS timelines in seconds and logical pixels.
pub(super) fn movement(
    index: usize,
    elapsed: f64,
    reduced: bool,
    width: f32,
    height: f32,
) -> (f32, f32, f32) {
    let vmax = width.max(height);
    if reduced {
        return (0., 0., 1.);
    }
    let durations = [18., 22., 26., 7., 4.5];
    let cycle = (elapsed / durations[index]) % 2.;
    let mut progress = if cycle <= 1. { cycle } else { 2. - cycle };
    if index == 4 {
        progress = 1. - progress;
    }
    if index < 3 {
        progress = aipass_desktop_native::motion::cubic_bezier(progress, 0.42, 0., 0.58, 1.);
    }
    let p = progress as f32;
    match index {
        0 => (0.08 * vmax * p, 0.04 * vmax * p, 1. + 0.08 * p),
        1 => (-0.06 * vmax * p, -0.03 * vmax * p, 1. + 0.05 * p),
        2 => (0.04 * vmax * p, -0.06 * vmax * p, 1. + 0.1 * p),
        _ => (-0.12 * 1.4 * width * p, 0., 1.),
    }
}
