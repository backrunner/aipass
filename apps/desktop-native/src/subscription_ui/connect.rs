use super::*;
use crate::parity::Observe;
use gpui::{div, px, svg, AnyElement, FontWeight};
use gpui_base::Button;

impl Subscriptions {
    fn back_button(&self, cx: &Context<Self>) -> Button {
        Button::new("subscription-back")
            .flex()
            .items_center()
            .justify_start()
            .gap(px(6.))
            .text_size(px(if self.provider == "claude" { 14. } else { 12. }))
            .line_height(px(if self.provider == "claude" { 17. } else { 15. }))
            .text_color(self.theme.secondary)
            .disabled(self.provider != "claude" && self.busy())
            .accessibility_label(self.text("oauthConnect.changeProvider"))
            .child(
                svg()
                    .path("arrow-left")
                    .size(px(14.))
                    .text_color(self.theme.secondary),
            )
            .child(self.text("oauthConnect.changeProvider"))
            .on_click(cx.listener(|this, _, window, cx| this.back(window, cx)))
    }
    fn cli_state(&self, claude: bool, cx: &Context<Self>) -> AnyElement {
        let prefix = if claude {
            "claudeConnect"
        } else {
            "subscriptionCli"
        };
        let container = div().flex().flex_col().gap(px(14.));
        if self.pending == Some(Kind::Cli) {
            return container
                .child(self.paragraph(self.text(&format!("{prefix}.checking")), 13.))
                .into_any_element();
        }
        if let Some(cli) = &self.cli {
            if !cli.available {
                let reason = match cli.reason.as_deref() {
                    Some("missing") => "missing",
                    Some("unsupported") => "unsupported",
                    _ => "unusable",
                };
                return container
                    .child(self.banner(self.text(&format!("{prefix}.{reason}")), true))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(8.))
                            .child(
                                self.button(
                                    "subscription-cli-install",
                                    self.text(&format!("{prefix}.install")),
                                    "external-link",
                                    claude,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.queue(Operation::Install(this.provider.clone()), cx)
                                    },
                                )),
                            )
                            .child(
                                self.button(
                                    "subscription-cli-recheck",
                                    self.text(&format!("{prefix}.recheck")),
                                    "refresh-cw",
                                    false,
                                )
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.retry(window, cx)),
                                ),
                            ),
                    )
                    .into_any_element();
            }
        } else {
            return container
                .child(
                    self.button(
                        "subscription-cli-recheck",
                        self.text(&format!("{prefix}.recheck")),
                        "refresh-cw",
                        false,
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.retry(window, cx))),
                )
                .into_any_element();
        }
        container.into_any_element()
    }
    fn failure(&self, cx: &Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .when(!self.error.is_empty(), |el| {
                el.child(self.banner(self.error.clone(), false)).child(
                    self.button(
                        "subscription-retry",
                        self.text("oauthConnect.retry"),
                        "refresh-cw",
                        false,
                    )
                    .disabled(self.pending.is_some())
                    .on_click(cx.listener(|this, _, window, cx| this.retry(window, cx))),
                )
            })
            .when(!self.feedback.is_empty(), |el| {
                el.child(self.paragraph(self.feedback.clone(), 12.))
            })
            .into_any_element()
    }
    pub(super) fn claude(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(self.back_button(cx).observe("claude-back"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(self.brand("claude", "Claude", 48.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight::BOLD)
                                    .child(self.text("claudeConnect.title"))
                                    .observe("claude-title"),
                            )
                            .child(
                                self.paragraph(self.text("claudeConnect.isolated"), 13.)
                                    .observe("claude-description"),
                            ),
                    )
                    .observe("claude-heading"),
            )
            .when(self.pending == Some(Kind::Start), |el| {
                el.child(self.paragraph(self.text("oauthConnect.preparing"), 13.))
            })
            .when(
                self.pending != Some(Kind::Start) && self.login.is_none(),
                |el| el.child(self.cli_state(true, cx)),
            )
            .when_some(self.login.clone(), |el, login| {
                el.when_some(login.url.clone(), |el, url| {
                    el.child(self.paragraph(self.text("claudeConnect.browserHint"), 13.))
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(
                                    self.button(
                                        "subscription-open-browser",
                                        self.text("oauthConnect.openBrowser"),
                                        "external-link",
                                        true,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.queue(Operation::OpenLogin(login.flow), cx)
                                        },
                                    )),
                                )
                                .child(
                                    self.button(
                                        "subscription-copy-link",
                                        self.text("oauthConnect.copyLink"),
                                        "copy",
                                        false,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                                url.clone(),
                                            ));
                                            this.feedback = this.text("oauthConnect.copied");
                                            cx.notify();
                                        },
                                    )),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(15.))
                                .text_color(self.theme.secondary)
                                .text_ellipsis()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .child(login.url.clone().unwrap_or_default()),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .child(self.field(
                                    "subscription-code".into(),
                                    self.text("claudeConnect.pasteLabel"),
                                    &self.code,
                                    window,
                                    cx,
                                ))
                                .child(
                                    self.button(
                                        "subscription-code-submit",
                                        self.text("claudeConnect.submit"),
                                        "",
                                        false,
                                    )
                                    .disabled(
                                        self.code.read(cx).value().trim().is_empty()
                                            || self.pending.is_some(),
                                    )
                                    .on_click(cx.listener(
                                        |this, _, window, cx| this.submit_code(window, cx),
                                    )),
                                ),
                        )
                })
                .child(self.paragraph(self.text("oauthConnect.waiting"), 13.))
            })
            .when(!self.error.is_empty() || !self.feedback.is_empty(), |el| {
                el.child(self.failure(cx))
            })
            .into_any_element()
    }
    pub(super) fn connection(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme;
        let query = self.query.read(cx).value().trim().to_lowercase();
        let providers = self
            .providers
            .iter()
            .filter(|p| {
                format!("{} {}", self.name(p), p.id)
                    .to_lowercase()
                    .contains(&query)
            })
            .collect::<Vec<_>>();
        let busy = self.busy();
        div()
            .h(px(374.))
            .min_h_0()
            .flex()
            .when(self.reconnect_entry.is_none(), |el| {
                el.child(
                    div()
                        .w(px(224.))
                        .h_full()
                        .flex_shrink_0()
                        .min_h_0()
                        .pt(px(16.))
                        .px(px(12.))
                        .pb(px(12.))
                        .border_r_1()
                        .border_color(t.divider)
                        .bg(t.surface2)
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(self.search())
                        .child(
                            div()
                                .relative()
                                .flex_1()
                                .min_h_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .id("subscription-provider-list")
                                        .track_scroll(&self.provider_scroll)
                                        .flex_1()
                                        .min_h_0()
                                        .overflow_y_scroll()
                                        .flex()
                                        .flex_col()
                                        .gap(px(3.))
                                        .children(providers.iter().enumerate().map(|(index, p)| {
                                            let id = p.id.clone();
                                            let name = self.name(p);
                                            let active = p.id == self.provider;
                                            Button::new(format!(
                                                "subscription-connect-provider-{id}"
                                            ))
                                            .flex_shrink_0()
                                            .w_full()
                                            .h(px(40.))
                                            .p(px(8.))
                                            .rounded(px(8.))
                                            .flex()
                                            .items_center()
                                            .gap(px(9.))
                                            .text_size(px(12.))
                                            .line_height(px(15.))
                                            .text_color(if active { t.accent } else { t.secondary })
                                            .when(active, |el| {
                                                el.bg(t.accent_soft)
                                                    .font_weight(FontWeight::SEMIBOLD)
                                            })
                                            .disabled(busy)
                                            .aria_selected(active)
                                            .accessibility_label(name.clone())
                                            .cursor_pointer()
                                            .hover(move |s| {
                                                s.bg(if active { t.accent_soft } else { t.surface })
                                            })
                                            .child(self.brand(&id, &name, 24.))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .text_ellipsis()
                                                    .overflow_hidden()
                                                    .whitespace_nowrap()
                                                    .child(name),
                                            )
                                            .when(active, |el| {
                                                el.child(
                                                    svg()
                                                        .path("check")
                                                        .size(px(14.))
                                                        .text_color(t.accent),
                                                )
                                            })
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.choose(id.clone(), window, cx)
                                            }))
                                            .observe(
                                                format!("subscription-connect-provider-{index}"),
                                            )
                                        }))
                                        .when(providers.is_empty(), |el| {
                                            el.child(self.paragraph(
                                                self.text("communityConnect.noResults"),
                                                11.,
                                            ))
                                        }),
                                )
                                .child(scroll::fade(t.surface2, self.provider_scroll.clone())),
                        )
                        .child(
                            div()
                                .px(px(8.))
                                .text_size(px(10.))
                                .line_height(px(12.))
                                .text_color(t.tertiary)
                                .child(
                                    self.text("communityConnect.providerCount")
                                        .replace("{count}", &self.providers.len().to_string()),
                                ),
                        )
                        .observe("subscription-provider-picker"),
                )
            })
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("subscription-connection-scroll")
                            .track_scroll(&self.body_scroll)
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .overflow_y_scroll()
                            .p(px(20.))
                            .flex()
                            .flex_col()
                            .gap(px(14.))
                            .when(self.reconnect_entry.is_none(), |el| {
                                el.child(self.back_button(cx))
                            })
                            .when(!self.error.is_empty(), |el| {
                                el.child(self.banner(self.error.clone(), false))
                            })
                            .when_some(self.provider(), |el, p| {
                                el.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(12.))
                                        .flex_shrink_0()
                                        .child(self.brand(&p.id, &p.name, 48.))
                                        .child(
                                            div()
                                                .text_size(px(18.))
                                                .line_height(px(22.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child(self.name(p)),
                                        ),
                                )
                            })
                            .when(model::cli_owned(&self.provider), |el| {
                                el.when(!self.cli.as_ref().is_some_and(|s| s.available), |el| {
                                    el.child(self.cli_state(false, cx))
                                })
                            })
                            .when(self.provider().is_some(), |el| {
                                el.child(self.connection_fields(window, cx))
                            })
                            .when(self.provider().is_none(), |el| {
                                if self.pending == Some(Kind::Catalog) {
                                    el.child(self.paragraph(self.text("common.loading"), 12.))
                                } else {
                                    el.child(
                                        self.button(
                                            "subscription-connect-load-retry",
                                            self.text("providerRuntime.retry"),
                                            "",
                                            false,
                                        )
                                        .on_click(
                                            cx.listener(|this, _, _, cx| {
                                                this.error.clear();
                                                this.queue(Operation::Catalog, cx);
                                            }),
                                        ),
                                    )
                                }
                            })
                            .when_some(self.login.as_ref().filter(|l| l.pending), |el, login| {
                                let flow = login.flow;
                                let message = if let Some(code) = &login.user_code {
                                    self.text("subscriptionCli.enterCode")
                                        .replace("{code}", code)
                                } else if model::cli_owned(&self.provider) {
                                    self.text("subscriptionCli.waiting")
                                } else {
                                    login
                                        .instructions
                                        .clone()
                                        .unwrap_or_else(|| self.text("communityConnect.waiting"))
                                };
                                el.child(
                                    div()
                                        .p(px(14.))
                                        .rounded(px(14.))
                                        .border_1()
                                        .border_color(t.border)
                                        .flex()
                                        .flex_col()
                                        .gap(px(12.))
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(t.accent)
                                                .child(self.text("communityConnect.authorizing")),
                                        )
                                        .child(self.paragraph(message, 12.))
                                        .when(login.url.is_some(), |el| {
                                            el.child(
                                                self.button(
                                                    "subscription-open-browser",
                                                    self.text("communityConnect.open"),
                                                    "external-link",
                                                    false,
                                                )
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.queue(Operation::OpenLogin(flow), cx)
                                                })),
                                            )
                                        })
                                        .when(login.code_required, |el| {
                                            el.child(self.field(
                                                "subscription-code".into(),
                                                self.text("communityConnect.code"),
                                                &self.code,
                                                window,
                                                cx,
                                            ))
                                            .child(
                                                self.button(
                                                    "subscription-code-submit",
                                                    self.text("communityConnect.submit"),
                                                    "",
                                                    true,
                                                )
                                                .disabled(
                                                    self.code.read(cx).value().trim().is_empty()
                                                        || self.pending.is_some(),
                                                )
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.submit_code(window, cx)
                                                })),
                                            )
                                        }),
                                )
                            })
                            .observe("subscription-connection-body"),
                    )
                    .child(scroll::fade(t.surface, self.body_scroll.clone())),
            )
            .observe("subscription-connect-layout")
            .into_any_element()
    }
    fn connection_fields(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let cli = model::cli_owned(&self.provider);
        let methods = self
            .provider()
            .map(|p| {
                p.methods
                    .iter()
                    .map(|m| {
                        (
                            m.index.to_string(),
                            if cli {
                                self.text(if m.index == 0 {
                                    "subscriptionCli.signIn"
                                } else {
                                    "subscriptionCli.existing"
                                })
                            } else {
                                self.translated(m.label_key.as_deref(), &m.label)
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .flex_shrink_0()
            .child(self.picker(
                "method".into(),
                self.text("communityConnect.method"),
                self.method.to_string(),
                methods,
                (window, cx),
            ))
            .when_some(self.method(), |el, method| {
                let hint = method.hint_key.as_deref().unwrap_or(if cli {
                    "subscriptionCli.hint"
                } else if method.kind == "api" {
                    "communityConnect.apiHint"
                } else if method.native {
                    "communityConnect.cliHint"
                } else {
                    "communityConnect.browserHint"
                });
                el.child(
                    div()
                        .p(px(12.))
                        .rounded(px(8.))
                        .bg(self.theme.surface2)
                        .flex()
                        .items_start()
                        .gap(px(9.))
                        .child(
                            svg()
                                .path(if method.kind == "api" {
                                    "key-round"
                                } else if method.native {
                                    "terminal"
                                } else {
                                    "globe"
                                })
                                .size(px(16.))
                                .flex_shrink_0()
                                .text_color(self.theme.secondary),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(self.paragraph(self.text(hint), 11.)),
                        ),
                )
                .children(method.prompts.iter().map(|prompt| {
                    let label = self.translated(prompt.message_key.as_deref(), &prompt.message);
                    if prompt.kind == "select" {
                        self.picker(
                            prompt.key.clone(),
                            label,
                            self.choices.get(&prompt.key).cloned().unwrap_or_default(),
                            prompt
                                .options
                                .iter()
                                .map(|o| {
                                    (
                                        o.value.clone(),
                                        self.translated(o.label_key.as_deref(), &o.label),
                                    )
                                })
                                .collect(),
                            (window, cx),
                        )
                    } else if let Some(input) = self.inputs.get(&prompt.key) {
                        self.field(
                            format!("subscription-input-{}", prompt.key),
                            label,
                            input,
                            window,
                            cx,
                        )
                    } else {
                        div().into_any_element()
                    }
                }))
                .when(method.kind == "api", |el| {
                    el.child(self.field(
                        "subscription-api-key".into(),
                        "API key".into(),
                        &self.api_key,
                        window,
                        cx,
                    ))
                    .when_some(
                        model::links::key_page(
                            &self.provider,
                            self.choices
                                .get("region")
                                .map(String::as_str)
                                .unwrap_or_default(),
                        ),
                        |el, uri| {
                            el.child(
                                self.button(
                                    "subscription-get-key",
                                    self.text("communityConnect.getKey"),
                                    "external-link",
                                    false,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.queue(Operation::OpenPublic(uri.clone()), cx)
                                    },
                                )),
                            )
                        },
                    )
                })
            })
            .observe("subscription-fields")
            .into_any_element()
    }
    pub(super) fn connection_footer(&self, cx: &Context<Self>) -> AnyElement {
        let cli = model::cli_owned(&self.provider);
        let method = self.method();
        let busy = self.busy();
        let disabled = self.pending.is_some()
            || method.is_none()
            || (cli && !self.cli.as_ref().is_some_and(|s| s.available))
            || method.is_some_and(|m| {
                m.kind == "api" && self.api_key.read(cx).value().trim().is_empty()
            });
        let note = if cli {
            "subscriptionCli.localNote"
        } else if self.provider().is_some_and(|p| p.local_authorization)
            && method.is_none_or(|m| m.kind != "api")
        {
            "communityConnect.deviceNote"
        } else {
            "communityConnect.localNote"
        };
        div()
            .px(px(24.))
            .py(px(14.))
            .border_t_1()
            .border_color(self.theme.divider)
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(16.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(11.))
                    .line_height(px(14.))
                    .text_color(self.theme.tertiary)
                    .child(
                        svg()
                            .path("shield-check")
                            .size(px(14.))
                            .flex_shrink_0()
                            .text_color(self.theme.tertiary),
                    )
                    .child(self.text(note)),
            )
            .child(if busy {
                self.button(
                    "subscription-cancel",
                    self.text(if self.login.is_some() {
                        "common.cancel"
                    } else {
                        "communityConnect.waiting"
                    }),
                    "",
                    false,
                )
                .disabled(self.pending.is_some())
                .on_click(cx.listener(|this, _, _, cx| this.cancel(cx)))
            } else {
                self.button(
                    "subscription-continue",
                    self.text("oauthConnect.continue"),
                    "",
                    true,
                )
                .child(
                    svg()
                        .path("arrow-right")
                        .size(px(14.))
                        .text_color(gpui::rgb(0xffffff)),
                )
                .disabled(disabled)
                .on_click(cx.listener(|this, _, window, cx| this.start(window, cx)))
            })
            .observe("subscription-footer")
            .into_any_element()
    }
}
