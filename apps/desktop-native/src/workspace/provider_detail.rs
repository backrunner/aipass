//! Selected provider header, credentials and detail sections.
use super::Workspace;
use crate::{parity::Observe, providers::presentation};
use aipass_desktop_native::catalog::Action;
use aipass_provider_registry::{CredentialKind, EndpointKind};
use gpui::{div, prelude::*, px, svg, AnyElement, Context, FontWeight, Window};
use gpui_base::Button;

impl Workspace {
    pub(super) fn provider_detail(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.form_is_editing(cx) {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .children(
                    self.tools
                        .read(cx)
                        .notice()
                        .map(|notice| div().px(px(24.)).pt(px(12.)).flex_shrink_0().child(notice)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .child(self.provider_form.as_ref().unwrap().clone()),
                )
                .into_any_element();
        }
        crate::parity::record_scroll("provider-detail", &self.detail_scroll, cx);
        let visible = self.visible_providers(cx);
        let entry = self
            .selected_provider
            .and_then(|id| visible.iter().find(|entry| entry.id == id).copied())
            .or_else(|| visible.first().copied());
        let Some(entry) = entry else {
            return self
                .empty(
                    "providerDetail.noneSelected",
                    "providerDetail.noneSelectedDesc",
                    true,
                    cx,
                )
                .into_any_element();
        };
        let t = self.theme;
        let advanced = self
            .provider_disclosure
            .sample(self.provider_advanced, window, cx);
        let id = entry.id;
        let editing = entry.clone();
        let favorite = entry.favorite;
        let card = || {
            div()
                .w_full()
                .flex_shrink_0()
                .bg(t.surface.opacity(0.86))
                .rounded(px(10.))
                .border_1()
                .border_color(t.border)
                .overflow_hidden()
                .flex()
                .flex_col()
        };
        let row = |name: &str, label: String, value: String| {
            div()
                .flex_shrink_0()
                .px(px(16.))
                .py(px(12.))
                .flex()
                .flex_col()
                .gap(px(5.))
                .border_b_1()
                .border_color(t.divider)
                .child(
                    div()
                        .text_size(px(11.))
                        .line_height(px(13.))
                        .text_color(t.tertiary)
                        .child(label)
                        .observe(format!("provider-{name}-label")),
                )
                .child(
                    div()
                        .h(px(29.))
                        .flex()
                        .items_center()
                        .text_size(px(12.))
                        .line_height(px(15.))
                        .child(value)
                        .observe(format!("provider-{name}-value")),
                )
                .observe(format!("provider-{name}-row"))
        };
        div()
            .on_children_prepainted(crate::parity::record(&["provider-header", "provider-body"]))
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(20.))
                    .py(px(18.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .border_b_1()
                    .border_color(t.divider)
                    .child(presentation::identity(entry, true, self.dark))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .h(px(28.))
                                            .line_height(px(28.))
                                            .text_size(px(18.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_ellipsis()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .child(if entry.credential_kind == CredentialKind::OAuth { aipass_desktop_native::subscriptions::presentation::account(entry, &|key|self.locale.text(key)).title } else {entry.title.clone()}),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .when(self.selected != 10, |el| {
                                                el.child(
                                                    Button::new("favorite-provider")
                                                        .size(px(32.)).rounded(px(8.)).flex_shrink_0()
                                                        .flex().items_center().justify_center().cursor_pointer()
                                                        .accessibility_label(self.locale.text(if favorite {
                                                            "providerDetail.removeFavorite"
                                                        } else {
                                                            "providerDetail.addFavorite"
                                                        }))
                                                        .child(svg().path(if favorite { "star-filled" } else { "star" })
                                                            .size(px(16.)).text_color(if favorite { t.accent } else { t.secondary }))
                                                        .hover(move |s| s.bg(t.accent_soft))
                                                    .when(favorite, |el| el.bg(t.accent_soft))
                                                    .disabled(self.provider_busy)
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.request_provider(
                                                            Action::Favorite(id, !favorite),
                                                            cx,
                                                        )
                                                    })),
                                                )
                                            })
                                            .when(matches!(self.selected, 9 | 10), |el| {
                                                el.child(
                                                    self.button(
                                                        "restore-provider",
                                                        "providerDetail.restore",
                                                        "undo-2",
                                                        true,
                                                        false,
                                                    )
                                                    .disabled(self.provider_busy)
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.request_provider(
                                                            Action::Restore(id),
                                                            cx,
                                                        )
                                                    })),
                                                )
                                            })
                                            .when(
                                                !matches!(self.selected, 9 | 10)
                                                    && entry.credential_kind == CredentialKind::Api,
                                                |el| {
                                                    el.child(
                                                        self.button(
                                                            "edit-provider",
                                                            "providerDetail.edit",
                                                            "pencil",
                                                            true,
                                                            false,
                                                        )
                                                        .disabled(self.provider_busy)
                                                        .on_click(cx.listener(
                                                            move |this, _, window, cx| {
                                                                this.open_provider_form(
                                                                    Some(editing.clone()),
                                                                    window,
                                                                    cx,
                                                                )
                                                            },
                                                        )),
                                                    )
                                                },
                                            )
                                            .when(self.selected != 10, |el| {
                                                el.child(
                                    self.provider_actions(id, window, cx),
                                )
                                            }),
                                    ),
                            )
                            .child(presentation::badges(entry, t, &self.locale, self.dark)),
                    ),
            )
            .child(
                div()
                    .id("provider-detail-body")
                    .track_scroll(&self.detail_scroll)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(24.))
                    .pt(px(24.))
                    .pb(px(36.))
                    .flex()
                    .flex_col()
                    .gap(px(18.))
                    .children(self.tools.read(cx).notice())
                    .when(entry.credential_kind == CredentialKind::OAuth, |el| el.child(self.subscriptions.update(cx,|view,cx|view.account_card(window,cx))))
                    .when(entry.credential_kind == CredentialKind::Api, |el| el.child(
                        card()
                            .on_children_prepainted(crate::parity::record(&["detail-cred-heading","detail-cred-endpoint","detail-cred-keys","detail-cred-model","detail-cred-tags","detail-cred-advanced","detail-cred-status","detail-cred-body"]))
                            .child(
                                div()
                                    .px(px(16.))
                                    .py(px(12.))
                                    .text_size(px(13.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .border_b_1()
                                    .border_color(t.divider)
                                    .child(self.locale.text("providerDetail.credentials")).observe("provider-credentials-heading"),
                            )
                            .children(
                                entry
                                    .endpoints
                                    .iter()
                                    .filter(|endpoint| endpoint.kind == EndpointKind::Api)
                                    .map(|endpoint| {
                                        row(
                                            "endpoint", self.locale.text("providerDetail.endpoint"),
                                            endpoint.url.clone().unwrap_or_default(),
                                        )
                                    }),
                            )
                            .child(self.credentials.clone())
                            .child(row(
                                "model", self.locale.text("providerDetail.defaultModel"),
                                entry.default_model.clone().unwrap_or_else(|| "—".into()),
                            ))
                            .child(row(
                                "tags", self.locale.text("providerDetail.tags"),
                                if entry.tags.is_empty() {
                                    "—".into()
                                } else {
                                    entry.tags.join(", ")
                                },
                            ))
                            .child(
                                Button::new("provider-detail-advanced")
                                    .line_height(px(16.8)).border_t_1().border_color(t.divider)
                                    .w_full()
                                    .px(px(16.))
                                    .py(px(13.))
                                    .text_size(px(13.))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .accessibility_label(self.locale.text("providerForm.advanced"))
                                    .child(
                                        div()
                                            .flex_1()
                                            .child(self.locale.text("providerForm.advanced")),
                                    )
                                    .aria_expanded(self.provider_advanced)
                                    .child(advanced.chevron(t.tertiary, 14.))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.provider_advanced = !this.provider_advanced;
                                        cx.notify();
                                    })),
                            )
                            .child(self.tools.read(cx).status())
                            .child(advanced.body("provider-detail-advanced-body",
                                div().flex().flex_col().child(row(
                                    "provider", self.locale.text("providerForm.provider"),
                                    entry.provider_id.clone().unwrap_or_default(),
                                ))
                                .child(row(
                                    "domains", self.locale.text("providerForm.domains"),
                                    entry.domains.join(", "),
                                ))
                                .child(row(
                                    "aliases", self.locale.text("providerDetail.aliases"),
                                    entry
                                        .model_aliases
                                        .iter()
                                        .map(|(a, b)| format!("{a}={b}"))
                                        .collect::<Vec<_>>()
                                        .join(", "),
                                ))
                                .child(row(
                                    "headers", self.locale.text("providerDetail.headers"),
                                    entry.header_names.join(", "),
                                ))
                            )),
                    )
                    )
                    .when(entry.subscription.is_some() || entry.credential_kind == CredentialKind::OAuth, |el| el.child(self.subscriptions.update(cx,|view,cx|view.usage_card(window,cx))))
                    .when(entry.quota.is_some(), |el| {
                        el.child(
                            card().child(row(
                                "quota", entry
                                    .quota
                                    .as_ref()
                                    .and_then(|quota| quota.label.clone())
                                    .unwrap_or_default(),
                                entry
                                    .quota
                                    .as_ref()
                                    .map(|quota| {
                                        format!(
                                            "{} {}",
                                            quota.remaining.as_deref().unwrap_or_default(),
                                            quota.unit.as_deref().unwrap_or_default()
                                        )
                                    })
                                    .unwrap_or_default(),
                            )),
                        )
                    })
                    .when(!matches!(self.selected, 9 | 10), |el| el.child(self.subscriptions.update(cx, |view, cx| view.options_card(window, cx))))
                    .when(entry.credential_kind == CredentialKind::Api || entry.notes.as_ref().is_some_and(|v|!v.is_empty()), |el|el.child(self.notes.clone()))
                    .when(self.integrations.read(cx).is_visible(), |el| el.child(self.integrations.clone())),
            )
            .into_any_element()
    }
}
