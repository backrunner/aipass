use super::*;
use crate::parity::Observe;
use gpui::{div, px, relative, svg, AnyElement, Div, FontWeight};
use gpui_base::Button;
use model::presentation as display;

impl Subscriptions {
    fn usage_text(&self, text: String, size: f32) -> Div {
        self.paragraph(text, size)
            .line_height(px(if size == 11. { 16.5 } else { size * 1.6 }))
    }
    pub(super) fn usage_button(
        &self,
        id: impl Into<gpui::ElementId>,
        label: impl Into<String>,
        icon: &'static str,
        primary: bool,
    ) -> Button {
        self.button(id, label, icon, primary)
            .h(px(26.))
            .px(px(9.))
            .text_size(px(12.))
            .line_height(px(12.))
    }

    pub(super) fn card(&self) -> Div {
        div()
            .w_full()
            .flex_shrink_0()
            .rounded(px(12.))
            .border_1()
            .border_color(self.theme.border)
            .bg(self.theme.surface)
            .overflow_hidden()
            .flex()
            .flex_col()
    }
    pub fn account_card(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(entry) = self.entry.as_ref() else {
            return div().into_any_element();
        };
        let account = display::account(entry, &|key| self.text(key));
        let frame = self
            .account_disclosure
            .sample(self.account_open, window, cx);
        self.card()
            .child(
                div()
                    .px(px(16.))
                    .py(px(12.))
                    .border_b_1()
                    .border_color(self.theme.divider)
                    .text_size(px(13.))
                    .line_height(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.text("providerDetail.account")),
            )
            .child(
                div()
                    .px(px(14.))
                    .py(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .text_size(px(13.))
                    .line_height(px(16.))
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .child(account.account.clone()),
                    )
                    .when(!account.scope.is_empty(), |el| {
                        el.child(
                            div()
                                .text_size(px(11.))
                                .line_height(px(13.))
                                .text_color(self.theme.tertiary)
                                .child(
                                    self.text("subscriptionUsage.workspace")
                                        .replace("{workspace}", &display::tail(&account.scope, 8)),
                                ),
                        )
                    })
                    .when_some(
                        entry
                            .account_identity
                            .as_ref()
                            .filter(|identity| **identity != account.account),
                        |el, identity| {
                            el.child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        Button::new("subscription-account-toggle")
                                            .w_full()
                                            .py(px(6.))
                                            .line_height(px(16.8))
                                            .flex()
                                            .items_center()
                                            .gap(px(6.))
                                            .text_size(px(12.))
                                            .text_color(self.theme.secondary)
                                            .accessibility_label(
                                                self.text("subscriptionUsage.accountId"),
                                            )
                                            .aria_expanded(self.account_open)
                                            .child(
                                                div().flex_1().child(
                                                    self.text("subscriptionUsage.accountId"),
                                                ),
                                            )
                                            .child(frame.chevron(self.theme.tertiary, 15.))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.account_open = !this.account_open;
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        frame.body(
                                            "subscription-account-reveal",
                                            div()
                                                .px(px(10.))
                                                .py(px(8.))
                                                .rounded(px(6.))
                                                .border_1()
                                                .border_color(self.theme.divider)
                                                .bg(self.theme.surface2)
                                                .text_size(px(11.))
                                                .line_height(px(17.6))
                                                .text_color(self.theme.secondary)
                                                .mt(px(6.))
                                                .child(identity.clone()),
                                        ),
                                    ),
                            )
                        },
                    ),
            )
            .observe("subscription-account-card")
            .into_any_element()
    }
    pub fn usage_card(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(entry) = self.entry.as_ref() else {
            return div().into_any_element();
        };
        let snapshot = entry.subscription.as_ref();
        let windows = snapshot.map(|s| s.windows.as_slice()).unwrap_or_default();
        let credits = snapshot
            .and_then(|s| s.credits_remaining.as_ref())
            .filter(|s| !s.trim().is_empty());
        let has_usage = !windows.is_empty() || credits.is_some();
        let failure = if !self.usage_error.is_empty() {
            self.usage_error.as_str()
        } else {
            snapshot
                .and_then(|s| s.error.as_deref())
                .unwrap_or_default()
        };
        let stale = !failure.is_empty() || snapshot.is_some_and(|s| s.stale);
        let reconnect = display::reconnect(failure);
        let console_only = snapshot.is_some_and(display::console_only);
        let reconnect_provider = entry.provider_id.clone().filter(|id| {
            display::catalog()
                .iter()
                .any(|p| p.id == *id && p.local_authorization)
        });
        let entry_id = entry.id;
        let t = self.theme;
        let frame = self.usage_disclosure.sample(self.usage_open, window, cx);
        let mut plan = snapshot.and_then(|s| s.plan.clone()).unwrap_or_default();
        if console_only {
            let key = format!(
                "subscriptionName.{}",
                snapshot
                    .unwrap()
                    .source
                    .strip_prefix("community:")
                    .unwrap_or_default()
            );
            let translated = self.text(&key);
            if translated != key {
                plan = translated;
            }
        }
        let body = div()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .min_w_0()
            .text_size(px(12.))
            .line_height(px(15.))
            .when(
                snapshot.is_some_and(|s| s.plan.is_some() || s.status.is_some()),
                |el| {
                    el.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(19.))
                                    .font_weight(FontWeight::BOLD)
                                    .child(plan),
                            )
                            .when_some(snapshot.and_then(|s| s.status.clone()), |el, status| {
                                el.child(
                                    div()
                                        .px(px(6.))
                                        .py(px(3.))
                                        .rounded(px(4.))
                                        .bg(t.surface2)
                                        .text_size(px(10.))
                                        .line_height(px(12.))
                                        .text_color(t.secondary)
                                        .child(status),
                                )
                            }),
                    )
                },
            )
            .when(!failure.is_empty() || stale && has_usage, |el| {
                el.child(
                    div()
                        .p(px(12.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(t.divider)
                        .bg(t.warning.opacity(if self.dark { 0.14 } else { 0.1 }))
                        .flex()
                        .items_start()
                        .gap(px(10.))
                        .child(
                            svg()
                                .path("circle-alert")
                                .size(px(16.))
                                .flex_shrink_0()
                                .text_color(t.warning),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(div().font_weight(FontWeight::MEDIUM).child(self.text(
                                    if reconnect {
                                        "subscriptionUsage.reconnectTitle"
                                    } else if !failure.is_empty() {
                                        "subscriptionUsage.failedTitle"
                                    } else {
                                        "providerDetail.snapshotStale"
                                    },
                                )))
                                .child(self.usage_text(
                                    self.text(if reconnect {
                                        "subscriptionUsage.reconnectHint"
                                    } else if has_usage {
                                        "subscriptionUsage.cachedHint"
                                    } else {
                                        "subscriptionUsage.failedHint"
                                    }),
                                    12.,
                                )),
                        ),
                )
            })
            .when(!has_usage && failure.is_empty(), |el| {
                el.child(
                    div()
                        .p(px(12.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .text_center()
                        .gap(px(8.))
                        .child(svg().path("gauge").size(px(22.)).text_color(t.tertiary))
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(t.secondary)
                                .child(self.text(if self.usage_pending {
                                    "subscriptionUsage.loadingTitle"
                                } else if console_only {
                                    "subscriptionUsage.consoleTitle"
                                } else {
                                    "subscriptionUsage.emptyTitle"
                                })),
                        )
                        .child(self.usage_text(
                            self.text(if self.usage_pending {
                                "subscriptionUsage.loadingHint"
                            } else if console_only {
                                "subscriptionUsage.consoleHint"
                            } else {
                                "subscriptionUsage.emptyHint"
                            }),
                            12.,
                        )),
                )
            })
            .when(has_usage, |el| {
                el.child(div().flex().flex_col().gap(px(16.)).children(
                    windows.iter().enumerate().map(|(index, usage)| {
                        let used = display::used(usage);
                        let label = display::window_label(usage, &|key| self.text(key));
                        let formatted = used
                            .map(|v| {
                                let v = (v * 10.).round() / 10.;
                                self.text("subscriptionUsage.usedPercent")
                                    .replace("{percent}", &v.to_string())
                            })
                            .unwrap_or_else(|| self.text("subscriptionUsage.unavailable"));
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(7.))
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .justify_between()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(label.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .text_color(t.secondary)
                                            .child(formatted),
                                    ),
                            )
                            .when_some(used, |el, used| {
                                el.child(
                                    gpui_base::Progress::new(format!(
                                        "subscription-progress-{index}"
                                    ))
                                    .value(used as f32)
                                    .accessibility_label(label.clone())
                                    .h(px(7.))
                                    .rounded_full()
                                    .border_1()
                                    .border_color(t.divider)
                                    .bg(t.surface2)
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .h_full()
                                            .w(relative(used as f32 / 100.))
                                            .rounded_full()
                                            .bg(if stale {
                                                t.tertiary
                                            } else if used >= 100. {
                                                t.danger
                                            } else if used >= 80. {
                                                t.warning
                                            } else {
                                                t.accent
                                            }),
                                    ),
                                )
                            })
                            .when_some(
                                usage
                                    .resets_at
                                    .as_deref()
                                    .and_then(|s| display::date(s, self.chinese)),
                                |el, date| {
                                    el.child(
                                        self.usage_text(
                                            self.text("subscriptionUsage.resetAt")
                                                .replace("{time}", &date),
                                            11.,
                                        ),
                                    )
                                },
                            )
                            .observe(format!("subscription-usage-window-{index}"))
                    }),
                ))
            })
            .when_some(credits, |el, credits| {
                el.child(
                    div()
                        .pt(px(12.))
                        .border_t_1()
                        .border_color(t.divider)
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            div()
                                .text_color(t.secondary)
                                .child(self.text("subscriptionUsage.credits")),
                        )
                        .child(div().font_weight(FontWeight::BOLD).child(format!(
                                "{credits}{}",
                                snapshot
                                    .and_then(|s| s.credits_currency.as_ref())
                                    .map(|s| format!(" {s}"))
                                    .unwrap_or_default()
                            ))),
                )
            })
            .children(
                snapshot
                    .map(|s| {
                        [
                            (
                                "providerDetail.subscriptionExpires",
                                s.subscription_expires_at.as_deref(),
                            ),
                            (
                                "providerDetail.subscriptionRenews",
                                s.subscription_renews_at.as_deref(),
                            ),
                            (
                                "providerDetail.billingPeriodEnds",
                                s.billing_period_ends_at.as_deref(),
                            ),
                        ]
                    })
                    .into_iter()
                    .flatten()
                    .filter_map(|(key, value)| {
                        value
                            .and_then(|value| display::date(value, self.chinese))
                            .map(|date| self.usage_text(format!("{}: {date}", self.text(key)), 11.))
                    }),
            )
            .child(
                div()
                    .pt(px(12.))
                    .border_t_1()
                    .border_color(t.divider)
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap(px(10.))
                    .when(reconnect, |el| {
                        el.when_some(reconnect_provider, |el, provider| {
                            el.child(
                                self.usage_button(
                                    "subscription-reauthorize",
                                    self.text("oauthConnect.reauth"),
                                    "log-in",
                                    false,
                                )
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.show(
                                            Some(provider.clone()),
                                            Some(entry_id),
                                            window,
                                            cx,
                                        )
                                    },
                                )),
                            )
                        })
                    })
                    .when(has_usage, |el| {
                        el.when_some(
                            snapshot.and_then(|s| display::date(&s.observed_at, self.chinese)),
                            |el, date| {
                                el.child(
                                    self.usage_text(
                                        self.text("subscriptionUsage.updatedAt")
                                            .replace("{time}", &date),
                                        11.,
                                    ),
                                )
                            },
                        )
                    })
                    .when_some(snapshot.and_then(|s| s.console_url.clone()), |el, url| {
                        el.child(
                            self.usage_button(
                                "subscription-console",
                                self.text("subscriptionUsage.openConsole"),
                                "external-link",
                                false,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.outbox.push_back(Request {
                                        generation: this.usage_generation,
                                        operation: Operation::Console(entry_id, url.clone()),
                                    });
                                    cx.notify();
                                },
                            )),
                        )
                    })
                    .when(!console_only, |el| {
                        el.child(
                            self.usage_button(
                                "subscription-refresh",
                                self.text(if self.usage_pending {
                                    "common.loading"
                                } else if failure.is_empty() {
                                    "subscriptionUsage.refresh"
                                } else {
                                    "subscriptionUsage.retry"
                                }),
                                "refresh-cw",
                                false,
                            )
                            .disabled(self.usage_pending)
                            .on_click(cx.listener(|this, _, _, cx| this.refresh_usage(cx))),
                        )
                    }),
            )
            .observe("subscription-usage-body");
        self.card()
            .child(
                Button::new("subscription-usage-toggle")
                    .w_full()
                    .px(px(16.))
                    .py(px(12.))
                    .border_b_1()
                    .border_color(t.divider)
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(13.))
                    .line_height(px(18.2))
                    .font_weight(FontWeight::SEMIBOLD)
                    .accessibility_label(self.text("providerDetail.subscription"))
                    .aria_expanded(self.usage_open)
                    .child(
                        div()
                            .flex_1()
                            .child(self.text("providerDetail.subscription")),
                    )
                    .child(frame.chevron(t.tertiary, 14.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.usage_open = !this.usage_open;
                        cx.notify();
                    })),
            )
            .child(frame.body("subscription-usage-reveal", body))
            .observe("subscription-usage-card")
            .into_any_element()
    }
}
