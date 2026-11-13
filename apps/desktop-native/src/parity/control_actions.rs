//! Synthetic capture actions, never enabled for a production vault/window.
use gpui::{
    point, px, App, KeyDownEvent, Keystroke, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PlatformInput, Window,
};

pub fn apply(action: &str, window: &mut Window, cx: &mut App) -> anyhow::Result<()> {
    let (kind, id) = action
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("capture action must be kind:observed-id"))?;
    anyhow::ensure!(
        ["hover", "click", "press", "keyboard", "scroll"].contains(&kind),
        "unknown capture action"
    );
    if kind == "scroll" {
        // Wheel the real scroll container under the target's column. Keep this
        // bounded and synthetic; never alter production window/OS input.
        for _ in 0..12 {
            let bounds = cx
                .try_global::<super::Geometry>()
                .and_then(|g| g.0.get(id))
                .copied()
                .ok_or_else(|| anyhow::anyhow!("capture target missing: {id}"))?;
            let height = f32::from(window.viewport_size().height);
            if bounds[1] >= 80. && bounds[1] + bounds[3] <= height - 48. {
                return Ok(());
            }
            let position = point(px(bounds[0] + bounds[2] / 2.), px(height - 60.));
            let delta = height / 2. - (bounds[1] + bounds[3] / 2.);
            window.dispatch_event(
                PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
                    position,
                    delta: gpui::ScrollDelta::Pixels(point(px(0.), px(delta))),
                    ..Default::default()
                }),
                cx,
            );
            window.draw(cx).clear(cx);
            window.draw(cx).clear(cx);
        }
        anyhow::bail!("capture target could not be scrolled into view: {id}");
    }
    let bounds = cx
        .try_global::<super::Geometry>()
        .and_then(|g| g.0.get(id))
        .copied()
        .ok_or_else(|| anyhow::anyhow!("capture target missing: {id}"))?;
    let position = point(
        px(bounds[0] + bounds[2] / 2.),
        px(bounds[1] + bounds[3] / 2.),
    );
    anyhow::ensure!(
        position.x >= px(0.)
            && position.y >= px(0.)
            && position.x < window.viewport_size().width
            && position.y < window.viewport_size().height,
        "capture target is outside the viewport: {id}"
    );
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position,
            ..Default::default()
        }),
        cx,
    );
    if kind != "hover" {
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position,
                click_count: 1,
                ..Default::default()
            }),
            cx,
        );
        if kind != "press" {
            window.dispatch_event(
                PlatformInput::MouseUp(MouseUpEvent {
                    button: MouseButton::Left,
                    position: if kind == "keyboard" {
                        point(px(0.), px(0.))
                    } else {
                        position
                    },
                    click_count: 1,
                    ..Default::default()
                }),
                cx,
            );
        }
    }
    window.draw(cx).clear(cx);
    if kind == "keyboard" {
        for key in ["tab", "shift-tab"] {
            window.dispatch_event(
                PlatformInput::KeyDown(KeyDownEvent {
                    keystroke: Keystroke::parse(key)?,
                    is_held: false,
                    prefer_character_input: false,
                }),
                cx,
            );
            window.draw(cx).clear(cx);
        }
    }
    window.draw(cx).clear(cx);
    Ok(())
}
