use super::{controls::icon, form::LABELS, Form};
use crate::parity::Observe;
use aipass_provider_registry::{default_provider_definitions, AuthScheme, InterfaceType};
use gpui::{div, prelude::*, px, AnyElement, Context, Focusable, FontWeight, Window};
use gpui_base::{
    input::{Input, Textarea},
    Button,
};

impl Form {
    pub(super) fn field(
        &self,
        index: usize,
        removable: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let t = self.theme;
        let inline_edit = self.draft.original.is_some() && index != 6;
        let focused = if index == 6 {
            self.notes.read(cx).focus_handle(cx).is_focused(window)
        } else {
            self.inputs[index]
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        };
        if index == 6 && self.is_editing() {
            return self.edit_notes(focused, cx);
        }
        let advanced_last = [8, 10, 11, 12, 13]
            .into_iter()
            .rev()
            .find(|i| self.visible.contains(i));
        let last = self.draft.original.is_none()
            && (index == 2
                || advanced_last == Some(index)
                || (index == 9
                    && advanced_last.is_none()
                    && !matches!(
                        self.draft.interface,
                        InterfaceType::OpenAiCompatible | InterfaceType::AzureOpenAi
                    ))
                || ((3..=6).contains(&index)
                    && self.visible.iter().rfind(|i| (3..=6).contains(*i)) == Some(&index)));
        div()
            .px(px(16.))
            .py(px(if inline_edit { 12. } else { 13. }))
            .flex()
            .gap(px(if removable { 8. } else { 10. }))
            .items_center()
            .when(!last, |el| el.border_b_1())
            .border_color(t.divider)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(if inline_edit { 5. } else { 4. }))
                    .when(index != 6 || self.draft.original.is_none(), |el| {
                        el.child(
                            div()
                                .text_size(px(11.))
                                .line_height(px(13.))
                                .text_color(t.tertiary)
                                .child(self.locale.text(match (inline_edit, index) {
                                    (true, 3) => "providerDetail.endpoint",
                                    (true, 8) => "providerDetail.console",
                                    (true, 10) => "providerDetail.aliases",
                                    (true, 11) => "providerDetail.headers",
                                    _ => LABELS[index],
                                }))
                                .observe(format!("provider-field-{index}-label")),
                        )
                    })
                    .when(inline_edit && index == 11, |el| {
                        el.when_some(
                            self.draft
                                .original
                                .as_ref()
                                .filter(|entry| !entry.header_names.is_empty()),
                            |el, entry| {
                                el.child(
                                    div()
                                        .font_family(if cfg!(target_os = "macos") {
                                            "Menlo"
                                        } else {
                                            "monospace"
                                        })
                                        .text_size(px(12.))
                                        .line_height(px(14.))
                                        .child(entry.header_names.join(", "))
                                        .observe("provider-edit-header-names"),
                                )
                            },
                        )
                    })
                    .child(
                        div()
                            .relative()
                            .when(index == 2, |el| el.pr(px(34.)))
                            .h(px(if index == 6 {
                                if self.draft.original.is_none() {
                                    88.
                                } else {
                                    64.
                                }
                            } else if inline_edit {
                                29.
                            } else {
                                28.
                            }))
                            .rounded(px(if inline_edit { 0. } else { 4. }))
                            .border_b_1()
                            .border_color(if focused {
                                t.accent
                            } else if inline_edit {
                                t.border
                            } else {
                                t.border.opacity(0.58)
                            })
                            .bg(if focused && !inline_edit {
                                t.surface2
                            } else {
                                gpui::rgba(0)
                            })
                            .px(px(if inline_edit { 0. } else { 1. }))
                            .text_size(px(if inline_edit { 12. } else { 13. }))
                            .line_height(px(if inline_edit { 14. } else { 20. }))
                            .child(if index == 6 {
                                Textarea::new(&self.notes).into_any_element()
                            } else if index == 9 {
                                self.concurrency_input(cx)
                            } else {
                                Input::new(&self.inputs[index]).into_any_element()
                            })
                            .when(index == 2, |el| {
                                el.child(
                                    icon(
                                        "provider-key-reveal",
                                        if self.revealed { "eye-off" } else { "eye" },
                                        self.locale.text(if self.revealed {
                                            "providerForm.hideApiKey"
                                        } else {
                                            "providerForm.showApiKey"
                                        }),
                                        t,
                                    )
                                    .absolute()
                                    .right(px(5.))
                                    .top(px(2.))
                                    .size(px(24.))
                                    .disabled(self.busy)
                                    .on_click(cx.listener(
                                        |this, _, window, cx| {
                                            this.revealed = !this.revealed;
                                            this.inputs[2].update(cx, |input, cx| {
                                                input.set_masked(!this.revealed, window, cx)
                                            });
                                            cx.notify();
                                        },
                                    )),
                                )
                            })
                            .observe(format!("provider-field-{index}-input")),
                    )
                    .when(
                        matches!(index, 9 | 12) && self.draft.original.is_none(),
                        |el| {
                            el.child(
                                div()
                                    .text_size(px(11.))
                                    .line_height(px(15.4))
                                    .text_color(t.tertiary)
                                    .child(self.locale.text(if index == 12 {
                                        "providerForm.groupHint"
                                    } else {
                                        "providerForm.maxConcurrentRequestsHint"
                                    })),
                            )
                        },
                    )
                    .observe(format!("provider-field-{index}-content")),
            )
            .when(removable, |el| {
                el.child(
                    icon(
                        ("remove-provider-field", index),
                        "x",
                        self.locale
                            .text("providerForm.removeField")
                            .replace("{label}", &self.locale.text(LABELS[index])),
                        t,
                    )
                    .disabled(self.busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.remove_optional(index, window, cx);
                    })),
                )
            })
            .observe(format!("provider-field-{index}"))
            .into_any_element()
    }

    pub(super) fn fields(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let card = || {
            div()
                .flex_shrink_0()
                .rounded(px(10.))
                .border_1()
                .border_color(t.divider)
                .bg(crate::theme::oklab_mix(t.surface, t.surface2, 0.92))
                .overflow_hidden()
                .flex()
                .flex_col()
        };
        let title = |key: &str| {
            div()
                .text_size(px(11.))
                .line_height(px(13.))
                .pl(px(2.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.tertiary)
                .child(self.locale.text(key).to_uppercase())
        };
        let definitions = default_provider_definitions();
        let provider = definitions
            .iter()
            .find(|definition| definition.id == self.draft.provider_id)
            .map(|definition| definition.display_name.to_owned())
            .unwrap_or_else(|| self.draft.provider_id.clone());
        let interfaces = [
            "interface.openaiCompatible",
            "interface.anthropicMessages",
            "interface.gemini",
            "interface.azureOpenai",
            "interface.bedrock",
            "interface.customHttp",
        ];
        let auth = [
            "authScheme.bearer",
            "authScheme.xApiKey",
            "authScheme.googleApiKey",
            "authScheme.azureApiKey",
            "authScheme.awsProfile",
            "authScheme.customHeader",
        ];
        let interface_index = match self.draft.interface {
            InterfaceType::OpenAiCompatible => 0,
            InterfaceType::AnthropicMessages => 1,
            InterfaceType::Gemini => 2,
            InterfaceType::AzureOpenAi => 3,
            InterfaceType::Bedrock => 4,
            InterfaceType::CustomHttp => 5,
        };
        let auth_index = match self.draft.auth {
            AuthScheme::Bearer => 0,
            AuthScheme::XApiKey => 1,
            AuthScheme::GoogleApiKey => 2,
            AuthScheme::AzureApiKey => 3,
            AuthScheme::AwsProfile => 4,
            AuthScheme::CustomHeader => 5,
        };
        let editing = self.draft.original.is_some();
        div().id("provider-form-fields").track_scroll(&self.scroll).flex_1().min_h_0().overflow_y_scroll().p(px(20.)).bg(t.bg).flex().flex_col().gap(px(22.))
            .child(div().flex().flex_col().gap(px(10.)).child(title("providerForm.identity"))
                .child(card().child(self.picker("provider", self.locale.text("providerForm.provider"), provider,
                    definitions.into_iter().enumerate().map(|(i, d)| (i, d.display_name.to_owned())).collect(), window, cx))
                    .child(self.field(0, false, window, cx))
                    .when(!editing, |el| el.child(self.field(1, false, window, cx)).child(self.field(2, false, window, cx)))))
            .when(self.visible.iter().any(|index| (3..=6).contains(index)), |el| el.child(div().flex().flex_col().gap(px(10.))
                .child(title("providerForm.details"))
                .child(card().children(self.visible.iter().filter(|index| (3..=6).contains(*index)).map(|index| self.field(*index, true, window, cx))))))
            .child(card().child(Button::new("provider-advanced").aria_expanded(self.advanced).w_full().px(px(16.)).py(px(12.)).border_b_1().border_color(if self.advanced { t.divider } else { gpui::rgba(0) }).flex().items_center().gap(px(8.))
                .accessibility_label(self.locale.text("providerForm.advanced")).text_size(px(13.)).line_height(px(18.2)).font_weight(FontWeight::SEMIBOLD)
                .cursor_pointer().child(div().flex_1().child(self.locale.text("providerForm.advanced")))
                .hover(move |s| s.bg(t.surface2)).disabled(self.busy)
                .child(self.disclosure_frame.chevron(t.tertiary, 15.))
                .on_click(cx.listener(|this, _, _, cx| { this.advanced = !this.advanced; cx.notify(); }))
                .observe("provider-advanced-trigger"))
                .child(self.disclosure_frame.body("provider-advanced-content", div().flex().flex_col()
                    .when(self.visible.contains(&7), |el| el.child(self.field(7, true, window, cx)))
                    .child(self.picker("interface", self.locale.text("providerForm.interface"), self.locale.text(interfaces[interface_index]),
                        interfaces.iter().enumerate().map(|(i, key)| (i, self.locale.text(key))).collect(), window, cx))
                    .child(self.picker("auth", self.locale.text("providerForm.auth"), self.locale.text(auth[auth_index]),
                        auth.iter().enumerate().filter(|(i, _)| {
                            let scheme = [AuthScheme::Bearer, AuthScheme::XApiKey, AuthScheme::GoogleApiKey, AuthScheme::AzureApiKey, AuthScheme::AwsProfile, AuthScheme::CustomHeader][*i].clone();
                            scheme == self.draft.auth || aipass_desktop_native::catalog::draft::auth_compatible(&scheme, &self.draft.interface)
                        }).map(|(i, key)| (i, self.locale.text(key))).collect(), window, cx))
                    .child(self.field(9, false, window, cx))
                    .when(matches!(self.draft.interface, InterfaceType::OpenAiCompatible | InterfaceType::AzureOpenAi), |el| el
                        .child(Button::new("provider-websocket").role(gpui::Role::Switch).disabled(self.busy)
                            .accessibility_label(self.locale.text("providerForm.supportsWebsockets"))
                            .px(px(16.)).py(px(13.)).flex().items_center().gap(px(12.)).text_size(px(12.))
                            .child(div().flex_1().child(self.locale.text("providerForm.supportsWebsockets")))
                            .child(div().w(px(36.)).h(px(20.)).p(px(2.)).rounded_full().bg(if self.draft.websockets { t.accent } else { t.border })
                                .flex().when(self.draft.websockets, |el| el.justify_end()).child(div().size(px(16.)).rounded_full().bg(gpui::rgb(0xffffff))))
                            .on_click(cx.listener(|this, _, _, cx| { this.draft.websockets = !this.draft.websockets; this.draft.websocket_touched = true; cx.notify(); }))))
                    .children([8, 10, 11, 12].into_iter().filter(|i| self.visible.contains(i)).map(|i| self.field(i, true, window, cx)))
                    .when(self.visible.contains(&13), |el| el.child(self.billing_fields(window, cx))))))
            .when(self.has_optional_fields(), |el| el.child(div().flex().items_start().child(self.add_field_menu(window, cx))))
    }
}
