use super::*;
use gpui::{div, px, FontWeight};
use gpui_base::Button;
pub(super) fn compact(v: u64) -> String {
    for (unit, suffix) in [
        (1_000_000_000_000, "T"),
        (1_000_000_000, "B"),
        (1_000_000, "M"),
        (1000, "K"),
    ] {
        if v >= unit {
            return format!(
                "{}{}",
                trim_decimal(format!(
                    "{:.1}",
                    (v as f64 / unit as f64 * 10.).round() / 10.
                )),
                suffix
            );
        }
    }
    v.to_string()
}
fn trim_decimal(s: String) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').into()
    } else {
        s
    }
}
pub(super) fn cost(micros: u64) -> String {
    if micros == 0 {
        return "$0".into();
    }
    let usd = micros as f64 / 1_000_000.;
    if usd >= 1000. {
        return format!("${}", compact(usd.round() as u64));
    }
    let precision = (3. - usd.log10().floor()).max(0.) as usize;
    format!("${}", trim_decimal(format!("{usd:.precision$}")))
}
impl Proxy {
    pub(super) fn status_grid(&self, width: f32) -> gpui::AnyElement {
        let t = self.theme;
        let Some(s) = &self.status else {
            return admin_ui::hint(self.locale.text("common.loading"), t).into_any_element();
        };
        let values = [
            ("server.requests", compact(s.requests)),
            ("server.failures", compact(s.failures)),
            ("server.rpm", compact(s.recent_requests)),
            ("server.tpm", compact(s.recent_tokens)),
            (
                "server.successRate",
                if s.requests == 0 {
                    "-".into()
                } else {
                    format!("{:.1}%", f32::from(s.success_rate_bps) / 100.)
                },
            ),
            (
                "server.firstToken",
                s.average_first_token_ms
                    .map(|v| format!("{} ms", compact(v)))
                    .unwrap_or_else(|| "-".into()),
            ),
            ("server.realtimeConcurrency", compact(s.in_flight_requests)),
            (
                "server.availableChannels",
                format!("{}/{}", s.available_channels, s.total_channels),
            ),
        ];
        let columns = if width <= 480. {
            2
        } else if width <= 760. {
            4
        } else {
            8
        };
        let mut rows = admin_ui::stack().gap(px(12.)).px(px(16.)).py(px(12.));
        for chunk in values.chunks(columns) {
            rows = rows.child(div().flex().gap(px(12.)).children(chunk.iter().map(
                |(label, value)| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(px(11.))
                                .line_height(px(13.2))
                                .text_color(t.tertiary)
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(self.locale.text(label)),
                        )
                        .child(
                            div()
                                .text_size(px(20.))
                                .line_height(px(22.))
                                .font_weight(FontWeight::BOLD)
                                .child(value.clone()),
                        )
                },
            )));
        }
        rows.rounded(px(12.))
            .border_1()
            .border_color(t.border)
            .bg(t.surface)
            .into_any_element()
    }
    fn usage_identities(&self) -> Vec<(Uuid, String)> {
        let mut identities = vec![];
        if let Some(config) = &self.config {
            for target in config.routes.iter().flat_map(|r| &r.targets) {
                let key = (target.provider_entry_id, target.secret_id.clone());
                if !identities.contains(&key) {
                    identities.push(key);
                }
            }
        }
        if let Some(usage) = &self.usage {
            let mut historical = usage
                .providers
                .iter()
                .map(|p| (p.provider_entry_id, p.secret_id.clone()))
                .collect::<Vec<_>>();
            historical.sort();
            for key in historical {
                if !identities.contains(&key) {
                    identities.push(key);
                }
            }
        }
        identities
    }
    pub(super) fn breakdown(
        &self,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let t = self.theme;
        let frame = self.usage_disclosure.sample(self.usage_open, window, cx);
        self.usage_range.update(cx, |v, cx| {
            let value = if self.hourly {
                "24h".into()
            } else {
                self.days.to_string()
            };
            if v.value != value
                || v.theme.bg != t.bg
                || v.locale.is_chinese() != self.locale.is_chinese()
            {
                v.theme = t;
                v.value = value;
                if v.locale.is_chinese() != self.locale.is_chinese() {
                    v.locale = Locale::new(self.locale.is_chinese());
                }
                cx.notify();
            }
        });
        let mut rows = admin_ui::rows().gap(px(0.)).pt(px(12.)).pb(px(14.));
        let mut rendered = Vec::new();
        let headers = [
            "server.usageProvider",
            "server.usageRequests",
            "server.usageInput",
            "server.usageOutput",
            "server.usageCache",
            "server.usageCacheRate",
            "server.usageSuccessRate",
            "server.usageFirstToken",
            "server.usageCost",
        ];
        let identities = self.usage_identities();
        for identity in &identities {
            let entry = self.entries.iter().find(|e| e.id == identity.0);
            let usage = self.usage.as_ref().and_then(|u| {
                u.providers
                    .iter()
                    .find(|p| p.provider_entry_id == identity.0 && p.secret_id == identity.1)
            });
            let label = entry
                .map(|e| e.title.clone())
                .unwrap_or_else(|| identity.0.to_string()[..8].into());
            let secret = entry.and_then(|e| e.secret_refs.iter().find(|s| s.id == identity.1));
            let sublabel = if identities.iter().filter(|i| i.0 == identity.0).count() > 1 {
                secret
                    .map(|s| {
                        if s.label.is_empty() {
                            s.masked.clone()
                        } else {
                            s.label.clone()
                        }
                    })
                    .unwrap_or_else(|| identity.1.chars().take(8).collect())
            } else {
                String::new()
            };
            let input = usage.map_or(0, |p| p.input_tokens);
            let cache_read = usage.map_or(0, |p| p.cache_read_tokens);
            let values = [
                compact(usage.map_or(0, |p| p.request_count)),
                compact(input),
                compact(usage.map_or(0, |p| p.output_tokens)),
                compact(cache_read + usage.map_or(0, |p| p.cache_creation_tokens)),
                if input + cache_read == 0 {
                    "-".into()
                } else {
                    format!(
                        "{:.1}%",
                        cache_read as f64 / (input + cache_read) as f64 * 100.
                    )
                },
                if usage.is_none_or(|p| p.completed_attempts == 0) {
                    "-".into()
                } else {
                    format!(
                        "{}%",
                        trim_decimal(format!(
                            "{:.1}",
                            f32::from(usage.unwrap().success_rate_bps) / 100.
                        ))
                    )
                },
                usage
                    .and_then(|p| p.average_first_token_ms)
                    .map(|v| format!("{} ms", compact(v)))
                    .unwrap_or_else(|| "-".into()),
                cost(usage.map_or(0, |p| p.estimated_cost_micros)),
            ];
            let channels = self
                .status
                .as_ref()
                .filter(|s| s.running)
                .map(|s| {
                    s.channels
                        .iter()
                        .filter(|c| c.provider_entry_id == identity.0 && c.secret_id == identity.1)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let indicators = super::channel::indicators(
                &channels,
                self.config.as_ref().map_or(&[], |c| c.routes.as_slice()),
                t,
                &self.locale,
                window,
                cx,
            );
            let heading = div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.text)
                        .child(label),
                )
                .child(indicators);
            let mut identity = div()
                .flex()
                .flex_col()
                .text_size(px(12.))
                .line_height(px(15.))
                .child(heading);
            if !sublabel.is_empty() {
                identity = identity.child(
                    div()
                        .truncate()
                        .text_size(px(11.))
                        .line_height(px(13.))
                        .text_color(t.tertiary)
                        .child(sublabel),
                );
            }
            rendered.push((identity, values));
        }
        let mut widths = [0_f32; 9];
        for (i, key) in headers.iter().enumerate() {
            widths[i] = measure(&self.locale.text(key), 11., FontWeight::SEMIBOLD, window);
        }
        for (_, values) in &rendered {
            for (i, value) in values.iter().enumerate() {
                widths[i + 1] = widths[i + 1].max(measure(value, 12., FontWeight::NORMAL, window));
            }
        }
        for (i, w) in widths.iter_mut().enumerate() {
            *w += if i == 8 { 8. } else { 16. };
        }
        widths[0] = 80.;
        widths[5] = widths[5].max(56.);
        widths[6] = widths[6].max(56.);
        let natural = widths.iter().sum::<f32>();
        let extra = (width - 34. - natural).max(0.);
        let flexible = natural - widths[0] - widths[5] - widths[6];
        for (i, w) in widths.iter_mut().enumerate() {
            if ![0, 5, 6].contains(&i) {
                *w += extra * *w / flexible;
            }
        }
        let cell = |i: usize| {
            div()
                .w(px(widths[i]))
                .flex_shrink_0()
                .pl(px(if i == 0 { 0. } else { 8. }))
                .pr(px(if i == 8 { 0. } else { 8. }))
        };
        rows = rows.child(
            div()
                .flex()
                .children(headers.iter().enumerate().map(|(i, key)| {
                    cell(i)
                        .py(px(4.))
                        .text_size(px(11.))
                        .line_height(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.tertiary)
                        .whitespace_nowrap()
                        .when(i > 0, |el| el.text_right())
                        .child(self.locale.text(key))
                })),
        );
        for (identity, values) in rendered {
            rows = rows.child(
                div()
                    .flex()
                    .items_center()
                    .border_t_1()
                    .border_color(t.divider)
                    .child(cell(0).py(px(6.)).overflow_hidden().child(identity))
                    .children(values.into_iter().enumerate().map(|(i, value)| {
                        cell(i + 1)
                            .py(px(6.))
                            .text_right()
                            .text_size(px(12.))
                            .line_height(px(15.))
                            .text_color(t.secondary)
                            .whitespace_nowrap()
                            .child(value)
                    })),
            );
        }
        if self.usage_identities().is_empty() {
            rows = admin_ui::rows()
                .items_center()
                .py(px(28.))
                .child(
                    gpui::svg()
                        .path("chart-column")
                        .size(px(18.))
                        .text_color(t.tertiary),
                )
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(self.locale.text("server.usageEmpty")),
                )
                .child(admin_ui::hint(self.locale.text("server.usageEmptyDesc"), t));
        }
        div()
            .rounded(px(12.))
            .border_1()
            .border_color(t.border)
            .bg(t.surface)
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(t.divider)
                    .child(
                        Button::new("proxy-usage-toggle")
                            .flex_1()
                            .min_w_0()
                            .h(px(42.2))
                            .px(px(16.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_size(px(13.))
                            .line_height(px(18.2))
                            .font_weight(FontWeight::SEMIBOLD)
                            .accessibility_label(self.locale.text("server.usageBreakdown"))
                            .child(self.locale.text("server.usageBreakdown"))
                            .child(frame.chevron(t.secondary, 15.))
                            .on_click(cx.listener(|s, _, _, cx| {
                                s.usage_open = !s.usage_open;
                                cx.notify();
                            })),
                    )
                    .child(div().pr(px(16.)).child(self.usage_range.clone())),
            )
            .child(
                frame.body(
                    "proxy-usage-body",
                    div()
                        .id("proxy-breakdown-scroll")
                        .overflow_x_scroll()
                        .child(rows),
                ),
            )
            .into_any_element()
    }
}

fn measure(value: &str, size: f32, weight: FontWeight, window: &Window) -> f32 {
    let mut font = window.text_style().font();
    font.weight = weight;
    window
        .text_system()
        .shape_line(
            value.to_owned().into(),
            px(size),
            &[gpui::TextRun {
                len: value.len(),
                font,
                color: gpui::black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            }],
            None,
        )
        .width
        .into()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quantities_and_costs_keep_small_values_without_trailing_zeroes() {
        assert_eq!(compact(1000), "1K");
        assert_eq!(compact(1_250_000), "1.3M");
        assert_eq!(cost(0), "$0");
        assert_eq!(cost(1), "$0.000001");
        assert_eq!(cost(37_500), "$0.0375");
        assert_eq!(cost(12_345_678), "$12.35");
    }
}
