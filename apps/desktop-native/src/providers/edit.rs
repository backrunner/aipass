use super::{controls::button, presentation, Form, FormEvent};
use aipass_provider_registry::{default_provider_definitions, AuthScheme, InterfaceType};
use gpui::{div, prelude::*, px, svg, AnyElement, Context, FontWeight, Window};
use gpui_base::{input::Input, Button};

impl Form {
    pub(super) fn edit(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let entry = self
            .draft
            .original
            .as_ref()
            .expect("inline editing requires an entry");
        let t = self.theme;
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
                                            .border_b_1()
                                            .border_color(t.border)
                                            .text_size(px(18.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(Input::new(&self.inputs[0])),
                                    )
                                    .child(
                                        button(
                                            "provider-edit-cancel",
                                            self.locale.text("common.cancel"),
                                            false,
                                            t,
                                        )
                                        .px(px(8.))
                                        .border_color(gpui::rgba(0))
                                        .disabled(self.busy)
                                        .on_click(
                                            cx.listener(|_, _, _, cx| cx.emit(FormEvent::Close)),
                                        ),
                                    )
                                    .child(
                                        button(
                                            "provider-edit-save",
                                            self.locale.text("providerModal.saveChanges"),
                                            true,
                                            t,
                                        )
                                        .disabled(self.busy)
                                        .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                                    ),
                            )
                            .child(presentation::badges(entry, t, &self.locale, self.dark)),
                    ),
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
                                Button::new("provider-edit-advanced")
                                    .w_full()
                                    .px(px(16.))
                                    .py(px(13.))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .text_size(px(13.))
                                    .accessibility_label(self.locale.text("providerForm.advanced"))
                                    .child(
                                        div()
                                            .flex_1()
                                            .child(self.locale.text("providerForm.advanced")),
                                    )
                                    .child(
                                        svg()
                                            .path(if self.advanced {
                                                "chevron-down"
                                            } else {
                                                "chevron-right"
                                            })
                                            .size(px(14.))
                                            .text_color(t.tertiary),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.advanced = !this.advanced;
                                        cx.notify();
                                    })),
                            )
                            .when(self.advanced, |el| el.child(self.edit_advanced(window, cx))),
                    )
                    .child(
                        card()
                            .child(heading("providerDetail.notes"))
                            .child(self.field(6, false, window, cx)),
                    ),
            )
            .into_any_element()
    }

    fn edit_advanced(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let definitions = default_provider_definitions();
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
            .child(
                self.picker(
                    "provider",
                    self.locale.text("providerForm.provider"),
                    provider,
                    definitions
                        .into_iter()
                        .enumerate()
                        .map(|(i, d)| (i, d.display_name.to_owned()))
                        .collect(),
                    cx,
                ),
            )
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
                    cx,
                ),
            )
            .child(self.field(7, false, window, cx))
            .child(self.field(8, false, window, cx))
            .child(self.field(9, false, window, cx))
            .child(
                Button::new("provider-edit-websocket")
                    .role(gpui::Role::Switch)
                    .disabled(self.busy)
                    .px(px(16.))
                    .py(px(12.))
                    .text_size(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .accessibility_label(self.locale.text("providerForm.supportsWebsockets"))
                    .child(
                        div()
                            .flex_1()
                            .child(self.locale.text("providerForm.supportsWebsockets")),
                    )
                    .child(
                        div()
                            .w(px(36.))
                            .h(px(20.))
                            .p(px(2.))
                            .rounded_full()
                            .bg(if self.draft.websockets {
                                self.theme.accent
                            } else {
                                self.theme.border
                            })
                            .flex()
                            .when(self.draft.websockets, |el| el.justify_end())
                            .child(div().size(px(16.)).rounded_full().bg(gpui::rgb(0xffffff))),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.draft.websockets = !this.draft.websockets;
                        this.draft.websocket_touched = true;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
}
