use super::{controls::icon, form::LABELS, Form};
use aipass_provider_registry::{default_provider_definitions, AuthScheme, InterfaceType};
use gpui::{div, prelude::*, px, svg, Anchor, AnyElement, Context, Focusable, FontWeight, Window};
use gpui_base::{
    input::{Input, Textarea},
    Button, Popover,
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
        let focused = if index == 6 {
            self.notes.read(cx).focus_handle(cx).is_focused(window)
        } else {
            self.inputs[index]
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        };
        div()
            .px(px(16.))
            .py(px(13.))
            .flex()
            .gap(px(10.))
            .items_center()
            .border_b_1()
            .border_color(t.divider)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .when(index != 6 || self.draft.original.is_none(), |el| {
                        el.child(
                            div()
                                .text_size(px(11.))
                                .line_height(px(13.))
                                .text_color(t.tertiary)
                                .child(self.locale.text(LABELS[index])),
                        )
                    })
                    .child(
                        div()
                            .relative()
                            .when(index == 2, |el| el.pr(px(34.)))
                            .h(px(if index == 6 { 64. } else { 28. }))
                            .rounded(px(4.))
                            .border_b_1()
                            .border_color(if focused { t.accent } else { t.divider })
                            .bg(if focused { t.surface2 } else { t.surface })
                            .text_size(px(13.))
                            .line_height(px(20.))
                            .child(if index == 6 {
                                Textarea::new(&self.notes).into_any_element()
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
                            }),
                    )
                    .when(index == 9 && self.draft.original.is_none(), |el| {
                        el.child(
                            div()
                                .text_size(px(11.))
                                .line_height(px(16.))
                                .text_color(t.tertiary)
                                .child(self.locale.text("providerForm.maxConcurrentRequestsHint")),
                        )
                    }),
            )
            .when(removable, |el| {
                el.child(
                    icon(
                        ("remove-provider-field", index),
                        "x",
                        self.locale.text("common.remove"),
                        t,
                    )
                    .disabled(self.busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.inputs[index].update(cx, |input, cx| input.set_value("", window, cx));
                        if index == 6 {
                            this.notes
                                .update(cx, |input, cx| input.set_value("", window, cx));
                        }
                        this.visible.remove(&index);
                        cx.notify();
                    })),
                )
            })
            .into_any_element()
    }

    pub(super) fn picker(
        &self,
        kind: &'static str,
        label: String,
        value: String,
        options: Vec<(usize, String)>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let t = self.theme;
        let owner = cx.entity().downgrade();
        Popover::new(kind)
            .anchor(Anchor::TopLeft)
            .offset(px(4.))
            .open(self.picker == Some(kind))
            .on_open_change(cx.listener(move |this, open: &bool, _, cx| {
                this.picker = open.then_some(kind);
                cx.notify();
            }))
            .trigger(if kind == "add-field" {
                Button::new("provider-add-field")
                    .disabled(self.busy)
                    .h(px(32.))
                    .px(px(12.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(t.border)
                    .bg(t.surface)
                    .text_color(t.accent)
                    .text_size(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .accessibility_label(label.clone())
                    .child(svg().path("plus").size(px(14.)).text_color(t.accent))
                    .child(label)
                    .child(
                        svg()
                            .path("chevron-down")
                            .size(px(14.))
                            .text_color(t.accent),
                    )
            } else {
                Button::new((kind, 1000_usize))
                    .disabled(self.busy)
                    .w_full()
                    .px(px(16.))
                    .py(px(13.))
                    .border_b_1()
                    .border_color(t.divider)
                    .text_color(t.text)
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .accessibility_label(label.clone())
                    .cursor_pointer()
                    .hover(move |s| s.bg(t.surface2))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .line_height(px(13.))
                                    .text_color(t.tertiary)
                                    .child(label),
                            )
                            .child(
                                div()
                                    .h(px(28.))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .rounded(px(4.))
                                    .border_b_1()
                                    .border_color(t.divider)
                                    .text_size(px(13.))
                                    .line_height(px(20.))
                                    .child(div().flex_1().child(value))
                                    .child(
                                        svg().path("chevron-down").size(px(14.)).text_color(t.text),
                                    ),
                            ),
                    )
            })
            .content(move |_, _, _| {
                div()
                    .id((kind, 1001_usize))
                    .w(px(280.))
                    .max_h(px(260.))
                    .overflow_y_scroll()
                    .p(px(4.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(t.border)
                    .bg(t.surface)
                    .flex()
                    .flex_col()
                    .children(options.into_iter().map(|(index, label)| {
                        let owner = owner.clone();
                        Button::new((kind, index))
                            .role(gpui::Role::MenuItem)
                            .w_full()
                            .px(px(10.))
                            .py(px(7.))
                            .rounded(px(6.))
                            .text_size(px(13.))
                            .text_color(t.text)
                            .cursor_pointer()
                            .hover(move |s| s.bg(t.accent_soft))
                            .focus(move |s| s.bg(t.accent_soft))
                            .accessibility_label(label.clone())
                            .child(label)
                            .on_click(move |_, window, cx| {
                                let _ = owner
                                    .update(cx, |form, cx| form.choose(kind, index, window, cx));
                            })
                    }))
            })
            .into_any_element()
    }

    pub(super) fn fields(&self, window: &Window, cx: &Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let card = || {
            div()
                .flex_shrink_0()
                .rounded(px(10.))
                .border_1()
                .border_color(t.border)
                .bg(t.surface)
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
                    definitions.into_iter().enumerate().map(|(i, d)| (i, d.display_name.to_owned())).collect(), cx))
                    .child(self.field(0, false, window, cx))
                    .when(!editing, |el| el.child(self.field(1, false, window, cx)).child(self.field(2, false, window, cx)))))
            .when(self.visible.iter().any(|index| (3..=6).contains(index)), |el| el.child(div().flex().flex_col().gap(px(10.))
                .child(title("providerForm.details"))
                .child(card().children(self.visible.iter().filter(|index| (3..=6).contains(*index)).map(|index| self.field(*index, true, window, cx))))))
            .child(card().child(Button::new("provider-advanced").w_full().px(px(16.)).py(px(14.)).flex().items_center().gap(px(8.)).bg(t.surface2)
                .accessibility_label(self.locale.text("providerForm.advanced")).text_size(px(13.)).font_weight(FontWeight::MEDIUM)
                .cursor_pointer().child(div().flex_1().child(self.locale.text("providerForm.advanced")))
                .child(svg().path(if self.advanced { "chevron-down" } else { "chevron-right" }).size(px(14.)).text_color(t.tertiary))
                .on_click(cx.listener(|this, _, _, cx| { this.advanced = !this.advanced; cx.notify(); })))
                .when(self.advanced, |el| el
                    .children(self.visible.iter().filter(|index| (7..=8).contains(*index)).map(|index| self.field(*index, true, window, cx)))
                    .child(self.picker("interface", self.locale.text("providerForm.interface"), self.locale.text(interfaces[interface_index]),
                        interfaces.iter().enumerate().map(|(i, key)| (i, self.locale.text(key))).collect(), cx))
                    .child(self.picker("auth", self.locale.text("providerForm.auth"), self.locale.text(auth[auth_index]),
                        auth.iter().enumerate().filter(|(i, _)| {
                            let scheme = [AuthScheme::Bearer, AuthScheme::XApiKey, AuthScheme::GoogleApiKey, AuthScheme::AzureApiKey, AuthScheme::AwsProfile, AuthScheme::CustomHeader][*i].clone();
                            scheme == self.draft.auth || aipass_desktop_native::catalog::draft::auth_compatible(&scheme, &self.draft.interface)
                        }).map(|(i, key)| (i, self.locale.text(key))).collect(), cx))
                    .child(self.field(9, false, window, cx))
                    .when(matches!(self.draft.interface, InterfaceType::OpenAiCompatible | InterfaceType::AzureOpenAi), |el| el
                        .child(Button::new("provider-websocket").role(gpui::Role::Switch).disabled(self.busy)
                            .accessibility_label(self.locale.text("providerForm.supportsWebsockets"))
                            .px(px(16.)).py(px(13.)).flex().items_center().gap(px(12.)).text_size(px(12.))
                            .child(div().flex_1().child(self.locale.text("providerForm.supportsWebsockets")))
                            .child(div().w(px(36.)).h(px(20.)).p(px(2.)).rounded_full().bg(if self.draft.websockets { t.accent } else { t.border })
                                .flex().when(self.draft.websockets, |el| el.justify_end()).child(div().size(px(16.)).rounded_full().bg(gpui::rgb(0xffffff))))
                            .on_click(cx.listener(|this, _, _, cx| { this.draft.websockets = !this.draft.websockets; this.draft.websocket_touched = true; cx.notify(); }))))))
            .child(div().flex().items_start().child(self.picker("add-field", self.locale.text("providerForm.addField"), "+".into(),
                (3..9).filter(|index| !self.visible.contains(index)).map(|index| (index, self.locale.text(LABELS[index]))).collect(), cx)))
    }
}
