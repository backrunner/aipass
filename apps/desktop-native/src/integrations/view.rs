use super::*;
use crate::providers::controls::{button, icon};
use gpui::{div, px, svg, Anchor, AnyElement, FontWeight};
use gpui_base::{Button, Popover};

impl Integrations {
    fn picker(&self, cx: &Context<Self>) -> AnyElement {
        let Some(entry) = &self.entry else {
            return div().into_any_element();
        };
        let t = self.theme;
        let selected = entry
            .secret_refs
            .iter()
            .find(|s| Some(&s.id) == self.secret_id.as_ref());
        let label = selected
            .map(|s| s.label.clone())
            .unwrap_or_else(|| self.locale.text("integration.chooseKey"));
        let options = entry.secret_refs.clone();
        let owner = cx.entity().downgrade();
        let focus = self.picker_focus.clone();
        let site_protocol = entry.interface_type.clone();
        let picker = Popover::new("integration-key-picker")
            .open(self.picker_open)
            .track_focus(&focus.container)
            .on_open_change(cx.listener(|this, open: &bool, _, cx| {
                this.picker_open = *open;
                cx.notify();
            }))
            .anchor(Anchor::TopLeft)
            .offset(px(4.))
            .trigger(
                Button::new("integration-key-trigger")
                    .track_focus(&focus.trigger)
                    .disabled(self.busy())
                    .w_full()
                    .min_h(px(60.))
                    .px(px(11.))
                    .py(px(9.))
                    .border_1()
                    .border_color(t.border)
                    .rounded(px(8.))
                    .bg(t.surface)
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .text_color(t.text)
                    .accessibility_label(self.locale.text("integration.credential"))
                    .child(
                        div()
                            .size(px(30.))
                            .flex_shrink_0()
                            .border_1()
                            .border_color(t.divider)
                            .rounded(px(8.))
                            .bg(t.surface2)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                svg()
                                    .path("key-round")
                                    .size(px(15.))
                                    .text_color(t.secondary),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(14.4))
                                    .font_weight(if selected.is_some() {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .text_color(if selected.is_some() {
                                        t.text
                                    } else {
                                        t.tertiary
                                    })
                                    .text_ellipsis()
                                    .child(label),
                            )
                            .when_some(selected, |el, secret| {
                                el.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .child(
                                            div()
                                                .max_w(px(110.))
                                                .font_family("Menlo")
                                                .text_size(px(10.))
                                                .text_color(t.tertiary)
                                                .text_ellipsis()
                                                .child(secret.masked.clone()),
                                        )
                                        .child(Self::protocol_badge(
                                            secret
                                                .interface_type
                                                .as_ref()
                                                .unwrap_or(&entry.interface_type),
                                            t,
                                        )),
                                )
                            }),
                    )
                    .child(
                        svg()
                            .path("chevron-down")
                            .size(px(14.))
                            .text_color(t.tertiary),
                    ),
            )
            .content(move |_, _, _| {
                let close = owner.clone();
                focus
                    .surface(
                        "integration-key-options",
                        focus.trigger.clone(),
                        focus.trigger.clone(),
                        move |_, cx| {
                            let _ = close.update(cx, |this, cx| {
                                this.picker_open = false;
                                cx.notify();
                            });
                        },
                    )
                    .w(px(290.))
                    .max_h(px(300.))
                    .overflow_y_scroll()
                    .p(px(4.))
                    .border_1()
                    .border_color(t.border)
                    .rounded(px(8.))
                    .bg(t.surface)
                    .flex()
                    .flex_col()
                    .children(options.iter().enumerate().map(|(i, secret)| {
                        let owner = owner.clone();
                        let id = secret.id.clone();
                        Button::new(("integration-key", i))
                            .track_focus(&focus.items[i])
                            .role(gpui::Role::MenuItem)
                            .p(px(8.))
                            .rounded(px(6.))
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .text_size(px(12.))
                            .text_color(t.text)
                            .hover(move |s| s.bg(t.accent_soft))
                            .focus(move |s| s.bg(t.accent_soft))
                            .accessibility_label(secret.label.clone())
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .text_ellipsis()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(secret.label.clone()),
                                    )
                                    .child(
                                        div()
                                            .font_family("Menlo")
                                            .text_size(px(10.))
                                            .text_color(t.tertiary)
                                            .child(secret.masked.clone()),
                                    ),
                            )
                            .child(Self::protocol_badge(
                                secret.interface_type.as_ref().unwrap_or(&site_protocol),
                                t,
                            ))
                            .on_click(move |_, window, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    this.select_secret(id.clone(), cx);
                                    this.picker_focus.trigger.focus(window, cx);
                                });
                            })
                    }))
            });
        div()
            .flex()
            .flex_col()
            .gap(px(7.))
            .child(
                div()
                    .text_size(px(11.))
                    .line_height(px(13.2))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(t.secondary)
                    .child(self.locale.text("integration.credential")),
            )
            .child(picker)
            .into_any_element()
    }
    fn protocol_badge(
        protocol: &aipass_provider_registry::InterfaceType,
        t: Theme,
    ) -> impl IntoElement {
        use aipass_provider_registry::InterfaceType::*;
        let label = match protocol {
            OpenAiCompatible => "OpenAI",
            AnthropicMessages => "Anthropic",
            Gemini => "Gemini",
            AzureOpenAi => "Azure OpenAI",
            Bedrock => "Bedrock",
            CustomHttp => "HTTP",
        };
        div()
            .h(px(21.))
            .px(px(8.))
            .rounded_full()
            .border_1()
            .border_color(t.accent.opacity(0.18))
            .bg(t.accent_soft)
            .text_color(t.accent)
            .text_size(px(10.))
            .font_weight(FontWeight::MEDIUM)
            .flex()
            .items_center()
            .child(label)
    }
    fn tool_row(&self, index: usize, tool: ToolConfigTool, cx: &Context<Self>) -> AnyElement {
        let t = self.theme;
        let enabled = self.can_configure(&tool);
        let preview = tool.clone();
        let apply = tool.clone();
        let restore = tool.clone();
        let status = self.statuses.iter().find(|s| s.tool == tool);
        let needs_model = self
            .entry
            .as_ref()
            .and_then(|e| {
                e.secret_refs
                    .iter()
                    .find(|s| Some(&s.id) == self.secret_id.as_ref())
                    .map(|s| !catalog::ready(&tool, e, s))
            })
            .unwrap_or(false);
        div()
            .px(px(16.))
            .py(px(10.))
            .min_h(px(56.))
            .border_b_1()
            .border_color(t.divider)
            .flex()
            .flex_col()
            .gap(px(10.))
            .flex_shrink_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .size(px(32.))
                                    .flex_shrink_0()
                                    .rounded(px(8.))
                                    .bg(t.surface2)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        svg()
                                            .path(catalog::icon(&tool))
                                            .size(px(17.))
                                            .text_color(t.secondary),
                                    ),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_ellipsis()
                                            .child(catalog::name(&tool)),
                                    )
                                    .when(needs_model, |el| {
                                        el.child(
                                            div().text_size(px(11.)).text_color(t.tertiary).child(
                                                self.locale.text(
                                                    "integration.providerDefaultModelRequired",
                                                ),
                                            ),
                                        )
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .flex_shrink_0()
                            .child(div().text_color(t.success).text_size(px(13.)).child("✓"))
                            .child(
                                icon(
                                    ("integration-preview", index),
                                    "eye",
                                    self.locale.text("providerDetail.preview"),
                                    t,
                                )
                                .size(px(26.))
                                .disabled(!enabled)
                                .opacity(if enabled { 1. } else { 0.45 })
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.show_preview(preview.clone(), true, cx)
                                    },
                                )),
                            )
                            .child(
                                button(
                                    ("integration-write", index),
                                    self.locale.text("server.writeConfig"),
                                    false,
                                    t,
                                )
                                .h(px(26.))
                                .px(px(9.))
                                .text_size(px(12.))
                                .disabled(!enabled)
                                .opacity(if enabled { 1. } else { 0.45 })
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.show_preview(apply.clone(), false, cx)
                                    },
                                )),
                            ),
                    ),
            )
            .when(tool == ToolConfigTool::Codex, |el| {
                el.child(
                    div()
                        .ml(px(42.))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(11.))
                                .text_color(t.tertiary)
                                .child(self.locale.text("providerDetail.codexAuthMode")),
                        )
                        .child(
                            div()
                                .p(px(2.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(t.border)
                                .bg(t.surface2)
                                .flex()
                                .gap(px(2.))
                                .children(
                                    [
                                        CodexApiKeyMode::AuthJson,
                                        CodexApiKeyMode::ExperimentalBearerToken,
                                    ]
                                    .into_iter()
                                    .enumerate()
                                    .map(|(i, mode)| {
                                        let active = self.codex_mode == mode;
                                        Button::new(("integration-codex-mode", i))
                                            .disabled(self.busy())
                                            .h(px(24.))
                                            .px(px(8.))
                                            .rounded(px(4.))
                                            .bg(if active { t.surface } else { gpui::rgba(0) })
                                            .text_size(px(11.))
                                            .text_color(t.secondary)
                                            .child(if i == 0 {
                                                "auth.json".into()
                                            } else {
                                                self.locale
                                                    .text("providerDetail.codexModeExperimental")
                                            })
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.set_mode(mode.clone(), cx)
                                            }))
                                    }),
                                ),
                        ),
                )
            })
            .when_some(status, |el, status| {
                el.child(
                    div()
                        .ml(px(42.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_size(px(11.))
                        .text_color(t.secondary)
                        .child(div().flex_1().min_w_0().child(format!(
                                "{}: {}",
                                self.locale.text("integration.current"),
                                status
                                    .entry_title
                                    .clone()
                                    .unwrap_or_else(|| self.locale.text("integration.unmanaged"))
                            )))
                        .when(status.operation_id.is_some(), |el| {
                            el.child(
                                button(
                                    ("integration-restore", index),
                                    self.locale.text("integration.restore"),
                                    false,
                                    t,
                                )
                                .h(px(26.))
                                .text_size(px(12.))
                                .disabled(self.busy() || status.state == "conflict")
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.restore(&restore, cx)),
                                ),
                            )
                        }),
                )
                .when(
                    !["ready", "unmanaged", "api_key_invalid", "api_key_changed"]
                        .contains(&status.state.as_str()),
                    |el| {
                        el.child(
                            self.banner(
                                self.locale
                                    .text(&format!("integration.state.{}", status.state)),
                                t.warning,
                            ),
                        )
                    },
                )
                .when(
                    ["api_key_invalid", "api_key_changed"].contains(&status.state.as_str()),
                    |el| {
                        el.child(
                            self.banner(self.locale.text("integration.updateApiKey"), t.warning),
                        )
                    },
                )
                .when(!status.overrides.is_empty(), |el| {
                    el.child(
                        self.banner(
                            self.locale
                                .text("integration.environmentOverrides")
                                .replace("{keys}", &status.overrides.join(", ")),
                            t.warning,
                        ),
                    )
                })
            })
            .when_some(
                self.applied.as_ref().filter(|result| result.tool == tool),
                |el, result| {
                    el.child(
                        div()
                            .p(px(10.))
                            .rounded(px(8.))
                            .bg(t.success.opacity(0.08))
                            .text_color(t.success)
                            .text_size(px(12.))
                            .flex()
                            .items_start()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        self.locale
                                            .text("providerDetail.configured")
                                            .replace("{title}", &result.entry_title),
                                    )
                                    .child(
                                        div()
                                            .font_family(preview::MONO)
                                            .text_size(px(11.))
                                            .child(result.target_path.clone()),
                                    ),
                            )
                            .child(
                                icon(
                                    ("integration-dismiss-applied", index),
                                    "x",
                                    self.locale.text("common.close"),
                                    t,
                                )
                                .size(px(20.))
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.applied = None;
                                        this.message_timer = None;
                                        cx.notify();
                                    },
                                )),
                            ),
                    )
                },
            )
            .into_any_element()
    }
    fn banner(&self, text: String, color: gpui::Rgba) -> impl IntoElement {
        div()
            .p(px(10.))
            .rounded(px(8.))
            .bg(color.opacity(0.08))
            .text_color(color)
            .text_size(px(12.))
            .child(text)
    }
}
impl Render for Integrations {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.entry.is_none() {
            return div().into_any_element();
        }
        let t = self.theme;
        let tools = self.tools();
        div()
            .w_full()
            .flex_shrink_0()
            .rounded(px(10.))
            .border_1()
            .border_color(t.border)
            .bg(t.surface)
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(if self.expanded {
                        t.divider
                    } else {
                        gpui::rgba(0)
                    })
                    .child(
                        Button::new("integration-expand")
                            .flex_1()
                            .min_w_0()
                            .px(px(16.))
                            .py(px(12.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.text)
                            .accessibility_label(self.locale.text("server.integrate"))
                            .child(
                                div()
                                    .flex_1()
                                    .text_left()
                                    .child(self.locale.text("server.integrate")),
                            )
                            .child(
                                svg()
                                    .path(if self.expanded {
                                        "chevron-down"
                                    } else {
                                        "chevron-right"
                                    })
                                    .size(px(15.))
                                    .text_color(t.tertiary),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.expanded = !this.expanded;
                                cx.notify();
                            })),
                    )
                    .child(
                        icon(
                            "integration-scan",
                            "refresh-cw",
                            self.locale.text("integration.scanInstallations"),
                            t,
                        )
                        .mr(px(12.))
                        .disabled(self.busy())
                        .on_click(cx.listener(|this, _, _, cx| this.scan(cx))),
                    ),
            )
            .when(self.expanded, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_col()
                        .when(!tools.is_empty(), |el| {
                            el.child(
                                div()
                                    .px(px(16.))
                                    .pt(px(12.))
                                    .pb(px(14.))
                                    .border_b_1()
                                    .border_color(t.divider)
                                    .child(self.picker(cx)),
                            )
                        })
                        .when(!self.error.is_empty(), |el| {
                            el.child(self.banner(self.error.clone(), t.danger))
                        })
                        .when(!self.message.is_empty(), |el| {
                            el.child(self.banner(self.message.clone(), t.success))
                        })
                        .when(tools.is_empty(), |el| {
                            el.child(
                                div()
                                    .px(px(16.))
                                    .py(px(10.))
                                    .min_h(px(56.))
                                    .text_size(px(13.))
                                    .child(self.locale.text("agents.empty")),
                            )
                        })
                        .children(
                            tools
                                .into_iter()
                                .enumerate()
                                .map(|(i, tool)| self.tool_row(i, tool, cx)),
                        ),
                )
            })
            .into_any_element()
    }
}
