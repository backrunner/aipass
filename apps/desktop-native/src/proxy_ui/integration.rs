//! Reuses the provider diff viewer and setup guide; writes remain Agent-owned.
use super::*;
use crate::{agents_ui::setup, integrations::preview};
use aipass_agent_protocol::{ToolConfigMode, ToolConfigRequest, ToolConfigTool as Tool};
use aipass_desktop_native::integration::catalog;
pub(super) fn unavailable(tool: &Tool, route: &ProxyRouteConfig) -> Option<&'static str> {
    use aipass_proxy::Protocol::*;
    let protocol = route.inbound_protocol;
    let compatible = match tool {
        Tool::Codex => protocol == OpenAiResponses,
        Tool::ClaudeCode => protocol == AnthropicMessages,
        Tool::GeminiCli | Tool::AntigravityCli => {
            return Some("integration.geminiProxyUnsupported")
        }
        Tool::MuseCode => return Some("integration.modelCatalogRequired"),
        Tool::Codebuddy | Tool::Workbuddy | Tool::JetbrainsCopilot => {
            protocol == OpenAiChatCompletions
        }
        Tool::Trae | Tool::Zcode => protocol != OpenAiResponses,
        _ => true,
    };
    if !compatible {
        return Some("integration.protocolMismatch");
    }
    if catalog::requires_model(tool)
        && !route
            .targets
            .iter()
            .any(|t| t.enabled && t.model.as_ref().is_some_and(|m| !m.trim().is_empty()))
    {
        return Some("integration.defaultModelRequired");
    }
    None
}
impl Proxy {
    pub(super) fn show_tool_preview(
        &mut self,
        response: aipass_agent_protocol::ToolConfigPreviewResponse,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.new(|cx| {
            let mut view = preview::Preview::new(
                response,
                !self.preview_readonly,
                self.dark,
                self.locale.is_chinese(),
                window,
                cx,
            );
            view.proxy = true;
            view
        });
        self.preview_subscription =
            Some(cx.subscribe_in(&view, window, |s, _, event, window, cx| {
                if s.busy() {
                    return;
                }
                match event {
                    preview::Event::Close => {
                        s.preview = None;
                        s.preview_subscription = None;
                        s.plan = None;
                        if let Some(f) = s.previous_focus.take() {
                            f.focus(window, cx);
                        }
                    }
                    preview::Event::Confirm => {
                        if s.preview_readonly {
                            return;
                        }
                        if let Some(request) = s.plan.clone() {
                            s.request(Operation::ToolApply(request), false, cx);
                            if let Some(view) = &s.preview {
                                view.update(cx, |v, cx| {
                                    v.busy = true;
                                    v.error.clear();
                                    cx.notify();
                                });
                            }
                        }
                    }
                }
                cx.notify();
            }));
        self.preview = Some(view);
    }
    pub(super) fn show_setup(
        &mut self,
        tool: Tool,
        id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(config) = &self.config else {
            return;
        };
        let Some(route) = config.routes.iter().find(|r| r.id == id) else {
            return;
        };
        let endpoint = endpoint(config, route);
        let model = route
            .targets
            .iter()
            .find(|r| r.enabled)
            .and_then(|t| t.model.clone())
            .unwrap_or_default();
        let protocol = match route.inbound_protocol {
            aipass_proxy::Protocol::OpenAiResponses => "OpenAI Responses",
            aipass_proxy::Protocol::OpenAiChatCompletions => "OpenAI Chat Completions",
            aipass_proxy::Protocol::AnthropicMessages => "Anthropic Messages",
        }
        .into();
        self.previous_focus = window.focused(cx);
        let view = cx.new(|cx| {
            setup::Setup::for_proxy(
                ToolConfigRequest {
                    tool,
                    id,
                    secret_id: None,
                    mode: ToolConfigMode::Plaintext,
                    codex_api_key_mode: None,
                    preview_id: None,
                },
                endpoint,
                model,
                protocol,
                window,
                cx,
            )
        });
        view.update(cx, |v, cx| v.appearance(self.dark, cx));
        self.setup_subscription =
            Some(
                cx.subscribe_in(&view, window, move |s, _, event, window, cx| {
                    match event {
                        setup::Event::Close => {
                            s.cancel_copy();
                            s.setup = None;
                            s.setup_subscription = None;
                            if let Some(f) = s.previous_focus.take() {
                                f.focus(window, cx);
                            }
                        }
                        setup::Event::CopyKey => s.copy_token(id, window, cx),
                    }
                    cx.notify();
                }),
            );
        self.setup = Some(view);
    }
}

pub(super) fn endpoint(config: &ProxyConfig, route: &ProxyRouteConfig) -> String {
    format!(
        "http://{}{}",
        super::view::advertised(&config.bind_addr),
        if route.inbound_protocol == aipass_proxy::Protocol::AnthropicMessages {
            ""
        } else {
            "/v1"
        }
    )
}
