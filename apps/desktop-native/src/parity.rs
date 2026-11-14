//! Layout observations for the synthetic comparison harness. Observers do not
//! alter layout or inject reference pixels into the renderer.
use gpui::{App, Bounds, Pixels, Window};
#[cfg(feature = "parity-render")]
pub mod appearance;
#[cfg(feature = "parity-render")]
pub mod control_actions;
#[cfg(feature = "parity-render")]
pub mod launch;
mod observation;
pub use observation::Observe;

#[cfg(feature = "parity-render")]
#[derive(Default)]
pub struct Geometry(pub std::collections::BTreeMap<String, [f32; 4]>);
#[cfg(feature = "parity-render")]
impl gpui::Global for Geometry {}

#[cfg(feature = "parity-render")]
#[derive(Default)]
pub struct Styles(pub std::collections::BTreeMap<String, serde_json::Value>);
#[cfg(feature = "parity-render")]
impl gpui::Global for Styles {}

#[cfg(feature = "parity-render")]
#[derive(Default)]
pub struct Scrolls(pub std::collections::BTreeMap<String, [f32; 2]>);
#[cfg(feature = "parity-render")]
impl gpui::Global for Scrolls {}

pub fn record_scroll(name: &str, scroll: &gpui::ScrollHandle, cx: &mut App) {
    #[cfg(feature = "parity-render")]
    {
        if !cx.has_global::<Scrolls>() {
            cx.set_global(Scrolls::default());
        }
        cx.global_mut::<Scrolls>().0.insert(
            name.into(),
            [
                f32::from(scroll.offset().y),
                f32::from(scroll.max_offset().y),
            ],
        );
    }
    #[cfg(not(feature = "parity-render"))]
    let _ = (name, scroll, cx);
}

#[cfg(feature = "parity-render")]
#[derive(Default)]
pub struct Backgrounds {
    pub time_ms: Option<f64>,
    pub frames: std::collections::BTreeMap<String, serde_json::Value>,
}
#[cfg(feature = "parity-render")]
impl gpui::Global for Backgrounds {}

/// GPUI's Metal readback swaps BGRA to RGBA but retains premultiplied RGB.
/// PNG stores straight alpha. Convert the representation before encoding;
/// preserve every alpha value and retain the original readback alongside it.
#[cfg(all(feature = "parity-render", target_os = "macos"))]
pub fn straight_alpha(rgba: &mut [u8]) {
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let alpha = u32::from(pixel[3]);
        if alpha > 0 && alpha < 255 {
            for channel in &mut pixel[..3] {
                *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
}

#[cfg(all(test, feature = "parity-render", target_os = "macos"))]
#[test]
fn png_encoding_preserves_alpha_and_opaque_channels() {
    let mut pixels = [0, 0, 0, 0, 64, 32, 128, 128, 17, 99, 231, 255];
    straight_alpha(&mut pixels);
    assert_eq!(pixels, [0, 0, 0, 0, 128, 64, 255, 128, 17, 99, 231, 255]);
}

/// Opt-in, synthetic-only observations of the live render loop. This records
/// render requests, not presentation timestamps from the display compositor.
#[cfg(feature = "parity-render")]
pub struct MotionTrace {
    pub output: Option<std::fs::File>,
    pub frames: Vec<serde_json::Value>,
}
#[cfg(feature = "parity-render")]
impl gpui::Global for MotionTrace {}

#[cfg(feature = "parity-render")]
pub fn record_motion(
    direction: aipass_desktop_native::motion::Direction,
    elapsed_ms: f64,
    reduced: bool,
    milestones: &aipass_desktop_native::motion::Milestones,
    cx: &mut App,
) {
    use aipass_desktop_native::motion::{unlock_offset, Direction};
    use std::io::Write;
    if !cx.has_global::<MotionTrace>() {
        return;
    }
    let trace = cx.global_mut::<MotionTrace>();
    if trace.output.is_none() {
        return;
    }
    trace.frames.push(serde_json::json!({
        "elapsedMs": elapsed_ms,
        "direction": if direction == Direction::Lock { "lock" } else { "unlock" },
        "offset": unlock_offset(elapsed_ms, direction == Direction::Lock, reduced),
        "covered": milestones.covered, "done": milestones.done, "reduced": reduced,
    }));
    if milestones.done {
        let mut output = trace.output.take().expect("checked trace output");
        let result = serde_json::to_writer_pretty(&mut output, &trace.frames)
            .map_err(std::io::Error::other)
            .and_then(|()| output.flush());
        if let Err(error) = result {
            eprintln!("Motion trace failed: {error}");
        }
    }
}

pub fn record_bounds(name: &str, bounds: Bounds<Pixels>, cx: &mut App) {
    #[cfg(feature = "parity-render")]
    {
        if !cx.has_global::<Geometry>() {
            cx.set_global(Geometry::default());
        }
        cx.global_mut::<Geometry>().0.insert(
            name.into(),
            [
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y),
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
            ],
        );
    }
    #[cfg(not(feature = "parity-render"))]
    let _ = (name, bounds, cx);
}

pub fn record(
    names: &'static [&'static str],
) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) {
    move |bounds, _window, cx| {
        #[cfg(feature = "parity-render")]
        {
            if !cx.has_global::<Geometry>() {
                cx.set_global(Geometry::default());
            }
            let geometry = cx.global_mut::<Geometry>();
            for (name, bounds) in names.iter().zip(bounds) {
                geometry.0.insert(
                    (*name).into(),
                    [
                        f32::from(bounds.origin.x),
                        f32::from(bounds.origin.y),
                        f32::from(bounds.size.width),
                        f32::from(bounds.size.height),
                    ],
                );
            }
        }
        #[cfg(not(feature = "parity-render"))]
        let _ = (names, bounds, cx);
    }
}
