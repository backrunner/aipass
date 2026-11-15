use super::usage::{compact, cost};
use super::*;
use gpui::{div, px, rgb, FontWeight};
use gpui_base::Button;
mod detail;
pub(super) use detail::ChartFocus;
use detail::{grid, point_label, tooltip};
fn timestamp(date: &str) -> Option<i64> {
    time::OffsetDateTime::parse(date, &time::format_description::well_known::Rfc3339)
        .ok()
        .map(|v| v.unix_timestamp())
}
fn tokens(p: &UsageTimeseriesPoint) -> u64 {
    p.input_tokens
        .saturating_add(p.output_tokens)
        .saturating_add(p.cache_read_tokens)
        .saturating_add(p.cache_creation_tokens)
}
fn color(model: &str) -> gpui::Rgba {
    let hash = model
        .encode_utf16()
        .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c.into()));
    rgb([
        0x3b82f6, 0x14b8a6, 0xf59e0b, 0xe879f9, 0xef4444, 0x8b5cf6, 0x84cc16,
    ][hash.unsigned_abs() as usize % 7])
}
fn points(
    series: &[UsageTimeseriesPoint],
    days: u32,
    hourly: bool,
    fixture_now: Option<time::OffsetDateTime>,
) -> Vec<UsageTimeseriesPoint> {
    let offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
    let now = fixture_now
        .unwrap_or_else(time::OffsetDateTime::now_utc)
        .to_offset(offset);
    let count = if hourly { 24 } else { days };
    (0..count)
        .map(|i| {
            let date = if hourly {
                let at = now - time::Duration::hours((count - 1 - i).into());
                at.replace_minute(0)
                    .unwrap()
                    .replace_second(0)
                    .unwrap()
                    .replace_nanosecond(0)
                    .unwrap()
                    .to_offset(time::UtcOffset::UTC)
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap()
            } else {
                (now.date() - time::Duration::days((count - 1 - i).into())).to_string()
            };
            series
                .iter()
                .find(|p| {
                    if hourly {
                        timestamp(&p.date) == timestamp(&date)
                    } else {
                        p.date == date
                    }
                })
                .cloned()
                .unwrap_or(UsageTimeseriesPoint {
                    date,
                    request_count: 0,
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read_tokens: 0,
                    cache_creation_tokens: 0,
                    estimated_cost_micros: 0,
                    models: vec![],
                })
        })
        .collect()
}
impl Proxy {
    pub(super) fn chart(
        &mut self,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let t = self.theme;
        let points = points(&self.series, self.days, self.hourly, self.fixture_now);
        self.chart_focus.prepare(&points, cx);
        let focused = self.chart_focus.focused(window);
        if focused != self.chart_focus.active {
            self.hovered_bucket = focused.clone();
            self.chart_focus.active = focused;
        }
        let max = points.iter().map(tokens).max().unwrap_or(1).max(1);
        let total = points.iter().map(tokens).sum::<u64>();
        let requests = points.iter().map(|p| p.request_count).sum::<u64>();
        let total_cost = points.iter().map(|p| p.estimated_cost_micros).sum::<u64>();
        let mut legend = div().flex_1().min_w_0().flex().flex_wrap().gap(px(10.));
        let mut models = std::collections::BTreeSet::new();
        for p in &points {
            for m in &p.models {
                models.insert(m.model.clone());
            }
        }
        for model in models {
            let name = model.as_deref().unwrap_or("__unknown__");
            legend = legend.child(
                div()
                    .flex()
                    .gap(px(4.))
                    .items_center()
                    .max_w(px(150.))
                    .text_size(px(10.))
                    .text_color(t.tertiary)
                    .child(div().size(px(7.)).rounded_full().bg(color(name)))
                    .child(
                        div().truncate().child(
                            model
                                .clone()
                                .unwrap_or_else(|| self.locale.text("server.unknownModel")),
                        ),
                    ),
            );
        }
        let ranges = div()
            .flex()
            .flex_shrink_0()
            .p(px(2.))
            .gap(px(2.))
            .rounded(px(8.))
            .bg(t.surface2)
            .children(
                [
                    (1, true, "server.last24Hours"),
                    (7, false, "server.last7Days"),
                    (30, false, "server.last30Days"),
                ]
                .into_iter()
                .map(|(days, hourly, key)| {
                    let selected = days == self.days && hourly == self.hourly;
                    gpui_base::Button::new(key)
                        .h(px(19.2))
                        .px(px(10.))
                        .rounded(px(4.))
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .selected(selected)
                        .disabled(self.busy())
                        .text_color(if selected { t.accent } else { t.tertiary })
                        .bg(if selected { t.surface } else { gpui::rgba(0) })
                        .child(self.locale.text(key))
                        .on_click(cx.listener(move |s, _, _, cx| {
                            s.days = days;
                            s.hourly = hourly;
                            s.hovered_bucket = None;
                            s.refresh(cx);
                        }))
                }),
            );
        let narrow = width - 34. <= 460.;
        let plot_width = (width - 34. - if narrow { 0. } else { 124. }).max(0.);
        let mut plot = div().relative().flex_1().min_w_0();
        let mut body = admin_ui::rows()
            .relative()
            .pt(px(8.))
            .pb(px(10.))
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .items_center()
                    .child(legend)
                    .child(ranges),
            );
        if total == 0 && requests == 0 {
            body = body.child(
                admin_ui::stack()
                    .h(px(138.))
                    .items_center()
                    .justify_center()
                    .child(
                        gpui::svg()
                            .path("chart-column")
                            .size(px(20.))
                            .text_color(t.tertiary),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.locale.text("server.usageEmpty")),
                    )
                    .child(admin_ui::hint(self.locale.text("server.usageEmptyDesc"), t)),
            );
        } else {
            let mut chart = div()
                .relative()
                .h(px(138.))
                .flex()
                .gap(px(0.))
                .child(grid(t))
                .child(
                    admin_ui::stack()
                        .w(px(40.))
                        .h(px(64.))
                        .mt(px(2.))
                        .justify_between()
                        .text_right()
                        .pr(px(6.))
                        .text_size(px(10.))
                        .text_color(t.tertiary)
                        .child(compact(max))
                        .child(compact(max / 2)),
                );
            for (i, p) in points.iter().enumerate() {
                let count = tokens(p);
                let date = p.date.clone();
                let focus_date = date.clone();
                let focus = self.chart_focus.handles[i].clone();
                let mut bar = div()
                    .w(gpui::relative(0.62))
                    .h(px(120.))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .justify_end();
                if p.models.is_empty() && (count > 0 || p.request_count > 0) {
                    bar = bar.child(
                        div()
                            .w_full()
                            .h(px((count as f32 / max as f32 * 112.).max(2.)))
                            .rounded(px(2.))
                            .bg(color("__unknown__")),
                    );
                } else {
                    for (j, m) in p.models.iter().enumerate().rev() {
                        let value = m.input_tokens
                            + m.output_tokens
                            + m.cache_read_tokens
                            + m.cache_creation_tokens;
                        bar = bar.child(
                            div()
                                .w_full()
                                .h(px(if value > 0 {
                                    (value as f32 / max as f32 * 112.).max(1.)
                                } else if count == 0 && p.request_count > 0 && j == 0 {
                                    2.
                                } else {
                                    0.
                                }))
                                .rounded(px(2.))
                                .bg(color(m.model.as_deref().unwrap_or("__unknown__"))),
                        );
                    }
                }
                chart = chart.child(
                    Button::new(("usage-bar", i))
                        .track_focus(&self.chart_focus.handles[i])
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .items_center()
                        .h(px(138.))
                        .px(px(0.))
                        .accessibility_label(format!(
                            "{} {} {}",
                            point_label(&p.date, self.hourly),
                            compact(count),
                            self.locale.text("server.totalTokens")
                        ))
                        .focus_visible(move |s| s.shadow(t.outline(t.focus_ring, 2., 0.)))
                        .child(bar)
                        .child(
                            div()
                                .h(px(18.))
                                .text_size(px(10.))
                                .line_height(px(12.))
                                .text_color(t.tertiary)
                                .child(
                                    if i % if self.hourly {
                                        4
                                    } else if self.days == 30 {
                                        5
                                    } else {
                                        1
                                    } == 0
                                        || i == points.len() - 1
                                    {
                                        if self.hourly {
                                            point_label(&p.date, true)
                                                .get(11..16)
                                                .unwrap_or("")
                                                .into()
                                        } else {
                                            p.date.get(5..).unwrap_or("").into()
                                        }
                                    } else {
                                        String::new()
                                    },
                                ),
                        )
                        .on_hover(cx.listener(move |s, hovered: &bool, w, cx| {
                            if *hovered {
                                s.hovered_bucket = Some(date.clone());
                            } else if s.hovered_bucket.as_ref() == Some(&date)
                                && !focus.is_focused(w)
                            {
                                s.hovered_bucket = None;
                            }
                            cx.notify();
                        }))
                        .on_click(cx.listener(move |s, _, _, cx| {
                            s.hovered_bucket = Some(focus_date.clone());
                            cx.notify();
                        })),
                );
            }
            plot = plot.child(chart);
        }
        if total > 0 || requests > 0 {
            if let Some((index, p)) = self
                .hovered_bucket
                .as_ref()
                .and_then(|date| points.iter().enumerate().find(|(_, p)| &p.date == date))
            {
                plot = plot.child(tooltip(
                    p,
                    index,
                    points.len(),
                    self.hourly,
                    plot_width,
                    t,
                    &self.locale,
                ));
            }
        } else {
            self.hovered_bucket = None;
        }
        if total == 0 && requests == 0 {
            return admin_ui::card(self.locale.text("server.usageChart"), t, body)
                .into_any_element();
        }
        let summary = div()
            .flex()
            .flex_shrink_0()
            .when(narrow, |el| {
                el.justify_end()
                    .pt(px(10.))
                    .border_t_1()
                    .border_color(t.divider)
            })
            .when(!narrow, |el| {
                el.flex_col()
                    .justify_center()
                    .gap(px(10.))
                    .min_w(px(108.))
                    .pl(px(16.))
                    .border_l_1()
                    .border_color(t.divider)
            })
            .children(
                [
                    (self.locale.text("server.totalTokens"), compact(total)),
                    (self.locale.text("server.requests"), compact(requests)),
                    (self.locale.text("server.estimatedCost"), cost(total_cost)),
                ]
                .into_iter()
                .enumerate()
                .map(|(i, (label, value))| {
                    div()
                        .flex()
                        .flex_col()
                        .items_end()
                        .gap(px(2.))
                        .when(narrow && i > 0, |el| {
                            el.ml(px(14.))
                                .pl(px(14.))
                                .border_l_1()
                                .border_color(t.divider)
                        })
                        .text_size(px(11.))
                        .line_height(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.tertiary)
                        .child(label)
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(650.))
                                .text_color(t.text)
                                .child(value),
                        )
                }),
            );
        body = body.child(
            div()
                .flex()
                .gap(px(if narrow { 12. } else { 16. }))
                .when(narrow, |el| el.flex_col())
                .child(plot)
                .child(summary),
        );
        admin_ui::card_actions(
            self.locale.text("server.usageChart"),
            t,
            body,
            crate::providers::controls::icon(
                "proxy-clear-usage",
                "trash-2",
                self.locale.text("server.clearUsage"),
                t,
            )
            .size(px(26.))
            .disabled(self.busy())
            .on_click(cx.listener(|s, _, w, cx| s.open_modal(Modal::ClearUsage, w, cx))),
        )
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hourly_buckets_match_timestamp_with_different_rfc3339_spellings() {
        let now = time::OffsetDateTime::parse(
            "2026-11-12T04:15:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();
        let mut fixture = points(&[], 1, true, Some(now));
        let last = fixture.last_mut().unwrap();
        last.date = "2026-11-12T12:00:00.000+08:00".into();
        last.request_count = 12;
        let result = points(&fixture, 1, true, Some(now));
        assert_eq!(result.last().unwrap().request_count, 12);
    }
}
