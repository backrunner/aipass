use super::*;
use crate::parity::Observe;
use aipass_proxy::{ModelPricing, UpstreamProxyMode};
use gpui::{div, px};
impl Settings {
    pub(super) fn server(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let t = self.theme;
        let mut body = presentation::cards();
        if self.panel.is_some() {
            body = body.child(self.panel_form(window, cx));
        }
        let Some(config) = self.config.clone() else {
            return body.into_any_element();
        };
        let mut retry = admin_ui::rows();
        if config.routes.is_empty() {
            retry = retry.child(admin_ui::hint(
                self.locale.text("settings.noServerRoutes"),
                t,
            ));
        }
        for (i, route) in config.routes.iter().enumerate() {
            let values = [
                ("server.maxAttempts", u64::from(route.retry.max_attempts)),
                (
                    "server.failureThreshold",
                    u64::from(route.retry.failure_threshold),
                ),
                (
                    "settings.circuitOpenSeconds",
                    route.retry.circuit_open_seconds,
                ),
                ("settings.connectTimeoutMs", route.retry.connect_timeout_ms),
                ("server.firstByteTimeout", route.retry.first_byte_timeout_ms),
                (
                    "settings.streamIdleTimeoutMs",
                    route.retry.stream_idle_timeout_ms,
                ),
            ];
            let mut group = admin_ui::stack()
                .gap(px(10.))
                .when(i + 1 < config.routes.len(), |s| {
                    s.pb(px(12.)).border_b_1().border_color(t.divider)
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(7.))
                        .text_color(t.secondary)
                        .child(
                            gpui::svg()
                                .path("server")
                                .size(px(14.))
                                .text_color(t.secondary),
                        )
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::BOLD)
                                .child(route.name.clone()),
                        ),
                );
            for (chunk, pair) in values.chunks(2).enumerate() {
                let mut row = div().flex().gap(px(10.));
                for (j, (label, v)) in pair.iter().enumerate() {
                    row = row.child(div().flex_1().min_w_0().child(self.field(
                        &format!("retry-{i}-{}", chunk * 2 + j),
                        label,
                        &v.to_string(),
                        false,
                        window,
                        cx,
                    )));
                }
                group = group.child(row);
            }
            retry = retry.child(group.observe(format!("settings-retry-{i}")));
        }
        let mode = match config.upstream_proxy.mode {
            UpstreamProxyMode::System => "system",
            UpstreamProxyMode::Direct => "direct",
            UpstreamProxyMode::Environment => "environment",
            UpstreamProxyMode::Custom => "custom",
        };
        let options = [
            ("system", "settings.upstreamProxySystem"),
            ("direct", "settings.upstreamProxyDirect"),
            ("environment", "settings.upstreamProxyEnvironment"),
            ("custom", "settings.upstreamProxyCustom"),
        ]
        .into_iter()
        .map(|(v, k)| (v, self.locale.text(k)))
        .collect();
        let modes = self.segmented(
            "upstream-mode",
            mode,
            options,
            |s, v, cx| {
                if let Some(c) = &mut s.config {
                    c.upstream_proxy.mode = match v {
                        "direct" => UpstreamProxyMode::Direct,
                        "environment" => UpstreamProxyMode::Environment,
                        "custom" => UpstreamProxyMode::Custom,
                        _ => UpstreamProxyMode::System,
                    };
                }
                if v != "custom" || s.fields.contains("upstream-url") {
                    s.save_server(cx);
                }
                cx.notify();
            },
            cx,
        );
        let mut upstream = presentation::rows().child(
            presentation::row()
                .child(
                    admin_ui::stack()
                        .gap(px(2.))
                        .min_w_0()
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .child(self.locale.text("settings.upstreamProxyMode")),
                        )
                        .when(
                            config.upstream_proxy.mode == UpstreamProxyMode::Environment,
                            |el| {
                                el.child(presentation::description(
                                    self.locale.text("settings.upstreamProxyEnvironmentDesc"),
                                    t,
                                ))
                            },
                        ),
                )
                .child(modes),
        );
        if config.upstream_proxy.mode == UpstreamProxyMode::Custom {
            upstream = upstream.child(
                self.field(
                    "upstream-url",
                    "settings.upstreamProxyUrl",
                    config
                        .upstream_proxy
                        .custom_url
                        .as_deref()
                        .unwrap_or_default(),
                    false,
                    window,
                    cx,
                ),
            );
        }
        let mut pricing = admin_ui::rows().child(admin_ui::hint(
            self.locale.text("settings.modelPricingDesc"),
            t,
        ));
        for (i, price) in config.pricing.iter().enumerate() {
            let mut group = admin_ui::stack()
                .gap(px(12.))
                .p(px(12.))
                .border_1()
                .border_color(t.divider)
                .rounded(px(8.))
                .child(
                    admin_ui::row()
                        .items_end()
                        .gap(px(8.))
                        .child(div().flex_1().child(self.field(
                            &format!("price-model-{i}"),
                            "settings.modelPrefix",
                            &price.model,
                            false,
                            window,
                            cx,
                        )))
                        .child(
                            crate::providers::controls::icon(
                                format!("price-remove-{i}"),
                                "trash-2",
                                self.locale.text("settings.removePricing"),
                                t,
                            )
                            .w(px(28.))
                            .h(px(34.))
                            .disabled(self.pending)
                            .on_click(cx.listener(
                                move |s, _, w, cx| {
                                    if let Some(c) = &mut s.config {
                                        c.pricing.remove(i);
                                    }
                                    s.fields.remove_indexed_row("price", i, w, cx);
                                    s.save_server(cx);
                                },
                            )),
                        ),
                );
            let values = [
                ("settings.inputPrice", Some(price.input_micros_per_million)),
                (
                    "settings.outputPrice",
                    Some(price.output_micros_per_million),
                ),
                (
                    "settings.cacheReadPrice",
                    Some(price.cache_read_micros_per_million),
                ),
                (
                    "settings.cacheCreationPrice",
                    Some(price.cache_creation_micros_per_million),
                ),
                (
                    "settings.cacheCreation1hPrice",
                    price.cache_creation_1h_micros_per_million,
                ),
            ];
            let mut grid = admin_ui::stack().gap(px(10.));
            for (chunk, pair) in values.chunks(2).enumerate() {
                let mut row = div().flex().gap(px(12.));
                for (j, (label, v)) in pair.iter().enumerate() {
                    row = row.child(div().flex_1().min_w_0().child(self.field(
                        &format!("price-{i}-{}", chunk * 2 + j),
                        label,
                        &v.map(|v| v.to_string()).unwrap_or_default(),
                        false,
                        window,
                        cx,
                    )));
                }
                if pair.len() == 1 {
                    row = row.child(div().flex_1().min_w_0());
                }
                grid = grid.child(row);
            }
            group = group.child(grid);
            pricing = pricing.child(group.observe(format!("settings-price-{i}")));
        }
        pricing = pricing.child(
            div().flex().child(
                crate::providers::controls::ghost_button("price-add", false, t)
                    .h(px(26.))
                    .px(px(9.))
                    .text_size(px(12.))
                    .gap(px(6.))
                    .disabled(self.pending)
                    .accessibility_label(self.locale.text("settings.addPricing"))
                    .child(gpui::svg().path("plus").size(px(13.)).text_color(t.text))
                    .child(self.locale.text("settings.addPricing"))
                    .on_click(cx.listener(|s, _, _, cx| {
                        if let Some(c) = &mut s.config {
                            c.pricing.push(ModelPricing {
                                model: String::new(),
                                input_micros_per_million: 0,
                                output_micros_per_million: 0,
                                cache_read_micros_per_million: 0,
                                cache_creation_micros_per_million: 0,
                                cache_creation_1h_micros_per_million: None,
                            });
                        }
                        cx.notify();
                    })),
            ),
        );
        body.child(admin_ui::card(
            self.locale.text("settings.serverFailover"),
            t,
            retry,
        ))
        .child(admin_ui::card(
            self.locale.text("settings.upstreamProxy"),
            t,
            upstream,
        ))
        .child(admin_ui::card(
            self.locale.text("settings.modelPricing"),
            t,
            pricing,
        ))
        .into_any_element()
    }
    pub(super) fn save_server(&mut self, cx: &mut Context<Self>) {
        let Some(mut config) = self.config.clone() else {
            return;
        };
        let parse = |key: &str, min: u64, max: u64| {
            self.fields
                .value(key, cx)
                .parse::<u64>()
                .ok()
                .filter(|v| (min..=max).contains(v))
        };
        let mut valid = true;
        for (i, route) in config.routes.iter_mut().enumerate() {
            let limits = [
                (1, 10),
                (1, 20),
                (1, u64::MAX),
                (100, u64::MAX),
                (1000, u64::MAX),
                (1000, u64::MAX),
            ];
            let values = limits
                .iter()
                .enumerate()
                .map(|(j, (min, max))| parse(&format!("retry-{i}-{j}"), *min, *max))
                .collect::<Option<Vec<_>>>();
            if let Some(v) = values {
                route.retry.max_attempts = v[0] as u8;
                route.retry.failure_threshold = v[1] as u8;
                route.retry.circuit_open_seconds = v[2];
                route.retry.connect_timeout_ms = v[3];
                route.retry.first_byte_timeout_ms = v[4];
                route.retry.stream_idle_timeout_ms = v[5];
            } else {
                valid = false;
            }
        }
        if config.upstream_proxy.mode == UpstreamProxyMode::Custom {
            let url = self.fields.value("upstream-url", cx);
            if url.trim().is_empty() {
                valid = false;
            }
            config.upstream_proxy.custom_url = Some(url.trim().into());
        }
        for (i, p) in config.pricing.iter_mut().enumerate() {
            p.model = self
                .fields
                .value(&format!("price-model-{i}"), cx)
                .trim()
                .into();
            if p.model.is_empty() {
                valid = false;
            }
            let v = (0..4)
                .map(|j| parse(&format!("price-{i}-{j}"), 0, u64::MAX))
                .collect::<Option<Vec<_>>>();
            if let Some(v) = v {
                p.input_micros_per_million = v[0];
                p.output_micros_per_million = v[1];
                p.cache_read_micros_per_million = v[2];
                p.cache_creation_micros_per_million = v[3];
            } else {
                valid = false;
            }
            let hour = self.fields.value(&format!("price-{i}-4"), cx);
            p.cache_creation_1h_micros_per_million = if hour.is_empty() {
                None
            } else {
                match hour.parse() {
                    Ok(v) => Some(v),
                    Err(_) => {
                        valid = false;
                        None
                    }
                }
            };
        }
        if !valid {
            self.error = self.locale.text("settings.saveFailed");
            cx.notify();
            return;
        }
        self.apply(AgentRequest::ServerConfigSet { config }, cx);
    }
}
