//! Shared native provider identity and credential presentation.
use crate::parity::Observe;
use crate::{locale::Locale, theme::Theme};
use aipass_provider_registry::{CredentialKind, InterfaceType, ProviderKind};
use aipass_vault::EntrySummary;
use gpui::{div, prelude::*, px, rgb, Div, FontWeight, Rgba};

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
    crate::provider_icon::icon(
        entry.provider_id.as_deref().unwrap_or_default(),
        &entry.title,
        entry.credential_kind,
        &entry.provider_kind,
        if large { 48. } else { 32. },
        dark,
    )
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
                if entry.credential_kind == CredentialKind::Api {
                    locale.text(protocol_key(&entry.interface_type))
                } else {
                    String::new()
                },
                if entry.credential_kind == CredentialKind::Api {
                    "API".into()
                } else {
                    locale.text("providerDetail.oauth")
                },
            ]
            .into_iter()
            .filter(|label| !label.is_empty())
            .enumerate()
            .map(|(index, label)| {
                div()
                    .flex_shrink_0()
                    .px(px(8.))
                    .py(px(3.))
                    .rounded_full()
                    .border_1()
                    .border_color(if index == 0 { gpui::rgba(0) } else { t.border })
                    .when(index == 0, |el| {
                        el.bg(color.opacity(if dark { 0.16 } else { 0.1 }))
                    })
                    .text_size(px(11.))
                    .line_height(px(11.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if index == 0 { color } else { t.secondary })
                    .child(label)
                    .observe(format!("provider-badge-{index}"))
            }),
        )
}
