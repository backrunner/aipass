use super::{
    controls::{button, ghost_button},
    presentation, Form, FormEvent,
};
use crate::parity::Observe;
use aipass_provider_registry::{default_provider_definitions, AuthScheme, InterfaceType};
use gpui::{div, prelude::*, px, AnyElement, Context, Focusable, FontWeight, Window};
use gpui_base::{input::Input, Button};

impl Form {
    pub(super) fn edit(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let entry = self
            .draft
            .original
            .as_ref()
            .expect("inline editing requires an entry");
        let t = self.theme;
        let title_focused = self.inputs[0].read(cx).focus_handle(cx).is_focused(window);
        let card = || {
            div()
                .flex_shrink_0()
                .rounded(px(12.))
                .border_1()
                .border_color(t.border)
                .bg(t.surface)
                .overflow_hidden()
                .flex()
                .flex_col()
        };
        let heading = |key: &str| {
            div()
                .px(px(16.))
                .py(px(12.))
                .text_size(px(13.))
                .line_height(px(16.))
                .font_weight(FontWeight::SEMIBOLD)
                .border_b_1()
                .border_color(t.divider)
                .child(self.locale.text(key))
        };
        div()
            .size_full()
            .rounded(px(14.))
            .overflow_hidden()
            .flex()
            .flex_col()
            .bg(t.surface)
            .text_color(t.text)
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(20.))
                    .py(px(18.))
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
                                    .flex()
                                    .items_center()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .h(px(28.))
                                            // Inline input baselines center the content box;
                                            // compensate the one-sided underline's half edge.
                                            .relative()
                                            .top(px(0.5))
                                            .border_b_1()
                                            .border_color(if title_focused {
                                                t.accent
                                            } else {
                                                t.border
                                            })
                                            .text_size(px(18.))
                                            .line_height(px(28.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(Input::new(&self.inputs[0]))
                                            .observe("provider-edit-title"),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(
                                                ghost_button("provider-edit-cancel", false, t)
                                                    .accessibility_label(
                                                        self.locale.text("common.cancel"),
                                                    )
                                                    .child(self.locale.text("common.cancel"))
                                                    .disabled(self.busy)
                                                    .on_click(cx.listener(|_, _, _, cx| {
                                                        cx.emit(FormEvent::Close)
                                                    }))
                                                    .observe("provider-edit-cancel"),
                                            )
                                            .child(
                                                button(
                                                    "provider-edit-save",
                                                    self.locale.text("providerModal.saveChanges"),
                                                    true,
                                                    t,
                                                )
                                                .disabled(self.busy)
                                                .on_click(
                                                    cx.listener(|this, _, _, cx| this.save(cx)),
                                                )
                                                .observe("provider-edit-save"),
                                            ),
                                    ),
                            )
                            .child(presentation::badges(entry, t, &self.locale, self.dark)),
                    )
                    .observe("provider-edit-header"),
            )
            .when(!self.error.is_empty(), |el| {
                el.child(
                    div()
                        .mx(px(24.))
                        .mt(px(12.))
                        .p(px(12.))
                        .rounded(px(8.))
                        .text_size(px(12.))
                        .bg(t.danger_soft)
                        .text_color(t.danger)
                        .child(self.error.clone()),
                )
            })
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("provider-edit-body")
                            .track_scroll(&self.scroll)
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .px(px(24.))
                            .pt(px(24.))
                            .pb(px(36.))
                            .flex()
                            .flex_col()
                            .gap(px(18.))
                            .child(
                                card()
                                    .child(heading("providerDetail.credentials"))
                                    .child(self.field(3, false, window, cx))
                                    .children(self.credentials.clone())
                                    .child(self.field(4, false, window, cx))
                                    .child(self.field(5, false, window, cx))
                                    .child(
                                        div().border_t_1().border_color(t.divider).child(
                                            Button::new("provider-edit-advanced")
                                                .aria_expanded(self.advanced)
                                                .w_full()
                                                .px(px(16.))
                                                .py(px(13.))
                                                .flex()
                                                .items_center()
                                                .gap(px(8.))
                                                .text_size(px(12.))
                                                .line_height(px(16.8))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(t.secondary)
                                                .accessibility_label(
                                                    self.locale.text("providerForm.advanced"),
                                                )
                                                .child(div().flex_1().child(
                                                    self.locale.text("providerForm.advanced"),
                                                ))
                                                .child(
                                                    self.disclosure_frame.chevron(t.tertiary, 14.),
                                                )
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.advanced = !this.advanced;
                                                    cx.notify();
                                                }))
                                                .observe("provider-edit-advanced-toggle"),
                                        ),
                                    )
                                    .child(self.disclosure_frame.body(
                                        "provider-edit-advanced-content",
                                        self.edit_advanced(window, cx),
                                    )),
                            )
                            .when_some(self.detail_panels.as_ref(), |el, (subscriptions, _)| {
                                el.when(entry.subscription.is_some(), |el| {
                                    el.child(
                                        subscriptions
                                            .update(cx, |view, cx| view.usage_card(window, cx)),
                                    )
                                })
                                .child(
                                    subscriptions
                                        .update(cx, |view, cx| view.options_card(window, cx)),
                                )
                            })
                            .child(
                                card()
                                    .child(heading("providerDetail.notes"))
                                    .child(self.field(6, false, window, cx)),
                            )
                            .when_some(self.detail_panels.as_ref(), |el, (_, integrations)| {
                                el.when(integrations.read(cx).is_visible(), |el| {
                                    el.child(integrations.clone())
                                })
                            })
                            .observe("provider-edit-body"),
                    )
                    // Content is inset by 24px. Keep the opaque mask inside
                    // that column so it cannot square off the pane's corners.
                    .child(
                        div()
                            .absolute()
                            .left(px(24.))
                            .right(px(24.))
                            .top_0()
                            .bottom_0()
                            .child(crate::scroll_fade::fade(t.surface, self.scroll.clone())),
                    ),
            )
            .into_any_element()
    }

    fn edit_advanced(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let definitions = default_provider_definitions();
        let mut providers: Vec<_> = definitions
            .iter()
            .enumerate()
            .map(|(i, d)| (i, d.display_name.to_owned()))
            .collect();
        if !definitions.iter().any(|d| d.id == self.draft.provider_id) {
            providers.push((definitions.len(), self.draft.provider_id.clone()));
        }
        let provider = definitions
            .iter()
            .find(|d| d.id == self.draft.provider_id)
            .map(|d| d.display_name.to_owned())
            .unwrap_or_else(|| self.draft.provider_id.clone());
        let interfaces = [
            InterfaceType::OpenAiCompatible,
            InterfaceType::AnthropicMessages,
            InterfaceType::Gemini,
            InterfaceType::AzureOpenAi,
            InterfaceType::Bedrock,
            InterfaceType::CustomHttp,
        ];
        let auths = [
            AuthScheme::Bearer,
            AuthScheme::XApiKey,
            AuthScheme::GoogleApiKey,
            AuthScheme::AzureApiKey,
            AuthScheme::AwsProfile,
            AuthScheme::CustomHeader,
        ];
        let keys = [
            "authScheme.bearer",
            "authScheme.xApiKey",
            "authScheme.googleApiKey",
            "authScheme.azureApiKey",
            "authScheme.awsProfile",
            "authScheme.customHeader",
        ];
        let auth_index = auths
            .iter()
            .position(|auth| *auth == self.draft.auth)
            .unwrap_or(0);
        div()
            .flex()
            .flex_col()
            .child(self.picker(
                "provider",
                self.locale.text("providerForm.provider"),
                provider,
                providers,
                window,
                cx,
            ))
            .child(
                self.picker(
                    "interface",
                    self.locale.text("providerForm.interface"),
                    self.locale
                        .text(presentation::protocol_key(&self.draft.interface)),
                    interfaces
                        .iter()
                        .enumerate()
                        .map(|(i, interface)| {
                            (i, self.locale.text(presentation::protocol_key(interface)))
                        })
                        .collect(),
                    window,
                    cx,
                ),
            )
            .child(
                self.picker(
                    "auth",
                    self.locale.text("providerForm.auth"),
                    self.locale.text(keys[auth_index]),
                    auths
                        .iter()
                        .enumerate()
                        .filter(|(_, auth)| {
                            **auth == self.draft.auth
                                || aipass_desktop_native::catalog::draft::auth_compatible(
                                    auth,
                                    &self.draft.interface,
                                )
                        })
                        .map(|(i, _)| (i, self.locale.text(keys[i])))
                        .collect(),
                    window,
                    cx,
                ),
            )
            .child(self.field(7, false, window, cx))
            .child(self.field(8, false, window, cx))
            .child(self.field(10, false, window, cx))
            .child(self.field(9, false, window, cx))
            .child(self.edit_websocket(window, cx))
            .child(self.field(11, false, window, cx))
            .when_some(
                self.draft
                    .original
                    .as_ref()
                    .and_then(|entry| entry.gateway.as_ref())
                    .and_then(|gateway| gateway.group.as_ref())
                    .filter(|group| !group.is_empty()),
                |el, group| {
                    el.child(
                        div()
                            .px(px(16.))
                            .py(px(12.))
                            .border_b_1()
                            .border_color(self.theme.divider)
                            // GPUI's overflow mask is rectangular. Match the
                            // containing card's curved bottom at this final row.
                            .rounded_bl(px(11.))
                            .rounded_br(px(11.))
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .line_height(px(13.))
                                    .text_color(self.theme.tertiary)
                                    .child(self.locale.text("providerDetail.gatewayGroup")),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(crate::typography::normal_line_height(
                                        window, group, 12.,
                                    ))
                                    .child(group.clone()),
                            )
                            .observe("provider-edit-gateway-group"),
                    )
                },
            )
            .into_any_element()
    }
}
