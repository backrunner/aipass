use super::*;
use crate::{
    parity::Observe,
    providers::controls::{button, icon},
};
use gpui::{div, px, svg, AnyElement, FontWeight};
use gpui_base::Button;

impl Integrations {
    pub(super) fn tool_row(
        &self,
        index: usize,
        tool: ToolConfigTool,
        last: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
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
            .when(!last, |el| el.border_b_1())
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
                                            .text_color(t.secondary)
                                            .observe(format!("integration-tool-icon-{index}")),
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
                            .child(
                                svg()
                                    .path("check")
                                    .size(px(13.))
                                    .text_color(t.success)
                                    .observe(format!("integration-installed-{index}")),
                            )
                            .child(
                                icon(
                                    ("integration-preview", index),
                                    "eye",
                                    self.locale.text("providerDetail.preview"),
                                    t,
                                )
                                .size(px(26.))
                                .disabled(!enabled)
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
                    // CSS 1fr columns share the widest max-content label. Keep
                    // that intrinsic extent fractional until the grid's edges snap.
                    let labels = [
                        "auth.json".to_owned(),
                        self.locale.text("providerDetail.codexModeExperimental"),
                    ];
                    let mut font = window.text_style().font();
                    font.weight = FontWeight::MEDIUM;
                    let width = labels
                        .iter()
                        .map(|label| {
                            window
                                .text_system()
                                .shape_line(
                                    label.clone().into(),
                                    px(12.),
                                    &[gpui::TextRun {
                                        len: label.len(),
                                        font: font.clone(),
                                        color: t.text.into(),
                                        background_color: None,
                                        underline: None,
                                        strikethrough: None,
                                    }],
                                    None,
                                )
                                .width
                        })
                        .fold(px(0.), |a, b| a.max(b))
                        * 2.
                        + px(56.);
                    el.child(
                        div()
                            .ml(px(42.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                div()
                                    .min_w_0()
                                    .text_size(px(11.))
                                    .line_height(px(13.))
                                    .text_color(t.tertiary)
                                    .child(self.locale.text("providerDetail.codexAuthMode"))
                                    .observe("integration-mode-label"),
                            )
                            .child(
                                div()
                                    .id("integration-mode-group")
                                    .role(gpui::Role::TabList)
                                    .aria_label(self.locale.text("providerDetail.codexAuthMode"))
                                    .pixel_snap(false)
                                    .p(px(2.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(t.border)
                                    .bg(t.surface2)
                                    .w(width)
                                    .grid()
                                    .grid_cols_min_content(2)
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
                                                    .disabled(self.pending.is_some())
                                                    .role(gpui::Role::Tab)
                                                    .aria_selected(active)
                                                    .track_focus(&self.mode_focus[i])
                                                    .tab_index(if active { 0 } else { -1 })
                                                    .h(px(26.))
                                                    .px(px(12.))
                                                    .rounded(px(4.))
                                                    .bg(if active {
                                                        t.surface
                                                    } else {
                                                        gpui::rgba(0)
                                                    })
                                                    .text_size(px(12.))
                                                    .line_height(px(14.))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .whitespace_nowrap()
                                                    .text_color(if active {
                                                        t.text
                                                    } else {
                                                        t.secondary
                                                    })
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .when(active, |el| {
                                                        el.shadow(vec![gpui_base::box_shadow(
                                                            0.,
                                                            1.,
                                                            crate::theme::shadow_blur(2.),
                                                            0.,
                                                            gpui::rgb(0x0f1110)
                                                                .opacity(0.08)
                                                                .into(),
                                                        )])
                                                    })
                                                    .hover(move |s| s.text_color(t.text))
                                                    .focus_visible(move |s| {
                                                        s.shadow(t.outline(t.accent, 2., -2.))
                                                    })
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
                                                    .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, window, cx| {
                                                        let mods = event.keystroke.modifiers;
                                                        if mods.control || mods.platform || mods.alt || this.pending.is_some() {
                                                            return;
                                                        }
                                                        let next = match event.keystroke.key.as_str() {
                                                            "left" | "right" => 1 - i,
                                                            "home" => 0,
                                                            "end" => 1,
                                                            _ => return,
                                                        };
                                                        cx.stop_propagation();
                                                        this.mode_focus[next].focus(window, cx);
                                                        this.set_mode(if next == 0 { CodexApiKeyMode::AuthJson } else { CodexApiKeyMode::ExperimentalBearerToken }, cx);
                                                    }))
                                                    .observe(format!("integration-mode-{i}"))
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
                        .line_height(px(13.))
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
            .observe(format!("integration-tool-{index}"))
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
    pub(super) fn banner(&self, text: String, color: gpui::Rgba) -> impl IntoElement {
        div()
            .p(px(10.))
            .rounded(px(8.))
            .bg(color.opacity(0.08))
            .text_color(color)
            .text_size(px(12.))
            .child(text)
    }
}
