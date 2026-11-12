use crate::{locale::Locale, theme::Theme};
use aipass_desktop_native::feedback::ToastTimer;
use gpui::{
    div, prelude::*, px, rgb, svg, BoxShadow, Context, FocusHandle, Subscription, Task, Window,
};
use gpui_base::Button;
use std::time::Instant;

#[cfg(all(test, feature = "parity-render"))]
mod tests;

// Synthetic capture text; never used for a real Agent error.
pub fn fixture_message(chinese: bool, long: bool) -> String {
    let message = if chinese {
        "无法刷新本地代理状态。请稍后重试。"
    } else {
        "Could not refresh local proxy status. Try again."
    };
    if long {
        (0..12).map(|_| message).collect::<Vec<_>>().join("\n")
    } else {
        message.into()
    }
}

pub struct ErrorToast {
    message: String,
    timer: ToastTimer,
    epoch: Instant,
    theme: Theme,
    locale: Locale,
    focus: FocusHandle,
    task: Option<Task<()>>,
    scroll: gpui::ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl ErrorToast {
    pub fn new(dark: bool, chinese: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle().tab_index(0);
        let subscriptions = vec![
            cx.on_focus_in(&focus, window, |this, window, cx| {
                this.timer.focus(true, this.epoch.elapsed());
                this.schedule(window, cx);
            }),
            cx.on_focus_out(&focus, window, |this, _, window, cx| {
                this.timer.focus(false, this.epoch.elapsed());
                this.schedule(window, cx);
            }),
        ];
        Self {
            message: String::new(),
            timer: ToastTimer::default(),
            epoch: Instant::now(),
            theme: Theme::new(dark),
            locale: Locale::new(chinese),
            focus,
            task: None,
            scroll: gpui::ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }
    pub fn show(&mut self, message: String, window: &mut Window, cx: &mut Context<Self>) {
        self.message = message;
        self.scroll.set_offset(gpui::point(px(0.), px(0.)));
        self.timer.show(self.epoch.elapsed());
        self.schedule(window, cx);
        cx.notify();
    }
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.timer.dismiss();
        self.task = None;
        self.message.clear();
        cx.notify();
    }
    fn schedule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.task = self.timer.remaining(self.epoch.elapsed()).map(|remaining| {
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(remaining).await;
                let _ = this.update_in(cx, |this, _, cx| {
                    if this.timer.tick(this.epoch.elapsed()) {
                        this.message.clear();
                        cx.notify();
                    }
                });
            })
        });
    }
}

impl Render for ErrorToast {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.timer.visible {
            return div().into_any_element();
        }
        let t = self.theme;
        div()
            .on_children_prepainted(crate::parity::record(&[
                "toast-icon",
                "toast-message",
                "toast-close",
            ]))
            .id("error-toast")
            .role(gpui::Role::Alert)
            .absolute()
            .right(px(20.))
            .bottom(px(20.))
            .w(px(360.))
            .p(px(14.))
            .border_1()
            .border_color(t.border)
            .rounded(px(12.))
            .bg(t.surface)
            .shadow(vec![
                BoxShadow {
                    color: rgb(0x000000).opacity(0.04).into(),
                    offset: gpui::point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(1.),
                    inset: false,
                },
                BoxShadow {
                    color: rgb(if t.bg.r < 0.1 { 0 } else { 0x0f1110 })
                        .opacity(if t.bg.r < 0.1 { 0.5 } else { 0.22 })
                        .into(),
                    offset: gpui::point(px(0.), px(24.)),
                    blur_radius: px(64.),
                    spread_radius: px(0.),
                    inset: false,
                },
            ])
            .flex()
            .items_start()
            .gap(px(10.))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_hover(cx.listener(|this, hovered, window, cx| {
                this.timer.hover(*hovered, this.epoch.elapsed());
                this.schedule(window, cx);
            }))
            .child(
                svg()
                    .path("circle-alert")
                    .size(px(18.))
                    .mt(px(2.))
                    .flex_shrink_0()
                    .text_color(t.danger),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .max_h(px(160.))
                    .child(
                        div()
                            .id("toast-message")
                            .max_h(px(160.))
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .text_size(px(13.))
                            .line_height(px(19.5))
                            .text_color(t.text)
                            .child(self.message.clone()),
                    )
                    .child(scroll_fade(self.scroll.clone(), t.surface)),
            )
            .child(
                Button::new("toast-close")
                    .group("toast-close")
                    .track_focus(&self.focus)
                    .accessibility_label(self.locale.text("common.close"))
                    .size(px(24.))
                    .relative()
                    .flex_shrink_0()
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(t.secondary)
                    .cursor_pointer()
                    .hover(move |s| s.bg(t.surface2).text_color(t.text))
                    .on_click(cx.listener(|this, _, _, cx| this.dismiss(cx)))
                    .child(
                        svg()
                            .path("x")
                            .size(px(16.))
                            .text_color(t.secondary)
                            .group_hover("toast-close", move |s| s.text_color(t.text)),
                    )
                    .child(focus_ring(self.focus.clone(), t.accent)),
            )
            .into_any_element()
    }
}

fn focus_ring(focus: FocusHandle, color: gpui::Rgba) -> impl IntoElement {
    gpui::canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            if focus.is_focused(window) && window.last_input_was_keyboard() {
                let mut ring = gpui::outline(bounds, color, gpui::BorderStyle::Solid);
                ring.border_widths = (2.).into();
                ring.corner_radii = (12.).into();
                window.paint_quad(ring);
            }
        },
    )
    .absolute()
    .left(px(-4.))
    .top(px(-4.))
    .size(px(32.))
}

// The toast surface is opaque. Painting its surface with inverse mask opacity
// over the text produces the same 16px scroll fade as the original scrollMask.
fn scroll_fade(scroll: gpui::ScrollHandle, surface: gpui::Rgba) -> impl IntoElement {
    gpui::canvas(
        |bounds, _, cx| {
            crate::parity::record_bounds("toast-fade", bounds, cx);
        },
        move |bounds, _, window, _| {
            let offset = -f32::from(scroll.offset().y);
            let maximum = f32::from(scroll.max_offset().y);
            let depth = |remaining: f32| {
                if remaining > 1. {
                    remaining.min(16.).min(f32::from(bounds.size.height) / 4.)
                } else {
                    0.
                }
            };
            let (top, bottom) = (depth(offset), depth(maximum - offset));
            let scale = window.scale_factor();
            for (depth, upper) in [(top, true), (bottom, false)] {
                let rows = (depth * scale).ceil() as u32;
                for row in 0..rows {
                    let alpha = (row as f32 + 0.5) / rows as f32;
                    let y = if upper {
                        bounds.top() + px(row as f32 / scale)
                    } else {
                        bounds.bottom() - px(depth) + px(row as f32 / scale)
                    };
                    window.paint_quad(gpui::fill(
                        gpui::Bounds::new(
                            gpui::point(bounds.left(), y),
                            gpui::size(bounds.size.width, px(1. / scale)),
                        ),
                        surface.opacity(if upper { 1. - alpha } else { alpha }),
                    ));
                }
            }
        },
    )
    .absolute()
    .inset_0()
    .size_full()
}
