use super::*;
use crate::parity::Observe;
use crate::providers::controls::{button, icon};
use gpui::{div, px, svg, Anchor, AnyElement, FontWeight};
use gpui_base::{Button, Popover};

impl Integrations {
    fn picker(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
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
        let empty = options.is_empty();
        let owner = cx.entity().downgrade();
        let mut focus = self.picker_focus.clone();
        let scroll_state = window.use_keyed_state("integration-picker-scroll", cx, |_, _| {
            gpui::ScrollHandle::new()
        });
        let scroll = scroll_state.read(cx).clone();
        focus.scroll = Some(scroll.clone());
        let site_protocol = entry.interface_type.clone();
        let site_group = entry
            .gateway
            .as_ref()
            .and_then(|gateway| gateway.group.clone());
        let open = self.picker_open && !self.busy() && !empty;
        let motion = crate::menu::motion::sample("integration-picker-motion", open, window, cx);
        let origin = crate::menu::motion::Origin::new(false);
        let track_origin = origin.clone();
        let selected_index = options
            .iter()
            .position(|s| Some(&s.id) == self.secret_id.as_ref());
        let initial_focus = focus.clone();
        let keyboard_focus = initial_focus.clone();
        let bounds = self.picker_bounds.clone();
        let width = bounds.get().size.width.max(px(1.));
        let picker = Popover::new("integration-key-picker")
            .on_position(move |position, trigger| track_origin.resolve(position, trigger))
            .open(open)
            .retain_content(motion.visible)
            .track_focus(&focus.container)
            .on_open_change(cx.listener(move |this, open: &bool, window, cx| {
                this.picker_open = *open && !this.busy();
                if this.picker_open {
                    initial_focus.focus_item(selected_index.unwrap_or(0), window, cx);
                }
                cx.notify();
            }))
            .anchor(Anchor::TopLeft)
            .offset(px(4.))
            .trigger(
                Button::new("integration-key-trigger")
                    .track_focus(&focus.trigger)
                    .disabled(self.busy() || empty)
                    .w_full()
                    .min_h(px(60.))
                    .px(px(11.))
                    .py(px(9.))
                    .border_1()
                    .border_color(t.border)
                    .rounded(px(8.))
                    .bg(t.surface)
                    .cursor_pointer()
                    .hover(move |s| {
                        s.bg(t.surface2)
                            .border_color(if open { t.accent } else { t.border_strong })
                    })
                    .focus_visible(move |s| {
                        s.border_color(t.accent)
                            .shadow(t.outline(t.focus_ring, 2., 2.))
                    })
                    .when(open, |el| {
                        el.border_color(t.accent)
                            .shadow(t.outline(t.focus_ring, 2., 2.))
                    })
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .text_color(t.text)
                    .accessibility_label(self.locale.text("integration.credential"))
                    .on_key_down(cx.listener(
                        move |this, event: &gpui::KeyDownEvent, window, cx| {
                            if !this.busy()
                                && !this.picker_open
                                && !empty
                                && matches!(event.keystroke.key.as_str(), "down" | "up")
                            {
                                this.picker_open = true;
                                let focus = keyboard_focus.clone();
                                window.on_next_frame(move |window, cx| {
                                    focus.focus_item(selected_index.unwrap_or(0), window, cx)
                                });
                                cx.stop_propagation();
                                cx.notify();
                            }
                        },
                    ))
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
                use crate::menu::select;
                let close = owner.clone();
                let menu = focus.surface(
                    "integration-key-options",
                    focus.trigger.clone(),
                    focus.trigger.clone(),
                    move |_, cx| {
                        let _ = close.update(cx, |this, cx| {
                            this.picker_open = false;
                            cx.notify();
                        });
                    },
                );
                let content = select::surface(menu, width, t)
                    .child(
                        div()
                            .id("integration-select-viewport")
                            .flex_shrink_0()
                            .max_h(px(278.))
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
                            .flex()
                            .flex_col()
                            .children(options.iter().enumerate().map(|(i, secret)| {
                                let owner = owner.clone();
                                let id = secret.id.clone();
                                let active = Some(i) == selected_index;
                                select::item(("integration-key", i), active, t)
                                    .disabled(!open)
                                    .track_focus(&focus.items[i])
                                    .accessibility_label(secret.label.clone())
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap(px(5.))
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
                                                            .text_size(px(12.))
                                                            .font_weight(FontWeight::SEMIBOLD)
                                                            .child(secret.label.clone()),
                                                    )
                                                    .child(
                                                        div()
                                                            .max_w(px(110.))
                                                            .text_ellipsis()
                                                            .font_family("Menlo")
                                                            .text_size(px(10.))
                                                            .text_color(t.tertiary)
                                                            .child(secret.masked.clone()),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(5.))
                                                    .min_w_0()
                                                    .overflow_hidden()
                                                    .child(
                                                        Self::protocol_badge(
                                                            secret
                                                                .interface_type
                                                                .as_ref()
                                                                .unwrap_or(&site_protocol),
                                                            t,
                                                        )
                                                        .observe(format!(
                                                            "menu-integration-protocol-{i}"
                                                        )),
                                                    )
                                                    .when_some(
                                                        secret
                                                            .group
                                                            .as_ref()
                                                            .or(site_group.as_ref()),
                                                        |el, group| {
                                                            el.child(
                                                                div()
                                                                    .h(px(21.))
                                                                    .min_w_0()
                                                                    .px(px(8.))
                                                                    .rounded_full()
                                                                    .border_1()
                                                                    .border_color(t.border)
                                                                    .bg(t.surface)
                                                                    .text_color(t.secondary)
                                                                    .text_size(px(10.))
                                                                    .line_height(px(10.))
                                                                    .font_weight(FontWeight::MEDIUM)
                                                                    .flex()
                                                                    .items_center()
                                                                    .gap(px(4.))
                                                                    .child(
                                                                        svg()
                                                                            .path("layers-2")
                                                                            .size(px(10.))
                                                                            .flex_shrink_0()
                                                                            .text_color(
                                                                                t.secondary,
                                                                            ),
                                                                    )
                                                                    .child(
                                                                        div()
                                                                            .min_w_0()
                                                                            .text_ellipsis()
                                                                            .child(group.clone()),
                                                                    ),
                                                            )
                                                        },
                                                    ),
                                            ),
                                    )
                                    .child(select::indicator(active, t))
                                    .on_click(move |_, window, cx| {
                                        let _ = owner.update(cx, |this, cx| {
                                            if !this.picker_open || this.busy() {
                                                return;
                                            }
                                            this.select_secret(id.clone(), cx);
                                            this.picker_focus.trigger.focus(window, cx);
                                        });
                                    })
                                    .observe(format!("menu-integration-option-{i}"))
                            })),
                    )
                    .observe("menu-integration-content");
                motion.wrap_positioned(crate::menu::select::fade(content, scroll, t), origin)
            });
        div()
            .on_children_prepainted(move |children, _, cx| {
                if let Some(rect) = children.last() {
                    bounds.set(*rect);
                    crate::parity::record_bounds("menu-integration-trigger", *rect, cx);
                }
            })
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
    fn protocol_badge(protocol: &aipass_provider_registry::InterfaceType, t: Theme) -> gpui::Div {
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
            .flex_shrink_0()
            .h(px(21.))
            .px(px(8.))
            .rounded_full()
            .border_1()
            .border_color(t.accent.opacity(0.18))
            .bg(t.accent_soft)
            .text_color(t.accent)
            .text_size(px(10.))
            .line_height(px(10.))
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
                                    self.locale.text(
                                        if self
                                            .entry
                                            .as_ref()
                                            .and_then(catalog::native_tool)
                                            .is_some()
                                        {
                                            "integration.switchAccount"
                                        } else {
                                            "server.writeConfig"
                                        },
                                    ),
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
            .when(
                tool == ToolConfigTool::Codex
                    && self.entry.as_ref().and_then(catalog::native_tool).is_none(),
                |el| {
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
                                        .map(
                                            |(i, mode)| {
                                                let active = self.codex_mode == mode;
                                                Button::new(("integration-codex-mode", i))
                                                    .disabled(self.busy())
                                                    .h(px(24.))
                                                    .px(px(8.))
                                                    .rounded(px(4.))
                                                    .bg(if active {
                                                        t.surface
                                                    } else {
                                                        gpui::rgba(0)
                                                    })
                                                    .text_size(px(11.))
                                                    .text_color(t.secondary)
                                                    .child(if i == 0 {
                                                        "auth.json".into()
                                                    } else {
                                                        self.locale.text(
                                                            "providerDetail.codexModeExperimental",
                                                        )
                                                    })
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.set_mode(mode.clone(), cx)
                                                    }))
                                            },
                                        ),
                                    ),
                            ),
                    )
                },
            )
            .when_some(status, |el, status| {
                el.child(
                    div()
                        .ml(px(42.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_size(px(11.))
                        .text_color(t.secondary)
                        .child(div().flex_1().min_w_0().child(self.status_label(status)))
                        .when(
                            status.mode == Some(aipass_agent_protocol::ToolConfigMode::Official)
                                && ["login_required", "renewal_required"]
                                    .contains(&status.state.as_str()),
                            |el| {
                                let tool = tool.clone();
                                el.child(
                                    button(
                                        ("integration-relogin", index),
                                        self.locale.text("integration.relogin"),
                                        false,
                                        t,
                                    )
                                    .h(px(26.))
                                    .text_size(px(12.))
                                    .disabled(self.busy())
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| {
                                            this.reconnect(&tool, cx)
                                        }),
                                    ),
                                )
                            },
                        )
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
    fn status_label(&self, status: &ToolConfigStatus) -> String {
        if status.mode == Some(aipass_agent_protocol::ToolConfigMode::Official) {
            if let Some(identity) = status.account_identity.as_deref().filter(|s| !s.is_empty()) {
                return self
                    .locale
                    .text("integration.usingOfficialSubscription")
                    .replace(
                        "{account}",
                        &aipass_desktop_native::subscriptions::presentation::readable(identity),
                    );
            }
        }
        format!(
            "{}: {}",
            self.locale.text("integration.current"),
            status
                .entry_title
                .clone()
                .unwrap_or_else(|| self.locale.text("integration.unmanaged"))
        )
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let disclosure = self.disclosure.sample(self.expanded, window, cx);
        if !self.is_visible() {
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
                            .aria_expanded(self.expanded)
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
                            .child(disclosure.chevron(t.tertiary, 15.))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.expanded = !this.expanded;
                                cx.notify();
                            }))
                            .observe("menu-integration-expand"),
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
            .child(
                disclosure.body(
                    "integration-content",
                    div()
                        .flex()
                        .flex_col()
                        .when(
                            !tools.is_empty()
                                && self.entry.as_ref().and_then(catalog::native_tool).is_none(),
                            |el| {
                                el.child(
                                    div()
                                        .px(px(16.))
                                        .pt(px(12.))
                                        .pb(px(14.))
                                        .border_b_1()
                                        .border_color(t.divider)
                                        .child(self.picker(window, cx)),
                                )
                            },
                        )
                        .when(!self.error.is_empty(), |el| {
                            el.child(self.banner(self.error.clone(), t.danger))
                        })
                        .when(!self.message.is_empty(), |el| {
                            el.child(self.banner(self.message.clone(), t.success))
                        })
                        .when(self.login_plan.is_some(), |el| {
                            el.child(
                                div()
                                    .p(px(10.))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .bg(t.warning.opacity(0.08))
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_size(px(12.))
                                            .text_color(t.warning)
                                            .child(self.locale.text("integration.loginRequired")),
                                    )
                                    .child(
                                        button(
                                            "integration-login",
                                            self.locale.text("integration.relogin"),
                                            false,
                                            t,
                                        )
                                        .disabled(self.busy())
                                        .on_click(
                                            cx.listener(|this, _, window, cx| {
                                                this.open_login(window, cx)
                                            }),
                                        ),
                                    ),
                            )
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
                ),
            )
            .into_any_element()
    }
}
