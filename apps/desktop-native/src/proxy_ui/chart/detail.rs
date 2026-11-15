use super::*;
#[derive(Default)]
pub(crate) struct ChartFocus {
    dates: Vec<String>,
    pub handles: Vec<gpui::FocusHandle>,
    pub active: Option<String>,
}
impl ChartFocus {
    pub fn prepare(&mut self, points: &[UsageTimeseriesPoint], cx: &mut Context<Proxy>) {
        let dates = points.iter().map(|p| p.date.clone()).collect::<Vec<_>>();
        if dates != self.dates {
            self.dates = dates;
            self.handles = self.dates.iter().map(|_| cx.focus_handle()).collect();
            self.active = None;
        }
    }
    pub fn focused(&self, window: &Window) -> Option<String> {
        self.handles
            .iter()
            .position(|h| h.is_focused(window))
            .map(|i| self.dates[i].clone())
    }
}
pub(super) fn point_label(date: &str, hourly: bool) -> String {
    if !hourly {
        return date.to_owned();
    }
    let Ok(at) = time::OffsetDateTime::parse(date, &time::format_description::well_known::Rfc3339)
    else {
        return date.to_owned();
    };
    // Resolve the offset at this timestamp, including the repeated DST hour.
    let offset = time::UtcOffset::local_offset_at(at).unwrap_or(time::UtcOffset::UTC);
    let local = at.to_offset(offset);
    format!("{} {:02}:{:02}", local.date(), local.hour(), local.minute())
}
pub(super) fn grid(t: Theme) -> gpui::Div {
    div()
        .absolute()
        .left(px(40.))
        .right_0()
        .top_0()
        .h(px(120.))
        .child(
            gpui::canvas(
                |_, _, _| (),
                move |b, _, w, _| {
                    for y in [8., 64.] {
                        let mut x = b.left();
                        while x < b.right() {
                            w.paint_quad(gpui::fill(
                                gpui::Bounds::new(
                                    gpui::point(x, b.top() + px(y)),
                                    gpui::size(px(3.).min(b.right() - x), px(1.)),
                                ),
                                t.divider.opacity(0.7),
                            ));
                            x += px(7.);
                        }
                    }
                    w.paint_quad(gpui::fill(
                        gpui::Bounds::new(
                            gpui::point(b.left(), b.bottom() - px(1.)),
                            gpui::size(b.size.width, px(1.)),
                        ),
                        t.divider,
                    ));
                },
            )
            .size_full(),
        )
}
pub(super) fn tooltip(
    p: &UsageTimeseriesPoint,
    index: usize,
    count: usize,
    hourly: bool,
    width: f32,
    t: Theme,
    l: &Locale,
) -> gpui::Div {
    let tip_width = 310f32.min(width - 16.).max(0.);
    let center = 40. + (index as f32 + 0.5) * (width - 40.) / count.max(1) as f32;
    let left = (center - tip_width / 2.).clamp(0., (width - tip_width).max(0.));
    let total = admin_ui::row()
        .min_h(px(0.))
        .justify_start()
        .flex_wrap()
        .gap(px(6.))
        .text_color(t.tertiary)
        .child(format!(
            "{} {}",
            l.text("server.totalTokens"),
            compact(tokens(p))
        ))
        .child(format!(
            "{} {}",
            l.text("server.requests"),
            compact(p.request_count)
        ))
        .child(format!(
            "{} {}",
            l.text("server.estimatedCost"),
            cost(p.estimated_cost_micros)
        ));
    let models: Vec<_> = if p.models.is_empty() {
        vec![(None, tokens(p), p.request_count, p.estimated_cost_micros)]
    } else {
        p.models
            .iter()
            .map(|m| {
                (
                    m.model.as_deref(),
                    m.input_tokens
                        .saturating_add(m.output_tokens)
                        .saturating_add(m.cache_read_tokens)
                        .saturating_add(m.cache_creation_tokens),
                    m.request_count,
                    m.estimated_cost_micros,
                )
            })
            .collect()
    };
    let rows = admin_ui::stack()
        .gap(px(3.))
        .mt(px(5.))
        .pt(px(5.))
        .border_t_1()
        .border_color(t.divider)
        .children(
            models
                .into_iter()
                .map(|(model, tokens, requests, model_cost)| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .size(px(7.))
                                .flex_shrink_0()
                                .rounded_full()
                                .bg(color(model.unwrap_or("__unknown__"))),
                        )
                        .child(
                            div().flex_1().min_w_0().truncate().child(
                                model
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| l.text("server.unknownModel")),
                            ),
                        )
                        .child(format!("{} tokens", compact(tokens)))
                        .child(format!("{} req", compact(requests)))
                        .child(cost(model_cost))
                }),
        );
    admin_ui::stack()
        .absolute()
        .top(px(4.))
        .left(px(left))
        .w(px(tip_width))
        .gap(px(0.))
        .px(px(10.))
        .py(px(8.))
        .border_1()
        .border_color(t.divider)
        .rounded(px(4.))
        .bg(t.surface)
        .shadow(vec![gpui_base::box_shadow(
            0.,
            8.,
            24.,
            0.,
            t.popup_shadow.into(),
        )])
        .text_size(px(10.))
        .line_height(px(13.5))
        .text_color(t.secondary)
        .child(
            div()
                .mb(px(4.))
                .text_size(px(11.))
                .font_weight(FontWeight::BOLD)
                .text_color(t.text)
                .child(point_label(&p.date, hourly)),
        )
        .child(total)
        .child(rows)
}
