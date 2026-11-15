//! Reuse the shared provider artwork; monochrome brands retain the UI tone.
use aipass_provider_registry::{CredentialKind, ProviderKind};
use gpui::{div, img, prelude::*, px, svg, Div, FontWeight};

pub fn name(id: &str, kind: CredentialKind) -> &str {
    if kind == CredentialKind::OAuth {
        match id {
            "anthropic" => return "claude",
            "openai" => return "codex",
            "xai" => return "grok",
            "google" => return "gemini",
            _ => {}
        }
    }
    match id {
        "claude-code" => "claude",
        "gemini-cli" => "gemini",
        "grok-build" => "grok",
        "commandcode-plan" => "commandcode",
        "zcode" => "zai",
        "qoder-cn" => "qoder",
        "mimo-plan" | "mimo-app" => "mimo",
        "trae-cn" | "trae-global" => "trae",
        "opencode-go" | "opencode-zen" => "opencode",
        "kilo-gateway" => "kilocode",
        "ollama-cloud" => "ollama",
        "kimi-work" | "kimi-code" | "kimi-code-cn" => "moonshot",
        "minimax-code" | "minimax-code-global" | "minimax-m-plan" | "minimax-m-plan-cn" => {
            "minimax"
        }
        "bailian-token-plan" | "bailian-coding-plan" => "alibaba",
        "stepfun-plan" | "stepfun-plan-cn" => "stepfun",
        "qwen-token-plan" => "qwen",
        "volcengine-coding-plan" | "volcengine-agent-plan" => "volcengine",
        "tencent-token-plan" | "tencent-token-plan-enterprise" => "tencentcloud",
        "workbuddy-ai" => "workbuddy",
        _ => id,
    }
}
pub fn asset(path: &str) -> Option<&'static [u8]> {
    let name = path.strip_prefix("provider-brand/")?.strip_suffix(".svg")?;
    macro_rules! brands {($($name:literal),* $(,)?)=> {match name {$($name=>Some(include_bytes!(concat!("../../../packages/ui/src/assets/provider-icons/",$name,".svg")).as_slice()),)*_=>None}};}
    brands!(
        "alibaba",
        "anthropic",
        "antigravity",
        "azure",
        "bedrock",
        "cerebras",
        "claude",
        "cline",
        "codex",
        "commandcode",
        "copilot",
        "cursor",
        "devin",
        "factory",
        "gemini",
        "grok",
        "kiro",
        "kilocode",
        "minimax",
        "mimo",
        "moonshot",
        "ollama",
        "opencode",
        "openai",
        "qoder",
        "qwen",
        "stepfun",
        "tencentcloud",
        "trae",
        "volcengine",
        "workbuddy",
        "xai",
        "zai",
        "zed"
    )
}
pub fn icon(
    id: &str,
    title: &str,
    kind: CredentialKind,
    provider_kind: &ProviderKind,
    size: f32,
    dark: bool,
) -> Div {
    let color = crate::providers::presentation::tone(provider_kind, dark);
    let name = name(id, kind);
    let path = format!("provider-brand/{name}.svg");
    let monochrome = matches!(
        name,
        "kilocode"
            | "tencentcloud"
            | "trae"
            | "opencode"
            | "antigravity"
            | "cline"
            | "anthropic"
            | "cerebras"
            | "codex"
            | "commandcode"
            | "copilot"
            | "cursor"
            | "factory"
            | "grok"
            | "mimo"
            | "moonshot"
            | "ollama"
            | "openai"
            | "qoder"
            | "xai"
            | "zai"
            | "zed"
    );
    let known = asset(&path).is_some();
    div()
        .size(px(size))
        .flex_shrink_0()
        .overflow_hidden()
        .rounded(px(if size >= 48. { 10. } else { 8. }))
        .bg(color.opacity(if dark { 0.16 } else { 0.1 }))
        .text_color(color)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(if size >= 48. {
            16.
        } else if size <= 24. {
            10.
        } else {
            11.
        }))
        .when(known, |el| {
            if monochrome {
                el.child(
                    svg()
                        .path(path.clone())
                        .size(px(size * 0.6))
                        .text_color(color),
                )
            } else {
                el.child(img(path.clone()).size(px(size * 0.6)))
            }
        })
        .when(!known, |el| {
            el.child(
                div().font_weight(FontWeight::SEMIBOLD).child(
                    title
                        .trim()
                        .chars()
                        .next()
                        .unwrap_or('?')
                        .to_uppercase()
                        .to_string(),
                ),
            )
        })
}
