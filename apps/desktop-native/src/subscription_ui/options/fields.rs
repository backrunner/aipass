use super::*;
use crate::parity::Observe;
use aipass_proxy::UpstreamProxyMode;
use gpui::{div, px, svg, AnyElement, Div, Focusable, FontWeight};
use gpui_base::Switch;

impl Subscriptions {
    fn options_section(&self) -> Div {
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(16.))
            .p(px(18.))
            .border_1()
            .border_color(self.theme.border)
            .rounded(px(10.))
            .bg(self.theme.surface)
    }
    fn options_divider(&self) -> Div {
        div().h(px(1.)).w_full().bg(self.theme.divider)
    }
    fn options_field(
        &self,
        key: String,
        label: &str,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        if key == "balance-body" {
            return div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(15.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(self.theme.secondary)
                        .child(self.text(label)),
                )
                .child(
                    div()
                        .h(px(88.))
                        .w_full()
                        .px(px(12.))
                        .py(px(10.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(self.theme.border)
                        .bg(self.theme.surface)
                        .text_size(px(13.))
                        .line_height(px(19.5))
                        .when_some(self.runtime_options.body.as_ref(), |el, body| {
                            let focused = body.read(cx).focus_handle(cx).is_focused(window);
                            el.when(focused, |el| {
                                el.border_color(self.theme.accent)
                                    .shadow(self.theme.outline(self.theme.focus_ring, 3., 0.))
                            })
                            .when(body.read(cx).presentation().is_disabled(), |el| {
                                el.opacity(0.55)
                            })
                            .child(gpui_base::input::Textarea::new(body))
                        })
                        .observe("runtime-field-balance-body-input"),
                )
                .observe("runtime-field-balance-body")
                .into_any_element();
        }
        self.runtime_options
            .fields
            .get(&key)
            .map(|input| {
                self.field(
                    format!("runtime-field-{key}"),
                    if label.contains('.') {
                        self.text(label)
                    } else {
                        label.into()
                    },
                    input,
                    window,
                    cx,
                )
            })
            .unwrap_or_else(|| div().into_any_element())
    }
    fn options_switch(
        &self,
        key: String,
        label: &str,
        description: &str,
        on: bool,
        disabled: bool,
        render: (&mut Window, &mut Context<Self>),
    ) -> AnyElement {
        let (window, cx) = render;
        let progress = gpui_base::motion::transition(
            format!("runtime-switch-motion-{key}"),
            if on { 1.0_f32 } else { 0.0 },
            gpui_base::Transition::new(std::time::Duration::from_millis(150)).ease(|time| {
                aipass_desktop_native::motion::cubic_bezier(f64::from(time), 0.25, 0.1, 0.25, 1.0)
                    as f32
            }),
            window,
            cx,
        );
        let owner = cx.entity().downgrade();
        let t = self.theme;
        let label = self.text(label);
        let observation = format!("runtime-switch-{key}");
        div()
            .min_h(px(36.))
            .flex()
            .items_center()
            .justify_between()
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
                            .line_height(px(16.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(label.clone()),
                    )
                    .when(!description.is_empty(), |el| {
                        el.child(
                            self.paragraph(self.text(description), 12.)
                                .line_height(px(15.6)),
                        )
                    }),
            )
            .child(
                Switch::new(format!("runtime-switch-{key}"))
                    .checked(on)
                    .disabled(disabled || self.runtime_options.pending)
                    .styles(|s| s.disabled(|s| s.opacity(0.5)))
                    .focus_visible(move |s| s.shadow(t.outline(t.accent, 2., 3.)))
                    .accessibility_label(label)
                    .w(px(36.))
                    .h(px(20.))
                    .p(px(2.))
                    .rounded_full()
                    .bg(gpui::Rgba {
                        r: t.border.r + (t.accent.r - t.border.r) * progress,
                        g: t.border.g + (t.accent.g - t.border.g) * progress,
                        b: t.border.b + (t.accent.b - t.border.b) * progress,
                        a: t.border.a + (t.accent.a - t.border.a) * progress,
                    })
                    .child(
                        div()
                            .size(px(16.))
                            .ml(px(16. * progress))
                            .rounded_full()
                            .bg(gpui::rgb(0xffffff))
                            .shadow(vec![gpui_base::box_shadow(
                                0.,
                                1.,
                                crate::theme::shadow_blur(3.),
                                0.,
                                gpui::black().opacity(0.15),
                            )]),
                    )
                    .on_change(move |value, _, window, cx| {
                        let _ = owner
                            .update(cx, |this, cx| this.options_toggle(&key, value, window, cx));
                    })
                    .observe(observation),
            )
            .into_any_element()
    }
    fn options_check(
        &self,
        key: String,
        label: &str,
        on: bool,
        render: (&mut Window, &mut Context<Self>),
    ) -> AnyElement {
        let (window, cx) = render;
        let owner = cx.entity().downgrade();
        let t = self.theme;
        let label = self.text(label);
        let observation = format!("runtime-check-{key}");
        let focus = window
            .use_keyed_state(format!("{observation}-focus"), cx, |_, cx| {
                cx.focus_handle()
            })
            .read(cx)
            .clone();
        let focused = focus.is_focused(window) && window.last_input_was_keyboard();
        gpui_base::Checkbox::new(observation.clone())
            .track_focus(&focus)
            .checked(on)
            .disabled(self.runtime_options.pending)
            .accessibility_label(label.clone())
            .w_full()
            .min_h(px(20.))
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(12.))
            .line_height(px(15.))
            .font_weight(FontWeight::NORMAL)
            .text_color(if on && key.contains("/event") {
                t.text
            } else {
                t.secondary
            })
            .child(
                div()
                    .size(px(14.))
                    .mx(px(2.))
                    .flex_shrink_0()
                    .rounded(px(5.))
                    .when(focused, |s| {
                        // Match the macOS WebKit checkbox's system focus treatment.
                        s.shadow(t.outline(
                            gpui::rgb(if self.dark { 0x166193 } else { 0x7fb3f9 }),
                            2.5,
                            0.,
                        ))
                    })
                    .when(self.runtime_options.pending, |s| s.opacity(0.5))
                    .border(px(1.5))
                    .border_color(if on {
                        if self.dark {
                            gpui::rgb(0x7ab7ff)
                        } else {
                            t.accent
                        }
                    } else {
                        gpui::rgb(if self.dark { 0x9a9a9a } else { 0x808080 })
                    })
                    .bg(if on {
                        if self.dark {
                            gpui::rgb(0x7ab7ff)
                        } else {
                            t.accent
                        }
                    } else {
                        gpui::rgb(if self.dark { 0x1e1e1e } else { 0xffffff })
                    })
                    .when(on, |el| {
                        el.child(
                            svg()
                                .path("checkbox-check")
                                .size(px(11.))
                                .text_color(gpui::rgb(0xffffff)),
                        )
                    })
                    .observe(format!("{observation}-box")),
            )
            .child(label)
            .on_change(move |state, _, window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    this.options_toggle(
                        &key,
                        state == gpui_base::CheckboxState::Checked,
                        window,
                        cx,
                    );
                });
            })
            .observe(observation)
            .into_any_element()
    }
    fn options_picker(
        &self,
        key: &str,
        label: &str,
        value: String,
        choices: Vec<(String, String)>,
        render: (&mut Window, &mut Context<Self>),
    ) -> AnyElement {
        let (window, cx) = render;
        self.picker(
            format!("runtime-{key}"),
            self.text(label),
            value,
            choices,
            (window, cx),
        )
    }
    pub(super) fn options_quota(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(options) = &self.runtime_options.value else {
            return div().into_any_element();
        };
        let provider = self
            .entry
            .as_ref()
            .and_then(|e| e.provider_id.as_deref())
            .unwrap_or_default();
        let policy = &options.subscription;
        let cap = policy.quota_use_limit_percent.unwrap_or(100);
        let mut caps = vec![50, 60, 70, 80, 90, 95, 100, cap];
        caps.sort();
        caps.dedup();
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                self.options_section()
                    .child(self.options_switch(
                        "tracking".into(),
                        "optimization.enableQuotaTracking",
                        "providerRuntime.quotaHint",
                        options.quota_tracking,
                        false,
                        (window, cx),
                    ))
                    .child(self.options_divider())
                    .child(
                        self.options_picker(
                            "interval",
                            "optimization.refreshInterval",
                            options.quota_refresh_seconds.to_string(),
                            [60, 120, 300, 900, 3600]
                                .into_iter()
                                .map(|s| {
                                    (
                                        s.to_string(),
                                        format!(
                                            "{} {}",
                                            s / 60,
                                            self.text("providerRuntime.minutes")
                                        ),
                                    )
                                })
                                .collect(),
                            (window, cx),
                        ),
                    ),
            )
            .child(
                self.options_picker(
                    "cap",
                    "subscriptionPolicy.quotaCap",
                    cap.to_string(),
                    caps.into_iter()
                        .map(|p| {
                            (
                                p.to_string(),
                                self.text("subscriptionPolicy.quotaCapPercent")
                                    .replace("{percent}", &p.to_string()),
                            )
                        })
                        .collect(),
                    (window, cx),
                ),
            )
            .when(matches!(provider, "codex" | "anthropic"), |el| {
                el.child(
                    self.options_section()
                        .when(provider == "codex", |el| {
                            el.child(self.options_switch(
                                "paid".into(),
                                "subscriptionPolicy.paidCredits",
                                "subscriptionPolicy.paidCreditsHint",
                                policy.allow_paid_credits,
                                false,
                                (window, cx),
                            ))
                            .child(self.options_switch(
                                "reset".into(),
                                "subscriptionPolicy.autoReset",
                                "",
                                policy.auto_reset_credits,
                                false,
                                (window, cx),
                            ))
                            .child(
                                self.usage_button(
                                    "runtime-reset",
                                    self.text("subscriptionPolicy.resetNow"),
                                    "",
                                    false,
                                )
                                .disabled(
                                    self.runtime_options.pending || self.runtime_options.dirty,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| this.options_request(Action::Reset, cx),
                                )),
                            )
                        })
                        .child(self.options_switch(
                            "native".into(),
                            "subscriptionPolicy.nativeSwitch",
                            "subscriptionPolicy.nativeSwitchHint",
                            policy.native_switch.is_some(),
                            self.runtime_options.routes.is_empty(),
                            (window, cx),
                        ))
                        .when_some(policy.native_switch.as_ref(), |el, native| {
                            el.child(
                                self.options_picker(
                                    "route",
                                    "subscriptionPolicy.group",
                                    native.route_id.to_string(),
                                    self.runtime_options
                                        .routes
                                        .iter()
                                        .map(|(id, name)| (id.to_string(), name.clone()))
                                        .collect(),
                                    (window, cx),
                                ),
                            )
                            .child(self.options_switch(
                                "preferred".into(),
                                "subscriptionPolicy.returnPreferred",
                                "",
                                native.return_preferred,
                                false,
                                (window, cx),
                            ))
                        }),
                )
            })
            .into_any_element()
    }
    pub(super) fn options_proxy(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let proxy = self
            .runtime_options
            .value
            .as_ref()
            .and_then(|v| v.proxy.as_ref());
        let mode = proxy
            .map(|p| match p.mode {
                UpstreamProxyMode::System => "system",
                UpstreamProxyMode::Direct => "direct",
                UpstreamProxyMode::Environment => "environment",
                UpstreamProxyMode::Custom => "custom",
            })
            .unwrap_or("inherit");
        self.options_section()
            .child(
                self.options_picker(
                    "proxy",
                    "providerRuntime.outbound",
                    mode.into(),
                    [
                        ("inherit", "providerRuntime.inherit"),
                        ("system", "proxy.useSystemProxy"),
                        ("direct", "providerRuntime.direct"),
                        ("environment", "providerRuntime.environment"),
                        ("custom", "providerRuntime.custom"),
                    ]
                    .into_iter()
                    .map(|(v, label)| (v.into(), self.text(label)))
                    .collect(),
                    (window, cx),
                ),
            )
            .when(mode == "custom", |el| {
                el.child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(self.options_field("proxy-url".into(), "proxy.proxyUrl", window, cx))
                        .child(
                            self.paragraph(self.text("providerRuntime.customProxyHint"), 11.)
                                .line_height(px(14.4)),
                        ),
                )
                .child(self.options_divider())
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .w_full()
                        .line_height(px(15.))
                        .text_size(px(12.))
                        .child(self.text("proxy.authentication"))
                        .child(self.paragraph(self.text("providerRuntime.optional"), 10.)),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(14.))
                        .child(div().flex_1().min_w_0().child(self.options_field(
                            "proxy-user".into(),
                            "proxy.username",
                            window,
                            cx,
                        )))
                        .child(div().flex_1().min_w_0().child(self.options_field(
                            "proxy-password".into(),
                            "proxy.password",
                            window,
                            cx,
                        ))),
                )
                .when(proxy.is_some_and(|p| p.has_credentials), |el| {
                    el.child(self.options_check(
                        "clear-proxy".into(),
                        "providerRuntime.clearAuth",
                        proxy.is_some_and(|p| p.clear_credentials),
                        (window, cx),
                    ))
                })
            })
            .into_any_element()
    }
    pub(super) fn options_balance(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let balance = self
            .runtime_options
            .value
            .as_ref()
            .and_then(|v| v.balance.as_ref());
        self.options_section()
            .child(self.options_switch(
                "balance".into(),
                "providerRuntime.enableBalance",
                "",
                balance.is_some(),
                false,
                (window, cx),
            ))
            .when_some(balance, |el, b| {
                el.child(self.options_divider())
                    .child(
                        div()
                            .flex()
                            .gap(px(14.))
                            .child(div().w(px(112.)).flex_shrink_0().child(self.options_picker(
                                "method",
                                "balanceEndpoint.method",
                                if b.post { "POST" } else { "GET" }.into(),
                                ["GET", "POST"].map(|s| (s.into(), s.into())).to_vec(),
                                (window, cx),
                            )))
                            .child(div().flex_1().min_w_0().child(self.options_field(
                                "balance-url".into(),
                                "URL",
                                window,
                                cx,
                            ))),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(14.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(self.options_field(
                                        "balance-path".into(),
                                        "providerRuntime.path",
                                        window,
                                        cx,
                                    ))
                                    .child(
                                        self.paragraph(self.text("providerRuntime.pathHint"), 11.)
                                            .mt(px(6.))
                                            .line_height(px(15.4)),
                                    ),
                            )
                            .child(div().flex_1().min_w_0().child(self.options_field(
                                "balance-unit".into(),
                                "providerRuntime.unit",
                                window,
                                cx,
                            ))),
                    )
                    .when(b.post, |el| {
                        el.child(self.options_field(
                            "balance-body".into(),
                            "providerRuntime.body",
                            window,
                            cx,
                        ))
                    })
                    .child(self.options_divider())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .w_full()
                            .line_height(px(15.))
                            .text_size(px(12.))
                            .child(self.text("providerRuntime.headers"))
                            .child(
                                self.ghost_button(self.usage_button(
                                    "runtime-header-add",
                                    self.text("providerRuntime.addHeader"),
                                    "plus",
                                    false,
                                ))
                                .disabled(self.runtime_options.pending)
                                .on_click(cx.listener(
                                    |this, _, window, cx| this.options_add_header(window, cx),
                                )),
                            ),
                    )
                    .children(b.headers.iter().enumerate().map(|(i, _)| {
                        div()
                            .flex()
                            .items_end()
                            .gap(px(10.))
                            .child(div().flex_1().min_w_0().child(self.options_field(
                                format!("header-{i}-name"),
                                "providerRuntime.headerName",
                                window,
                                cx,
                            )))
                            .child(div().flex_1().flex_grow(1.3).min_w_0().child(
                                self.options_field(
                                    format!("header-{i}-value"),
                                    "providerRuntime.headerValue",
                                    window,
                                    cx,
                                ),
                            ))
                            .child(
                                self.icon_button(
                                    format!("runtime-header-remove-{i}"),
                                    "trash-2",
                                    self.text("common.remove"),
                                    true,
                                    true,
                                )
                                .mb(px(4.))
                                .disabled(self.runtime_options.pending)
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.options_remove_header(i, window, cx)
                                    },
                                )),
                            )
                    }))
                    .child(
                        self.paragraph(self.text("providerRuntime.headerHint"), 11.)
                            .line_height(px(16.5)),
                    )
                    .child(self.options_test_row(
                        "runtime-probe",
                        "providerRuntime.readBalance",
                        Action::Probe,
                        cx,
                    ))
            })
            .into_any_element()
    }
    fn options_test_row(
        &self,
        id: impl Into<gpui::ElementId>,
        label: &str,
        action: Action,
        cx: &Context<Self>,
    ) -> AnyElement {
        let hook = if let Action::Test(id) = action {
            Some(id)
        } else {
            None
        };
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(
                self.usage_button(
                    id,
                    self.text(label),
                    if hook.is_some() {
                        "send"
                    } else {
                        "circle-dollar-sign"
                    },
                    false,
                )
                .disabled(self.runtime_options.pending || self.runtime_options.dirty)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.options_request(hook.map(Action::Test).unwrap_or(Action::Probe), cx)
                })),
            )
            .when(self.runtime_options.dirty, |el| {
                el.child(self.paragraph(self.text("providerRuntime.saveToTest"), 11.))
            })
            .into_any_element()
    }
    pub(super) fn options_hooks(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let hooks = self
            .runtime_options
            .value
            .as_ref()
            .map(|v| v.webhooks.as_slice())
            .unwrap_or_default();
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .when(hooks.is_empty(), |el| {
                el.child(
                    self.options_section()
                        .py(px(28.))
                        .items_center()
                        .child(
                            svg()
                                .path("bell")
                                .size(px(22.))
                                .text_color(self.theme.tertiary),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::MEDIUM)
                                .child(self.text("providerRuntime.noWebhooks")),
                        )
                        .child(self.paragraph(self.text("providerRuntime.noWebhooksHint"), 11.)),
                )
            })
            .children(hooks.iter().map(|hook| {
                let id = hook.id;
                self.options_section()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(16.))
                            .child(div().flex_1().child(self.options_switch(
                                format!("hook-{id}/enabled"),
                                "webhook.enabled",
                                "",
                                hook.enabled,
                                false,
                                (window, cx),
                            )))
                            .child(
                                self.icon_button(
                                    format!("runtime-hook-remove-{id}"),
                                    "trash-2",
                                    self.text("webhook.remove"),
                                    true,
                                    true,
                                )
                                .disabled(self.runtime_options.pending)
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.options_remove_hook(id, window, cx)
                                    },
                                )),
                            ),
                    )
                    .child(self.options_field(format!("hook-{id}-url"), "URL", window, cx))
                    .child(self.options_field(
                        format!("hook-{id}-secret"),
                        "webhook.secret",
                        window,
                        cx,
                    ))
                    .when(hook.has_secret, |el| {
                        el.child(self.options_check(
                            format!("hook-{id}/clear"),
                            "providerRuntime.clearAuth",
                            hook.clear_secret,
                            (window, cx),
                        ))
                    })
                    .child(self.options_divider())
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(15.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(self.theme.secondary)
                            .child(self.text("providerRuntime.events")),
                    )
                    .child(div().flex().flex_col().gap(px(8.)).children(
                        actions::EVENTS.chunks(2).map(|events| {
                            div().flex().gap(px(8.)).children(events.iter().map(
                                |(event, label)| {
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .px(px(10.))
                                        .py(px(9.))
                                        .border_1()
                                        .border_color(if hook.events.contains(event) {
                                            self.theme.accent.blend(self.theme.border.opacity(0.7))
                                        } else {
                                            self.theme.border
                                        })
                                        .rounded(px(8.))
                                        .bg(if hook.events.contains(event) {
                                            self.theme.accent_soft
                                        } else {
                                            self.theme.surface2
                                        })
                                        .child(self.options_check(
                                            format!("hook-{id}/{label}"),
                                            &format!("webhook.{label}"),
                                            hook.events.contains(event),
                                            (window, cx),
                                        ))
                                        .observe(format!("runtime-event-{id}-{label}"))
                                },
                            ))
                        }),
                    ))
                    .child(self.options_test_row(
                        format!("runtime-webhook-test-{id}"),
                        "providerRuntime.sendTest",
                        Action::Test(id),
                        cx,
                    ))
            }))
            .child(
                self.usage_button(
                    "runtime-hook-add",
                    self.text("webhook.addWebhook"),
                    "plus",
                    false,
                )
                .disabled(self.runtime_options.pending || hooks.len() >= 8)
                .on_click(cx.listener(|this, _, window, cx| this.options_add_hook(window, cx))),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .child(
                        svg()
                            .path("info")
                            .size(px(14.))
                            .mt(px(1.))
                            .flex_shrink_0()
                            .text_color(self.theme.tertiary),
                    )
                    .child(
                        self.paragraph(self.text("providerRuntime.webhookHint"), 11.)
                            .line_height(px(16.5))
                            .flex_1()
                            .min_w_0(),
                    ),
            )
            .into_any_element()
    }
}
