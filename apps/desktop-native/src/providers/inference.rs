//! Infer on blur, without replacing explicit provider/protocol choices or text.
use super::Form;
use aipass_desktop_native::catalog::lists;
use aipass_provider_registry::{
    inference::infer_provider_from_endpoint, match_provider_by_domain, EndpointKind,
};
use gpui::{Context, Window};

impl Form {
    pub(super) fn infer_provider(
        &mut self,
        field: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        let value = self.inputs[field].read(cx).value();
        let first = if field == 3 {
            lists::endpoints(&value)
        } else {
            lists::parse(&value)
        }
        .into_iter()
        .next()
        .unwrap_or_default();
        let provider = if field == 3 {
            infer_provider_from_endpoint(&first)
        } else {
            match_provider_by_domain(&first)
        };
        let Some(provider) = provider else {
            return;
        };
        if !self.draft.provider_touched {
            self.draft.provider_id = provider.id.into();
        }
        if !self.draft.interface_touched {
            if let Some(interface) = provider.interfaces.first() {
                self.draft.interface = interface.clone();
            }
        }
        if !self.draft.auth_touched {
            if let Some(auth) = provider.auth_schemes.first() {
                self.draft.auth = auth.clone();
            }
        }
        if self.inputs[0].read(cx).value().is_empty() {
            self.inputs[0].update(cx, |s, cx| s.set_value(provider.display_name, window, cx));
        }
        if field == 7 && self.inputs[3].read(cx).value().is_empty() {
            if let Some((_, _, endpoint)) = provider
                .endpoints
                .iter()
                .find(|(_, kind, _)| *kind == EndpointKind::Api)
            {
                self.inputs[3].update(cx, |s, cx| s.set_value(*endpoint, window, cx));
                self.visible.insert(3);
            }
        }
        if field == 7 && self.draft.original.is_none() && self.draft.imported.favicon_url.is_none()
        {
            let url = if first.contains("://") {
                first
            } else {
                format!("https://{first}")
            };
            self.draft.imported.favicon_url = url::Url::parse(&url).ok().and_then(|url| {
                url.host_str()
                    .map(|host| format!("https://{host}/favicon.ico"))
            });
        }
        cx.notify();
    }
}

#[cfg(all(test, feature = "parity-render"))]
mod tests {
    use super::*;
    #[gpui::test]
    fn endpoint_and_domain_inference_respect_explicit_choices(cx: &mut gpui::TestAppContext) {
        use aipass_provider_registry::{AuthScheme, InterfaceType};
        cx.update(gpui_base::init);
        let (form, cx) = cx.add_window_view(|w, cx| Form::new(None, false, false, w, cx));
        cx.update(|w, cx| {
            form.update(cx, |s, cx| {
                s.inputs[3].update(cx, |s, cx| s.set_value("https://api.openai.com/v1", w, cx));
                s.infer_provider(3, w, cx);
                assert_eq!(s.draft.provider_id, "openai");
                assert_eq!(s.draft.interface, InterfaceType::OpenAiCompatible);
                assert_eq!(s.draft.auth, AuthScheme::Bearer);
                assert_eq!(s.inputs[0].read(cx).value(), "OpenAI");
                s.draft.choose_provider("anthropic".into());
                s.inputs[0].update(cx, |s, cx| s.set_value("Keep my title", w, cx));
                s.infer_provider(3, w, cx);
                assert_eq!(s.draft.provider_id, "anthropic");
                assert_eq!(s.draft.interface, InterfaceType::AnthropicMessages);
                assert_eq!(s.inputs[0].read(cx).value(), "Keep my title");
                s.inputs[3].update(cx, |s, cx| s.set_value("", w, cx));
                s.inputs[7].update(cx, |s, cx| s.set_value("console.anthropic.com", w, cx));
                s.infer_provider(7, w, cx);
                assert_eq!(s.inputs[3].read(cx).value(), "https://api.anthropic.com");
                assert_eq!(
                    s.draft.imported.favicon_url.as_deref(),
                    Some("https://console.anthropic.com/favicon.ico")
                );
            })
        });
    }
}
