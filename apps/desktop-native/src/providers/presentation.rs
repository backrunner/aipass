//! Shared native provider identity and credential presentation.
use crate::{locale::Locale, theme::Theme};
use aipass_provider_registry::{CredentialKind, InterfaceType, ProviderKind};
use aipass_vault::EntrySummary;
use gpui::{div, prelude::*, px, rgb, svg, Div, FontWeight, Rgba};

pub fn tone(kind: &ProviderKind, dark: bool) -> Rgba {
    rgb(match (kind, dark) {
        (ProviderKind::Official, false) => 0x2563eb,
        (ProviderKind::Official, true) => 0x6c9bff,
        (ProviderKind::ThirdParty, false) => 0xb45309,
        (ProviderKind::ThirdParty, true) => 0xd0a25e,
        (ProviderKind::SelfHosted, false) => 0x475569,
        (ProviderKind::SelfHosted, true) => 0x9aa1b4,
        (ProviderKind::Unknown, false) => 0x6b7385,
        (ProviderKind::Unknown, true) => 0x9aa3b8,
    })
}

pub fn identity(entry: &EntrySummary, large: bool, dark: bool) -> Div {
    let color = tone(&entry.provider_kind, dark);
    let size = if large { 48. } else { 32. };
    let brand = match (entry.provider_id.as_deref(), entry.credential_kind) {
        (Some("anthropic"), CredentialKind::Api) => Some("brand-anthropic"),
        (Some("openai"), CredentialKind::Api) => Some("brand-openai"),
        _ => None,
    };
    div()
        .size(px(size))
        .flex_shrink_0()
        .rounded(px(if large { 10. } else { 8. }))
        .bg(color.opacity(if dark { 0.16 } else { 0.1 }))
        .text_color(color)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(if large { 16. } else { 11. }))
        .font_weight(FontWeight::SEMIBOLD)
        .when_some(brand, |el, brand| {
            el.child(svg().path(brand).size(px(size * 0.6)).text_color(color))
        })
        .when(brand.is_none(), |el| {
            el.child(
                entry
                    .title
                    .trim()
                    .chars()
                    .next()
                    .unwrap_or('?')
                    .to_uppercase()
                    .to_string(),
            )
        })
}

pub fn protocol_key(interface: &InterfaceType) -> &'static str {
    match interface {
        InterfaceType::OpenAiCompatible => "interface.openaiCompatible",
        InterfaceType::AnthropicMessages => "interface.anthropicMessages",
        InterfaceType::Gemini => "interface.gemini",
        InterfaceType::AzureOpenAi => "interface.azureOpenai",
        InterfaceType::Bedrock => "interface.bedrock",
        InterfaceType::CustomHttp => "interface.customHttp",
    }
}

pub fn badges(entry: &EntrySummary, t: Theme, locale: &Locale, dark: bool) -> Div {
    let color = tone(&entry.provider_kind, dark);
    let kind = match entry.provider_kind {
        ProviderKind::Official => "sidebar.official",
        ProviderKind::ThirdParty => "sidebar.thirdParty",
        ProviderKind::SelfHosted => "sidebar.selfHosted",
        ProviderKind::Unknown => "sidebar.custom",
    };
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .overflow_hidden()
        .children(
            [
                locale.text(kind),
                locale.text(protocol_key(&entry.interface_type)),
                if entry.credential_kind == CredentialKind::Api {
                    "API".into()
                } else {
                    "OAuth".into()
                },
            ]
            .into_iter()
            .enumerate()
            .map(|(index, label)| {
                div()
                    .flex_shrink_0()
                    .px(px(8.))
                    .py(px(2.))
                    .rounded_full()
                    .border_1()
                    .border_color(if index == 0 { gpui::rgba(0) } else { t.border })
                    .when(index == 0, |el| {
                        el.bg(color.opacity(if dark { 0.16 } else { 0.1 }))
                    })
                    .text_size(px(11.))
                    .line_height(px(14.))
                    .text_color(if index == 0 { color } else { t.secondary })
                    .child(label)
            }),
        )
}
