use super::*;
use crate::parity::Observe;
use aipass_provider_registry::{AuthScheme, InterfaceType, SecretRef};
use aipass_proxy::{Protocol, ProxyTargetConfig, RetryPolicy, RouteStrategy};
use gpui::{div, px};
pub(super) fn protocols(_: &Locale) -> Vec<(String, String)> {
    [
        ("anthropic_messages", "Anthropic Messages"),
        ("open_ai_responses", "OpenAI Responses"),
        ("open_ai_chat_completions", "OpenAI Chat Completions"),
    ]
    .into_iter()
    .map(|(a, b)| (a.into(), b.into()))
    .collect()
}
pub(super) fn strategies(l: &Locale) -> Vec<(String, String)> {
    [
        ("fallback", "server.strategyFallback"),
        ("round_robin", "server.strategyRoundRobin"),
        ("quota_aware", "server.strategyQuotaAware"),
        ("reset_soonest", "server.strategyResetSoonest"),
        ("quota_pace", "server.strategyQuotaPace"),
    ]
    .into_iter()
    .map(|(v, k)| (v.into(), l.text(k)))
    .collect()
}
pub(super) fn target(
    entry: &aipass_vault::EntrySummary,
    secret: &SecretRef,
    priority: u16,
) -> Option<ProxyTargetConfig> {
    let interface = secret
        .interface_type
        .as_ref()
        .unwrap_or(&entry.interface_type);
    if !matches!(
        interface,
        InterfaceType::OpenAiCompatible
            | InterfaceType::AnthropicMessages
            | InterfaceType::Gemini
            | InterfaceType::AzureOpenAi
    ) {
        return None;
    }
    let auth = secret.effective_auth(&entry.interface_type, &entry.auth_scheme);
    if !matches!(
        auth,
        AuthScheme::Bearer
            | AuthScheme::XApiKey
            | AuthScheme::AzureApiKey
            | AuthScheme::GoogleApiKey
            | AuthScheme::CustomHeader
    ) {
        return None;
    }
    let protocol = if *interface == InterfaceType::AnthropicMessages {
        Protocol::AnthropicMessages
    } else if entry.provider_id.as_deref().is_some_and(|s| {
        s == "openai"
            || (s == "codex"
                && entry.credential_kind == aipass_provider_registry::CredentialKind::OAuth)
    }) {
        Protocol::OpenAiResponses
    } else {
        Protocol::OpenAiChatCompletions
    };
    let base_url = secret
        .endpoint
        .clone()
        .or_else(|| aipass_config_writers::endpoint_url(&entry.endpoints))?;
    Some(ProxyTargetConfig {
        id: Uuid::new_v4(),
        provider_entry_id: entry.id,
        secret_id: secret.id.clone(),
        label: secret.label.clone(),
        base_url,
        auth_scheme: serde_json::to_value(auth).ok()?.as_str()?.into(),
        headers: if protocol == Protocol::AnthropicMessages {
            vec![("anthropic-version".into(), "2023-06-01".into())]
        } else {
            vec![]
        },
        group: secret
            .group
            .clone()
            .or_else(|| entry.gateway.as_ref().and_then(|g| g.group.clone())),
        priority,
        weight: 1,
        enabled: true,
        protocol: Some(protocol),
        prefer_ws: false,
        model: None,
    })
}
impl Proxy {
    pub(super) fn edit_route(
        &mut self,
        route: Option<ProxyRouteConfig>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_modal(Modal::Route, window, cx);
        self.advanced = false;
        let route = route.unwrap_or_else(|| ProxyRouteConfig {
            id: Uuid::new_v4(),
            name: String::new(),
            token: String::new(),
            inbound_protocol: Protocol::OpenAiResponses,
            upstream_protocol: Protocol::OpenAiResponses,
            conversion_enabled: false,
            strategy: RouteStrategy::Fallback,
            targets: vec![],
            retry: RetryPolicy::default(),
            enabled: true,
        });
        self.bind_models = route.targets.iter().any(|t| t.model.is_some());
        self.protocol.update(cx, |v, cx| {
            v.selected = serde_json::to_value(route.inbound_protocol)
                .unwrap()
                .as_str()
                .unwrap()
                .into();
            cx.notify();
        });
        self.strategy.update(cx, |v, cx| {
            v.selected = serde_json::to_value(route.strategy)
                .unwrap()
                .as_str()
                .unwrap()
                .into();
            cx.notify();
        });
        self.route = Some(route);
        self.reconcile_picker(cx);
    }
    pub(super) fn reconcile_picker(&mut self, cx: &mut Context<Self>) {
        let selected = self
            .route
            .as_ref()
            .map(|r| {
                r.targets
                    .iter()
                    .map(|t| (t.provider_entry_id, t.secret_id.clone()))
                    .collect()
            })
            .unwrap_or_default();
        self.picker.update(cx, |p, cx| {
            p.configure(
                self.entries.clone(),
                selected,
                self.theme,
                &self.locale,
                self.busy(),
                cx,
            )
        });
    }
    pub(super) fn add_member(&mut self, value: &str, cx: &mut Context<Self>) {
        let Some((entry, secret)) = value.split_once('/') else {
            return;
        };
        let Ok(id) = Uuid::parse_str(entry) else {
            return;
        };
        if let Some(route) = &mut self.route {
            if let Some(e) = self.entries.iter().find(|e| e.id == id) {
                if let Some(secret) = e.secret_refs.iter().find(|s| s.id == secret) {
                    if let Some(target) = target(e, secret, route.targets.len() as u16) {
                        route.targets.push(target);
                    }
                }
            }
        }
        self.reconcile_picker(cx);
        cx.notify();
    }
    pub(super) fn route_form(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(route) = self.route.clone() else {
            return div().into_any_element();
        };
        let t = self.theme;
        self.protocol.update(cx, |v, cx| {
            v.label = self.locale.text("routeGroup.inboundProtocol");
            v.theme = t;
            v.set_options(protocols(&self.locale), cx);
        });
        self.strategy.update(cx, |v, cx| {
            v.label = self.locale.text("server.strategy");
            v.theme = t;
            v.set_options(strategies(&self.locale), cx);
        });
        let mut body = admin_ui::stack()
            .gap(px(12.))
            .child(self.field(
                "route-name",
                "server.groupName",
                &route.name,
                false,
                window,
                cx,
            ))
            .child(
                div()
                    .flex()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(self.strategy.clone())
                            .observe("proxy-route-strategy"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(self.protocol.clone())
                            .observe("proxy-route-protocol"),
                    ),
            );
        body = body.child(
            self.described_toggle(
                "server.bindMemberModels",
                "server.bindMemberModelsDesc",
                self.bind_models,
                |s, v, cx| {
                    s.bind_models = v;
                    cx.notify();
                },
                window,
                cx,
            )
            .mt(px(8.)),
        );
        if self.bind_models {
            body = body
                .child(
                    div()
                        .px(px(10.))
                        .py(px(6.))
                        .border_1()
                        .border_color(t.border)
                        .rounded(px(6.))
                        .bg(t.surface2)
                        .text_color(t.secondary)
                        .font_family(".AppleSystemUIFontMonospaced")
                        .text_size(px(12.))
                        .line_height(px(15.))
                        .child(format!("group/{}", route.id)),
                )
                .child(
                    admin_ui::hint(self.locale.text("server.groupModelHistoryHint"), t)
                        .mt(px(12.))
                        .px(px(12.))
                        .py(px(10.))
                        .border_l_2()
                        .border_color(t.accent_soft)
                        .rounded(px(4.))
                        .line_height(px(18.)),
                );
        }
        let frame = self.advanced_disclosure.sample(self.advanced, window, cx);
        let divider = if self.advanced {
            t.divider
        } else {
            gpui::rgba(0)
        };
        let advanced_trigger = gpui_base::Button::new("route-advanced")
            .disabled(self.busy())
            .accessibility_label(self.locale.text("server.advancedSettings"))
            .styles(|s| s.disabled(|s| s.opacity(0.45)))
            .focus_visible(move |s| s.shadow(t.outline(t.focus_ring, 2., 1.)))
            .bg(t.surface)
            .text_color(t.text)
            .flex()
            .items_center()
            .cursor_pointer()
            .child(self.locale.text("server.advancedSettings"))
            .h(px(43.2))
            .w_full()
            .border_0()
            .border_b_1()
            .border_color(divider)
            .hover(move |style| style.bg(t.surface2).border_color(divider))
            .text_size(px(13.))
            .line_height(px(18.2))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .px(px(16.))
            .rounded(px(0.))
            .justify_between()
            .child(frame.chevron(t.tertiary, 15.))
            .on_click(cx.listener(|s, _, _, cx| {
                s.advanced = !s.advanced;
                cx.notify();
            }));

        let mut advanced = admin_ui::stack().gap(px(0.));
        let mut retry = admin_ui::stack().gap(px(12.)).px(px(16.)).py(px(14.));
        let mut hold = admin_ui::stack()
            .gap(px(12.))
            .px(px(16.))
            .py(px(14.))
            .border_t_1()
            .border_color(t.divider);
        {
            retry = retry.child(self.described_toggle(
                "server.silentRetry",
                "server.silentRetryDesc",
                route.retry.silent_retry,
                |s, v, cx| {
                    if let Some(r) = &mut s.route {
                        r.retry.silent_retry = v;
                    }
                    cx.notify();
                },
                window,
                cx,
            ));
            if route.retry.silent_retry {
                retry = retry.child(div().w(px(220.)).child(self.field(
                    "silent-retries",
                    "server.maxSilentRetries",
                    &route.retry.max_silent_retries.to_string(),
                    false,
                    window,
                    cx,
                )));
            }
            hold = hold.child(self.described_toggle(
                "server.holdOnFailure",
                "server.holdOnFailureDesc",
                route.retry.hold_on_failure,
                |s, v, cx| {
                    if let Some(r) = &mut s.route {
                        r.retry.hold_on_failure = v;
                    }
                    cx.notify();
                },
                window,
                cx,
            ));
            if route.retry.hold_on_failure {
                hold = hold
                    .child(
                        div()
                            .flex()
                            .gap(px(12.))
                            .child(div().flex_1().min_w_0().child(self.field(
                                "hold-initial",
                                "server.holdInitialDelayMs",
                                &route.retry.hold_initial_delay_ms.to_string(),
                                false,
                                window,
                                cx,
                            )))
                            .child(div().flex_1().min_w_0().child(self.field(
                                "hold-max",
                                "server.holdMaxDelayMs",
                                &route.retry.hold_max_delay_ms.to_string(),
                                false,
                                window,
                                cx,
                            ))),
                    )
                    .child(self.field(
                        "hold-duration",
                        "server.holdMaxDurationMs",
                        &route.retry.hold_max_duration_ms.to_string(),
                        false,
                        window,
                        cx,
                    ));
            }
        }
        advanced = advanced.child(retry).child(hold);
        body = body.child(
            div()
                .mt(px(8.))
                .rounded(px(12.))
                .border_1()
                .border_color(t.border)
                .overflow_hidden()
                .child(advanced_trigger)
                .child(frame.body("route-advanced-body", advanced))
                .observe("proxy-route-advanced"),
        );
        self.reconcile_picker(cx);
        let members = admin_ui::stack()
            .gap(px(10.))
            .mt(px(8.))
            .child(
                admin_ui::row()
                    .min_h(px(0.))
                    .gap(px(12.))
                    .child(
                        div()
                            .text_size(px(11.))
                            .line_height(crate::typography::normal_line_height(
                                window,
                                &self.locale.text("server.members"),
                                11.,
                            ))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(t.tertiary)
                            .child(self.locale.text("server.members")),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .line_height(crate::typography::normal_line_height(
                                window,
                                &self.locale.text("server.memberCount"),
                                11.,
                            ))
                            .text_color(t.tertiary)
                            .child(
                                self.locale
                                    .text("server.memberCount")
                                    .replace("{count}", &route.targets.len().to_string()),
                            ),
                    ),
            )
            .child(self.picker.clone())
            .when(!route.targets.is_empty(), |el| {
                el.child(self.members(&route, window, cx))
            });
        body = body.child(members.observe("proxy-route-members"));
        self.fields.configure(
            "route-name",
            &self.locale.text("server.groupName"),
            self.busy(),
            window,
            cx,
        );
        body.into_any_element()
    }
    pub(super) fn save_route(&mut self, cx: &mut Context<Self>) {
        let Some(mut route) = self.route.clone() else {
            return;
        };
        let Some(mut config) = self.config.clone() else {
            return;
        };
        route.name = self.fields.value("route-name", cx).trim().into();
        if route.name.is_empty() || route.targets.is_empty() {
            self.error = self.locale.text("server.saveGroupFailed");
            cx.notify();
            return;
        }
        for (i, target) in route.targets.iter_mut().enumerate() {
            target.priority = i as u16;
            if self.bind_models {
                let value = self
                    .fields
                    .value(&format!("member-model-{}", target.id), cx);
                if target.enabled && value.trim().is_empty() {
                    self.error = self.locale.text("server.memberModelsRequired");
                    cx.notify();
                    return;
                }
                target.model = Some(value.trim().into());
            } else {
                target.model = None;
            }
            if route.strategy == RouteStrategy::RoundRobin {
                let Ok(weight) = self
                    .fields
                    .value(&format!("member-weight-{}", target.id), cx)
                    .parse::<u32>()
                else {
                    self.error = self.locale.text("settings.saveFailed");
                    cx.notify();
                    return;
                };
                if weight == 0 {
                    self.error = self.locale.text("settings.saveFailed");
                    cx.notify();
                    return;
                }
                target.weight = weight;
            }
        }
        if route.retry.silent_retry && self.fields.contains("silent-retries") {
            match self.fields.value("silent-retries", cx).parse::<u8>() {
                Ok(v) if (1..=20).contains(&v) => route.retry.max_silent_retries = v,
                _ => {
                    self.error = self.locale.text("settings.saveFailed");
                    cx.notify();
                    return;
                }
            }
        }
        if route.retry.hold_on_failure && self.fields.contains("hold-initial") {
            let values = ["hold-initial", "hold-max", "hold-duration"]
                .map(|k| self.fields.value(k, cx).parse::<u64>());
            match values {
                [Ok(initial), Ok(max), Ok(duration)] if initial > 0 && max >= initial => {
                    route.retry.hold_initial_delay_ms = initial;
                    route.retry.hold_max_delay_ms = max;
                    route.retry.hold_max_duration_ms = duration;
                }
                _ => {
                    self.error = self.locale.text("settings.saveFailed");
                    cx.notify();
                    return;
                }
            }
        }
        route.conversion_enabled = route
            .targets
            .iter()
            .any(|t| t.protocol.is_some_and(|p| p != route.inbound_protocol));
        self.selected = Some(route.id);
        if let Some(existing) = config.routes.iter_mut().find(|r| r.id == route.id) {
            *existing = route;
        } else {
            config.routes.push(route);
        }
        self.apply(AgentRequest::ServerConfigSet { config }, cx);
    }
    fn described_toggle(
        &self,
        key: &str,
        description: &str,
        on: bool,
        change: impl Fn(&mut Self, bool, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let owner = cx.entity().downgrade();
        admin_ui::row()
            .min_h(px(36.))
            .gap(px(12.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .line_height(crate::typography::normal_line_height(
                                window,
                                &self.locale.text(key),
                                13.,
                            ))
                            .child(self.locale.text(key)),
                    )
                    .child(
                        admin_ui::hint(self.locale.text(description), self.theme)
                            .line_height(px(15.6)),
                    ),
            )
            .child(
                admin_ui::switch(
                    key.to_owned(),
                    self.locale.text(key),
                    on,
                    self.busy(),
                    self.theme,
                    window,
                    cx,
                )
                .on_change(move |v, _, _, cx| {
                    let _ = owner.update(cx, |s, cx| change(s, v, cx));
                }),
            )
    }
    pub(super) fn toggle(
        &self,
        key: &str,
        on: bool,
        change: impl Fn(&mut Self, bool, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let owner = cx.entity().downgrade();
        let label = if key.starts_with("member-enabled-") {
            self.locale.text("server.enabled")
        } else if key.starts_with("search-enabled-") {
            self.locale.text("search.enabled")
        } else {
            self.locale.text(key)
        };
        admin_ui::row().child(label.clone()).child(
            admin_ui::switch(
                key.to_owned(),
                label,
                on,
                self.busy(),
                self.theme,
                window,
                cx,
            )
            .on_change(move |value, _, _, cx| {
                let _ = owner.update(cx, |s, cx| change(s, value, cx));
            }),
        )
    }
}
