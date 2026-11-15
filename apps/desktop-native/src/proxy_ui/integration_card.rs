//! Proxy integration presentation; preview/apply ownership remains in integration.
use super::*;
use crate::{parity::Observe, providers::controls};
use aipass_agent_protocol::{ToolConfigProxyRequest, ToolConfigTool as Tool};
use aipass_desktop_native::{agent_management::choices, integration::catalog};
use gpui::{div, px, FontWeight};

impl Proxy {
    pub(super) fn integration_card(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let t = self.theme;
        let Some(config) = &self.config else {
            return div().into_any_element();
        };
        let route = config
            .routes
            .iter()
            .find(|r| Some(r.id) == self.selected && r.enabled)
            .or_else(|| config.routes.iter().find(|r| r.enabled));
        let tools = catalog::AUTOMATIC
            .into_iter()
            .chain([
                (Tool::VscodeChat, "VS Code Chat"),
                (Tool::Cline, "Cline"),
                (Tool::Trae, "TRAE"),
                (Tool::Qoder, "Qoder"),
                (Tool::Zcode, "ZCode"),
                (Tool::JetbrainsCopilot, "JetBrains Copilot"),
            ])
            .filter(|(tool, _)| route.is_some() && catalog::installed(tool, &self.detections))
            .collect::<Vec<_>>();
        let mut body = admin_ui::stack().gap(px(0.)).py(px(4.));
        if tools.is_empty() {
            body = body.child(div().min_h(px(56.)).px(px(16.)).py(px(10.)).child(
                self.locale.text(if self.scanned {
                    "agents.empty"
                } else {
                    "common.loading"
                }),
            ));
        } else {
            let mut context = admin_ui::stack()
                .gap(px(10.))
                .px(px(16.))
                .pt(px(12.))
                .pb(px(14.))
                .border_b_1()
                .border_color(t.divider)
                .child(
                    admin_ui::hint(self.locale.text("server.integrateDesc"), t)
                        .line_height(px(16.8)),
                );
            if let Some(route) = route {
                let id = route.id;
                let endpoint = integration::endpoint(config, route);
                let actions = |children: Vec<gpui::AnyElement>| {
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(4.))
                        .flex_shrink_0()
                        .children(children)
                };
                context = context.child(self.integration_value(
                    "server.endpoint",
                    endpoint.clone(),
                    actions(vec![controls::icon(
                            "proxy-copy-endpoint",
                            "copy",
                            self.locale.text("server.copyEndpoint"),
                            t,
                        )
                        .size(px(26.))
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(endpoint.clone()))
                        })
                        .into_any_element()]),
                ));
                let mut token_actions = vec![];
                if !route.token.is_empty() {
                    token_actions.push(
                        controls::icon(
                            "proxy-copy-token",
                            "copy",
                            self.locale.text("server.copyToken"),
                            t,
                        )
                        .size(px(26.))
                        .disabled(self.busy() || self.copying)
                        .on_click(cx.listener(move |s, _, w, cx| s.copy_token(id, w, cx)))
                        .into_any_element(),
                    );
                }
                token_actions.push(
                    controls::icon(
                        "proxy-rotate",
                        "rotate-cw",
                        self.locale.text("server.rotateToken"),
                        t,
                    )
                    .size(px(26.))
                    .disabled(self.busy())
                    .on_click(cx.listener(move |s, _, _, cx| {
                        s.apply(AgentRequest::ServerTokenRotate { route_id: id }, cx)
                    }))
                    .into_any_element(),
                );
                context = context.child(self.integration_value(
                    "server.token",
                    if route.token.is_empty() {
                        self.locale.text("server.noToken")
                    } else {
                        route.token.clone()
                    },
                    actions(token_actions),
                ));
            } else {
                context = context.child(admin_ui::hint(self.locale.text("server.noneActive"), t));
            }
            body = body.child(context.observe("proxy-integration-context"));
            let total = tools.len();
            for (index, (tool, name)) in tools.into_iter().enumerate() {
                let tool_icon = catalog::icon(&tool);
                let reason = route.and_then(|r| integration::unavailable(&tool, r));
                let disabled =
                    self.busy() || reason.is_some() || route.is_none_or(|r| r.token.is_empty());
                let mut side = div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .flex_shrink_0()
                    .child(
                        gpui::svg()
                            .path("check")
                            .size(px(13.))
                            .text_color(t.success),
                    );
                if let Some(route) = route {
                    let id = route.id;
                    if choices::guide(&tool).is_some() {
                        side =
                            side.child(
                                self.integration_action(
                                    &format!("proxy-setup-{index}"),
                                    "integration.setupAction",
                                )
                                .disabled(disabled)
                                .on_click(cx.listener(
                                    move |s, _, w, cx| s.show_setup(tool.clone(), id, w, cx),
                                )),
                            );
                    } else {
                        let request = ToolConfigProxyRequest {
                            tool: catalog::tool_id(&tool),
                            route_id: id,
                        };
                        let preview = request.clone();
                        side = side
                            .child(
                                controls::icon(
                                    format!("proxy-view-{index}"),
                                    "eye",
                                    self.locale.text("providerDetail.preview"),
                                    t,
                                )
                                .size(px(26.))
                                .disabled(disabled)
                                .on_click(cx.listener(
                                    move |s, _, w, cx| {
                                        s.previous_focus = w.focused(cx);
                                        s.plan = Some(preview.clone());
                                        s.preview_readonly = true;
                                        s.request(
                                            Operation::ToolPreview(preview.clone()),
                                            false,
                                            cx,
                                        );
                                    },
                                )),
                            )
                            .child(
                                self.integration_action(
                                    &format!("proxy-preview-{index}"),
                                    "server.writeConfig",
                                )
                                .disabled(disabled)
                                .on_click(cx.listener(
                                    move |s, _, w, cx| {
                                        s.previous_focus = w.focused(cx);
                                        s.plan = Some(request.clone());
                                        s.preview_readonly = false;
                                        s.request(
                                            Operation::ToolPreview(request.clone()),
                                            false,
                                            cx,
                                        );
                                    },
                                )),
                            );
                    }
                }
                body = body.child(
                    div()
                        .px(px(16.))
                        .py(px(10.))
                        .min_h(px(56.))
                        .flex_shrink_0()
                        .when(index + 1 < total, |s| {
                            s.border_b_1().border_color(t.divider)
                        })
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
                                                    gpui::svg()
                                                        .path(tool_icon)
                                                        .size(px(17.))
                                                        .text_color(t.secondary),
                                                ),
                                        )
                                        .child(
                                            admin_ui::stack()
                                                .gap(px(2.))
                                                .flex_1()
                                                .min_w_0()
                                                .child(
                                                    div()
                                                        .text_size(px(13.))
                                                        .line_height(px(16.))
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .truncate()
                                                        .child(name),
                                                )
                                                .when_some(reason, |s, key| {
                                                    s.child(
                                                        div()
                                                            .max_w(px(300.))
                                                            .text_size(px(11.))
                                                            .line_height(px(14.85))
                                                            .text_color(t.tertiary)
                                                            .child(self.locale.text(key)),
                                                    )
                                                }),
                                        ),
                                )
                                .child(side),
                        )
                        .observe(format!("proxy-integration-tool-{index}")),
                );
            }
        }
        let frame = self
            .integration_disclosure
            .sample(self.integration_open, window, cx);
        div()
            .pixel_snap(false)
            .flex_shrink_0()
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
                    .border_color(if self.integration_open {
                        t.divider
                    } else {
                        gpui::rgba(0)
                    })
                    .child(
                        gpui_base::Button::new("proxy-integration-toggle")
                            .flex_1()
                            .min_w_0()
                            .h(px(42.2))
                            .px(px(16.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .accessibility_label(self.locale.text("server.integrate"))
                            .aria_expanded(self.integration_open)
                            .text_size(px(13.))
                            .line_height(px(18.2))
                            .font_weight(FontWeight::SEMIBOLD)
                            .hover(move |s| s.bg(t.surface2))
                            .focus_visible(move |s| s.shadow(t.outline(t.focus_ring, 2., 0.)))
                            .child(self.locale.text("server.integrate"))
                            .child(frame.chevron(t.secondary, 15.))
                            .on_click(cx.listener(|s, _, _, cx| {
                                s.integration_open = !s.integration_open;
                                cx.notify();
                            })),
                    )
                    .child(
                        div().pr(px(16.)).child(
                            controls::icon(
                                "proxy-tools-scan",
                                "scan-search",
                                self.locale.text("integration.scanInstallations"),
                                t,
                            )
                            .size(px(26.))
                            .disabled(self.busy())
                            .on_click(
                                cx.listener(|s, _, _, cx| s.request(Operation::Tools, false, cx)),
                            ),
                        ),
                    ),
            )
            .child(frame.body("proxy-integration-body", body))
            .observe("proxy-integration-card")
            .into_any_element()
    }
    fn integration_value(&self, label: &str, value: String, actions: gpui::Div) -> gpui::Div {
        let t = self.theme;
        let scroll = if label == "server.endpoint" {
            &self.integration_endpoint_scroll
        } else {
            &self.integration_token_scroll
        };
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                div()
                    .w(px(72.))
                    .flex_shrink_0()
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.tertiary)
                    .child(self.locale.text(label)),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .rounded(px(6.))
                    .overflow_hidden()
                    .child(
                        div()
                            .id(format!("proxy-integration-{label}"))
                            .track_scroll(scroll)
                            .overflow_x_scroll()
                            .overflow_y_hidden()
                            .flex()
                            .px(px(8.))
                            .py(px(6.))
                            .rounded(px(6.))
                            .bg(t.surface2)
                            .text_size(px(12.))
                            .line_height(px(15.))
                            .text_color(t.text)
                            .font_family(".AppleSystemUIFontMonospaced")
                            .whitespace_nowrap()
                            .child(div().flex_shrink_0().child(gpui_base::SelectableText::new(
                                format!("proxy-integration-value-{label}"),
                                value,
                            ))),
                    )
                    .child(crate::scroll_fade::horizontal(t.surface, scroll.clone()))
                    .observe(format!("proxy-integration-value-{label}")),
            )
            .child(actions.observe(format!("proxy-integration-actions-{label}")))
    }
    fn integration_action(&self, id: &str, label: &str) -> gpui_base::Button {
        self.button(id, label, false)
            .h(px(26.))
            .px(px(9.))
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
    }
}
