use super::*;
use crate::{
    parity::Observe,
    providers::controls::{base_button, button},
};
use aipass_desktop_native::integration::catalog;
use gpui::{div, px, svg, AnimationExt, AnyElement, Div, FontWeight};
use gpui_base::Button;
impl Agents {
    fn tool_icon(&self, tool: &Tool, size: f32) -> Div {
        div()
            .size(px(36.))
            .flex_shrink_0()
            .rounded(px(8.))
            .bg(self.theme.surface2)
            .flex()
            .items_center()
            .justify_center()
            .child(
                svg()
                    .path(catalog::icon(tool))
                    .size(px(size))
                    .text_color(self.theme.text),
            )
    }
    fn account_label(&self, status: Option<&ToolConfigStatus>) -> String {
        if status.is_some_and(|s| s.state == "not_logged_in") {
            return self.locale.text("integration.state.not_logged_in");
        }
        let entry = status.and_then(|s| self.entries.iter().find(|e| Some(e.id) == s.entry_id));
        if let Some(entry) = entry {
            return if entry.credential_kind == aipass_provider_registry::CredentialKind::OAuth {
                aipass_desktop_native::subscriptions::presentation::account(entry, &|key| {
                    self.locale.text(key)
                })
                .account
            } else {
                entry.title.clone()
            };
        }
        status
            .and_then(|s| s.entry_title.clone().or(s.account_identity.clone()))
            .unwrap_or_else(|| self.locale.text("integration.unmanaged"))
    }
    fn pane(&self) -> Div {
        div()
            .relative()
            .h_full()
            .min_w_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(14.))
            .bg(self.theme.surface)
    }
    fn empty(&self, detail: bool) -> Div {
        div()
            .px(px(20.))
            .py(px(48.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(12.))
            .text_center()
            .text_size(px(12.))
            .line_height(px(14.4))
            .text_color(self.theme.secondary)
            .child(
                svg()
                    .path("terminal")
                    .size(px(28.))
                    .text_color(self.theme.secondary),
            )
            .child(
                div()
                    .font_weight(if detail {
                        FontWeight::NORMAL
                    } else {
                        FontWeight::SEMIBOLD
                    })
                    .child(self.locale.text(if self.loaded {
                        "agents.empty"
                    } else {
                        "agents.scanning"
                    })),
            )
            .when(self.loaded && !detail, |el| {
                el.child(self.locale.text("agents.emptyDetail"))
            })
    }
    fn banner(&self, text: String, success: bool) -> Div {
        let color = if success {
            self.theme.success
        } else {
            self.theme.danger
        };
        div()
            .px(px(12.))
            .py(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(gpui::transparent_black())
            .bg(color.opacity(0.08))
            .text_size(px(12.))
            .line_height(px(16.8))
            .text_color(color)
            .child(text)
    }
    fn list(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme;
        let tools = choices::tools(&self.detections);
        let body = div()
            .id("agents-list-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.list_scroll)
            .p(px(7.))
            .flex()
            .flex_col()
            .when(!self.error.is_empty() && tools.is_empty(), |el| {
                el.child(self.banner(self.error.clone(), false))
            })
            .when(tools.is_empty(), |el| el.child(self.empty(false)))
            .children(tools.iter().enumerate().map(|(i, tool)| {
                let selected = Some(tool) == self.selected.as_ref();
                let target = tool.clone();
                Button::new(("agent-row", i))
                    .w_full()
                    .flex_shrink_0()
                    .track_focus(&self.row_focus[i])
                    .accessibility_label(choices::name(tool))
                    .aria_selected(selected)
                    .px(px(6.))
                    .py(px(12.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(11.))
                    .text_left()
                    .text_color(t.text)
                    .when(selected, |el| el.bg(t.accent_soft))
                    .when(!selected, |el| el.hover(move |s| s.bg(t.surface2)))
                    .focus_visible(move |s| s.shadow(t.outline(t.focus_ring, 2., 0.)))
                    .child(self.tool_icon(tool, 23.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(15.6))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(choices::name(tool)),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .line_height(px(13.2))
                                    .font_weight(FontWeight::NORMAL)
                                    .text_color(t.secondary)
                                    .text_ellipsis()
                                    .child(self.account_label(
                                        self.statuses.iter().find(|s| &s.tool == tool),
                                    )),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.choose(target.clone(), cx)))
                    .observe(format!("agent-row-{i}"))
            }));
        self.pane()
            .w(px(width))
            .flex_shrink_0()
            .child(
                div()
                    .pt(px(38.))
                    .pl(px(13.))
                    .pr(px(8.))
                    .pb(px(11.))
                    .border_b_1()
                    .border_color(t.divider)
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(self.locale.text("agents.title")),
                            )
                            .when(!tools.is_empty(), |el| {
                                el.child(
                                    div()
                                        .min_w(px(20.))
                                        .h(px(20.))
                                        .px(px(5.))
                                        .rounded(px(6.))
                                        .bg(t.surface2)
                                        .text_color(t.tertiary)
                                        .text_size(px(11.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(tools.len().to_string()),
                                )
                            }),
                    )
                    .child(
                        Button::new("agents-scan")
                            .accessibility_label(self.locale.text("integration.scanInstallations"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(8.))
                            .hover(move |s| s.bg(t.accent_soft))
                            .focus_visible(move |s| s.shadow(t.outline(t.focus_ring, 2., 1.)))
                            .child(if self.pending == Some(Kind::Scan) {
                                svg()
                                    .path("refresh-cw")
                                    .size(px(14.))
                                    .text_color(t.secondary)
                                    .with_animation(
                                        "agents-scan-spin",
                                        gpui::Animation::new(std::time::Duration::from_secs(1))
                                            .repeat(),
                                        |el, p| {
                                            el.with_transformation(gpui::Transformation::rotate(
                                                gpui::percentage(p),
                                            ))
                                        },
                                    )
                                    .into_any_element()
                            } else {
                                svg()
                                    .path("refresh-cw")
                                    .size(px(14.))
                                    .text_color(t.secondary)
                                    .into_any_element()
                            })
                            .size(px(26.))
                            .disabled(self.pending.is_some() || self.preview.is_some())
                            .on_click(cx.listener(|this, _, _, cx| this.scan(cx))),
                    )
                    .observe("agents-toolbar"),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(body)
                    .child(crate::scroll_fade::fade(
                        t.surface,
                        self.list_scroll.clone(),
                    )),
            )
            .observe("agents-list-pane")
            .into_any_element()
    }
    fn detail(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme;
        let Some(tool) = self.selected.clone() else {
            return self
                .pane()
                .flex_1()
                .child(self.empty(true))
                .observe("agents-detail")
                .into_any_element();
        };
        let detection = self
            .detections
            .iter()
            .find(|d| d.tool == catalog::tool_id(&tool));
        let status = self.status().cloned();
        let switching = matches!(
            self.pending,
            Some(Kind::Preview | Kind::Apply | Kind::Restore)
        );
        let switch_label = self.locale.text(if choices::guide(&tool).is_some() {
            "integration.setupAction"
        } else {
            "agents.switch"
        });
        let body = div()
            .id("agents-body-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.detail_scroll)
            .p(px(18.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .when(!self.message.is_empty(), |el| {
                el.child(self.banner(self.message.clone(), true))
            })
            .when(!self.error.is_empty(), |el| {
                el.child(self.banner(self.error.clone(), false))
            })
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap(px(8.))
                    .min_w_0()
                    .flex_shrink_0()
                    .child(div().flex_1().min_w_0().child(self.picker.clone()))
                    .child(
                        base_button("agents-switch", false, t)
                            .relative()
                            .accessibility_label(switch_label.clone())
                            .child(
                                div()
                                    .opacity(if switching { 0. } else { 1. })
                                    .child(switch_label),
                            )
                            .when(switching, |el| {
                                el.child(
                                    svg()
                                        .path("spinner")
                                        .size(px(12.))
                                        .text_color(t.text)
                                        .absolute()
                                        .top(gpui::relative(0.5))
                                        .left(gpui::relative(0.5))
                                        .mt(px(-6.))
                                        .ml(px(-6.))
                                        .with_animation(
                                            "agents-switch-spinner",
                                            gpui::Animation::new(std::time::Duration::from_millis(
                                                700,
                                            ))
                                            .repeat(),
                                            |el, p| {
                                                el.with_transformation(
                                                    gpui::Transformation::rotate(gpui::percentage(
                                                        p,
                                                    )),
                                                )
                                            },
                                        ),
                                )
                            })
                            .h(px(34.))
                            .px(px(9.))
                            .text_size(px(12.))
                            .line_height(px(12.))
                            .track_focus(&self.switch_focus)
                            .disabled(!self.can_switch(cx))
                            .on_click(cx.listener(|this, _, window, cx| this.prepare(window, cx))),
                    )
                    .observe("agents-switch-controls"),
            )
            .when(
                detection.is_none_or(|d| !d.binary_found && !d.extension_found),
                |el| el.child(self.note("agents.configOnly")),
            )
            .when(self.needs_runtime(), |el| {
                el.child(self.note("agents.cursorRuntimeRequired"))
            })
            .when_some(
                status.as_ref().filter(|s| {
                    !matches!(s.state.as_str(), "ready" | "unmanaged" | "not_logged_in")
                }),
                |el, status| {
                    el.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .text_color(t.warning)
                            .text_size(px(11.))
                            .line_height(px(13.2))
                            .child(
                                div().flex_1().child(
                                    self.locale
                                        .text(&format!("integration.state.{}", status.state)),
                                ),
                            )
                            .when(status.operation_id.is_some(), |el| {
                                el.child(
                                    button(
                                        "agents-restore",
                                        self.locale.text("integration.restore"),
                                        false,
                                        t,
                                    )
                                    .disabled(self.pending.is_some())
                                    .on_click(cx.listener(|this, _, _, cx| this.restore(cx))),
                                )
                            }),
                    )
                },
            )
            .when_some(
                status.as_ref().filter(|s| !s.overrides.is_empty()),
                |el, status| {
                    el.child(
                        div()
                            .text_color(t.warning)
                            .text_size(px(11.))
                            .line_height(px(13.2))
                            .child(
                                self.locale
                                    .text("integration.environmentOverrides")
                                    .replace("{keys}", &status.overrides.join(", ")),
                            ),
                    )
                },
            )
            .child(if let Some(entry) = self.entry() {
                self.account(&entry, window, cx)
            } else {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .line_height(px(14.4))
                    .text_color(t.tertiary)
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.secondary)
                            .child(self.account_label(status.as_ref())),
                    )
                    .when(
                        status.as_ref().is_none_or(|s| s.state != "not_logged_in"),
                        |el| el.child(self.locale.text("agents.unlinkedUsage")),
                    )
                    .into_any_element()
            });
        self.pane()
            .flex_1()
            .child(
                div()
                    .px(px(20.))
                    .py(px(18.))
                    .min_h(px(76.))
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(t.divider)
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(self.tool_icon(&tool, 25.))
                    .child(
                        div()
                            .text_size(px(17.))
                            .line_height(px(20.4))
                            .font_weight(FontWeight(650.))
                            .child(choices::name(&tool)),
                    )
                    .observe("agents-detail-header"),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(body)
                    .child(crate::scroll_fade::fade(
                        t.surface,
                        self.detail_scroll.clone(),
                    )),
            )
            .observe("agents-detail")
            .into_any_element()
    }
    fn note(&self, key: &str) -> Div {
        div()
            .text_size(px(11.))
            .line_height(px(13.2))
            .text_color(self.theme.tertiary)
            .child(self.locale.text(key))
    }
    fn card(&self, title: &str, body: AnyElement) -> Div {
        div()
            .flex_shrink_0()
            .rounded(px(12.))
            .border_1()
            .border_color(self.theme.border)
            .bg(self.theme.surface)
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(16.))
                    .py(px(12.))
                    .border_b_1()
                    .border_color(self.theme.divider)
                    .text_size(px(13.))
                    .line_height(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.locale.text(title)),
            )
            .child(div().px(px(16.)).py(px(12.)).child(body))
    }
    fn metrics(&self, rows: Vec<(&str, String)>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .children(rows.chunks(2).map(|pair| {
                div()
                    .flex()
                    .gap(px(14.))
                    .children(pair.iter().map(|(label, value)| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(self.note(label).line_height(px(13.)))
                            .child(
                                div()
                                    .text_size(px(17.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(value.clone()),
                            )
                    }))
            }))
            .into_any_element()
    }
    fn account(
        &mut self,
        entry: &EntrySummary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let oauth = entry.credential_kind == aipass_provider_registry::CredentialKind::OAuth;
        let secret = self.status().and_then(|s| s.secret_id.clone());
        let mut body = div()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(14.))
            .min_w_0();
        if oauth {
            body = body.child(
                self.subscriptions
                    .update(cx, |view, cx| view.account_card(window, cx)),
            );
        } else {
            body = body.child(
                self.card(
                    "agents.currentCredential",
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(13.))
                                .line_height(px(16.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(entry.title.clone()),
                        )
                        .child(
                            div()
                                .mt(px(8.))
                                .text_size(px(11.))
                                .line_height(px(16.5))
                                .text_color(self.theme.tertiary)
                                .child(
                                    entry
                                        .secret_refs
                                        .iter()
                                        .find(|s| Some(&s.id) == secret.as_ref())
                                        .map(|s| s.label.clone())
                                        .unwrap_or_default(),
                                ),
                        )
                        .into_any_element(),
                ),
            );
        }
        if oauth || entry.subscription.is_some() {
            body = body.child(
                self.subscriptions
                    .update(cx, |view, cx| view.usage_card(window, cx)),
            );
        }
        if let Some(quota) = &entry.quota {
            let mut rows = vec![(
                "providerDetail.remaining",
                format!(
                    "{} {}",
                    quota.remaining.as_deref().unwrap_or("—"),
                    quota.unit.as_deref().unwrap_or_default()
                )
                .trim_end()
                .to_string(),
            )];
            if let Some(used) = &quota.used {
                rows.push(("providerDetail.used", used.clone()));
            }
            if let Some(limit) = &quota.limit {
                rows.push(("providerForm.limit", limit.clone()));
            }
            body = body.child(self.card("providerDetail.quota", self.metrics(rows)));
        }
        let totals = self.summary.as_ref().map(|s| {
            s.providers
                .iter()
                .filter(|r| {
                    r.provider_entry_id == entry.id
                        && secret.as_ref().is_none_or(|id| r.secret_id == *id)
                })
                .fold([0u64; 4], |mut sum, r| {
                    for (i, n) in [
                        r.request_count,
                        r.input_tokens,
                        r.output_tokens,
                        r.cache_read_tokens.saturating_add(r.cache_creation_tokens),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        sum[i] = sum[i].saturating_add(n);
                    }
                    sum
                })
        });
        let local = div()
            .flex()
            .flex_col()
            .when(self.usage_failed, |el| {
                el.child(self.banner(self.locale.text("agents.usageFailed"), false))
                    .child(
                        button(
                            "agents-usage-retry",
                            self.locale.text("subscriptionUsage.retry"),
                            false,
                            self.theme,
                        )
                        .h(px(26.))
                        .px(px(9.))
                        .text_size(px(12.))
                        .line_height(px(12.))
                        .self_start()
                        .disabled(self.pending.is_some())
                        .on_click(cx.listener(|this, _, _, cx| this.load_usage(cx))),
                    )
            })
            .when(!self.usage_failed, |el| {
                el.child(
                    self.metrics(
                        [
                            "server.requests",
                            "agents.inputTokens",
                            "agents.outputTokens",
                            "agents.cacheTokens",
                        ]
                        .iter()
                        .enumerate()
                        .map(|(i, k)| {
                            (
                                *k,
                                totals
                                    .map(|t| compact(t[i], self.chinese))
                                    .unwrap_or_else(|| "—".into()),
                            )
                        })
                        .collect(),
                    ),
                )
            })
            .child(
                div()
                    .mt(px(8.))
                    .text_size(px(11.))
                    .line_height(px(16.5))
                    .text_color(self.theme.tertiary)
                    .child(self.locale.text("agents.localUsageHint")),
            );
        body.child(self.card("agents.localUsage", local.into_any_element()))
            .observe("agents-account-detail")
            .into_any_element()
    }
}
fn compact(value: u64, chinese: bool) -> String {
    let units: &[(u64, &str)] = if chinese {
        &[(100_000_000, "亿"), (10_000, "万")]
    } else {
        &[(1_000_000_000, "B"), (1_000_000, "M"), (1_000, "K")]
    };
    for (scale, suffix) in units {
        if value >= *scale {
            let number = format!("{:.1}", value as f64 / *scale as f64);
            return format!("{}{suffix}", number.trim_end_matches(".0"));
        }
    }
    value.to_string()
}
impl Render for Agents {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (_, width) = crate::theme::columns(f32::from(window.viewport_size().width));
        div()
            .flex()
            .size_full()
            .min_w_0()
            .gap(px(8.))
            .text_color(self.theme.text)
            .child(self.list(width, cx))
            .child(self.detail(window, cx))
    }
}
