use super::*;
use gpui::{div, px, AnimationExt};
use gpui_base::{Button, Popup, Tooltip};

#[derive(Clone)]
struct Tip {
    text: String,
    theme: Theme,
}
impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        surface(&self.text, self.theme)
    }
}
fn surface(text: &str, t: Theme) -> impl IntoElement {
    Tooltip::new("channel-tooltip")
        .max_w(px(360.))
        .px(px(10.))
        .py(px(8.))
        .border_1()
        .border_color(t.divider)
        .rounded(px(8.))
        .bg(t.surface)
        .text_color(t.text)
        .text_size(px(11.))
        .line_height(px(16.5))
        .shadow(vec![gpui_base::box_shadow(
            0.,
            4.,
            18.,
            0.,
            t.popup_shadow.into(),
        )])
        .child(
            div()
                .debug_selector(|| "channel-tooltip-content".into())
                .child(text.to_owned()),
        )
}
pub(super) fn indicators(
    channels: &[&aipass_proxy::ChannelStatus],
    routes: &[ProxyRouteConfig],
    t: Theme,
    l: &Locale,
    window: &mut Window,
    cx: &mut gpui::App,
) -> gpui::Div {
    let mut activity = Vec::new();
    let mut health = Vec::new();
    let blocked = channels.iter().any(|c| c.cooldown_remaining_ms > 0);
    for channel in channels {
        let route = routes
            .iter()
            .find(|r| r.id == channel.route_id)
            .map(|r| r.name.clone())
            .unwrap_or_else(|| channel.route_id.to_string()[..8].into());
        if channel.in_flight_requests > 0 {
            activity.push(format!(
                "{route}: {}",
                l.text("server.channelActive")
                    .replace("{count}", &channel.in_flight_requests.to_string())
            ));
        }
        if channel.cooldown_remaining_ms > 0 {
            health.push(format!(
                "{route}: {}",
                l.text("server.channelBlocked").replace(
                    "{seconds}",
                    &channel.cooldown_remaining_ms.div_ceil(1000).to_string()
                )
            ));
        } else {
            if channel.degraded {
                health.push(format!("{route}: {}", l.text("server.channelDegraded")));
            }
            if channel.websocket_cooling_down {
                health.push(format!("{route}: {}", l.text("server.channelHttpFallback")));
            }
        }
    }
    let mut row = div().flex().items_center().gap(px(4.)).flex_shrink_0();
    if let Some(first) = channels.first() {
        let key = format!("{}-{}", first.provider_entry_id, first.secret_id);
        for (kind, text, icon, color) in [
            ("activity", activity.join("\n"), "loader-circle", t.accent),
            (
                "health",
                health.join("\n"),
                if blocked {
                    "circle-slash"
                } else {
                    "circle-alert"
                },
                if blocked { t.danger } else { t.warning },
            ),
        ] {
            if text.is_empty() {
                continue;
            }
            let id = format!("channel-{key}-{kind}");
            let focus = window
                .use_keyed_state(format!("{id}-focus"), cx, |_, cx| cx.focus_handle())
                .read(cx)
                .clone();
            let focused = focus.is_focused(window);
            let tip = Tip {
                text: text.clone(),
                theme: t,
            };
            let mut button = Button::new(id.clone())
                .debug_selector(move || format!("channel-{kind}-trigger"))
                .track_focus(&focus)
                .w(px(12.))
                .h(px(18.))
                .p(px(0.))
                .rounded(px(3.))
                .accessibility_label(text.clone())
                .focus_visible(move |s| s.shadow(t.outline(t.accent, 2., 0.)))
                .child({
                    let icon = gpui::svg().path(icon).size(px(11.)).text_color(color);
                    if kind == "activity" && !cx.reduce_motion() {
                        icon.with_animation(
                            format!("{id}-spin"),
                            gpui::Animation::new(Duration::from_millis(1500)).repeat(),
                            |el, p| {
                                el.with_transformation(gpui::Transformation::rotate(
                                    gpui::percentage(p),
                                ))
                            },
                        )
                        .into_any_element()
                    } else {
                        icon.into_any_element()
                    }
                });
            if !focused {
                button = button
                    .tooltip(move |_, cx| cx.new(|_| tip.clone()).into())
                    .tooltip_show_delay(Duration::from_millis(150));
            }
            let popup = Popup::new(format!("{id}-keyboard-tip"), button)
                .anchor(gpui::Anchor::BottomCenter)
                .offset(px(6.));
            row = row.child(if focused {
                popup.content(surface(&text, t))
            } else {
                popup
            });
        }
    }
    row
}

#[cfg(all(test, feature = "parity-render"))]
mod tests {
    use super::*;
    struct Harness;
    impl Render for Harness {
        fn render(&mut self, w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let fixture: serde_json::Value = serde_json::from_str(include_str!(
                "../../../../tests/desktop-parity/fixtures/administration.json"
            ))
            .unwrap();
            let status: ProxyStatus = serde_json::from_value(fixture["status"].clone()).unwrap();
            let config: ProxyConfig = serde_json::from_value(fixture["config"].clone()).unwrap();
            div()
                .p(px(30.))
                .child(indicators(
                    &status.channels.iter().take(1).collect::<Vec<_>>(),
                    &config.routes,
                    Theme::new(false),
                    &Locale::new(false),
                    w,
                    cx,
                ))
                .child(Button::new("after").child("After"))
        }
    }
    #[gpui::test]
    fn keyboard_focus_exposes_channel_details_and_blur_retires_them(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_base::init(cx);
            cx.set_reduce_motion(true);
        });
        let (_, cx) = cx.add_window_view(|_, _| Harness);
        cx.update(|w, cx| w.draw(cx).clear(cx));
        cx.update(|w, cx| {
            w.focus_next(cx);
            w.draw(cx).clear(cx);
        });
        assert!(cx.debug_bounds("channel-tooltip-content").is_some());
        cx.update(|w, cx| {
            w.blur(cx);
            w.draw(cx).clear(cx);
        });
        assert!(cx.debug_bounds("channel-tooltip-content").is_none());
    }
}
