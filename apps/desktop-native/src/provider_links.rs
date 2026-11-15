//! Provider links prepare an editable draft; receiving a link never writes data.
use crate::catalog::{draft::Draft, lists, optional, Catalog};
use aipass_desktop_platform::deeplink::{self, PendingDeepLink};
use aipass_provider_registry::{
    default_provider_definitions, AuthScheme, EndpointKind, InterfaceType,
};
use aipass_vault::EntrySummary;
use url::Url;
use zeroize::Zeroizing;

pub const QUEUE_LIMIT: usize = 16;

/// Parse at the OS boundary and discard raw URL text, which may contain secrets.
pub fn parse(raw: String) -> Option<PendingDeepLink> {
    let raw = Zeroizing::new(raw);
    // Match the existing provider-link parser's bounded payload, and never put
    // an unvalidated URL or parser diagnostic containing it into a log.
    if raw.len() > 1024 * 1024 {
        return None;
    }
    let url = Url::parse(&raw).ok()?;
    match url.scheme() {
        "ccswitch" => Some(match deeplink::parse_ccswitch_link(&url) {
            Ok(link) => PendingDeepLink::CcSwitch(Box::new(link)),
            Err(error) => PendingDeepLink::CcSwitchError(error.payload()),
        }),
        "aipass-provider" => Some(match deeplink::parse_aipass_provider_link(&url) {
            Ok(link) => PendingDeepLink::AipassProvider(Box::new(link)),
            Err(error) => PendingDeepLink::AipassProviderError(error.payload()),
        }),
        _ => None,
    }
}

pub struct Message {
    pub key: &'static str,
    pub parameter: &'static str,
    pub value: String,
}

pub fn into_draft(link: PendingDeepLink) -> Result<Draft, Message> {
    let mut draft = Draft::new(None);
    match link {
        PendingDeepLink::CcSwitch(link) => {
            let app = link.app.trim().to_ascii_lowercase();
            let (endpoint, provider) = match app.as_str() {
                "claude" => ("https://api.anthropic.com", "anthropic"),
                "codex" => ("https://api.openai.com/v1", "openai"),
                "gemini" => ("https://generativelanguage.googleapis.com", "gemini"),
                _ => ("", ""),
            };
            let endpoints = lists::endpoints(link.endpoint.as_deref().unwrap_or_default());
            let official = !provider.is_empty()
                && endpoints
                    .iter()
                    .all(|value| identity(value) == identity(endpoint));
            let interface = if app == "claude" {
                InterfaceType::AnthropicMessages
            } else {
                InterfaceType::OpenAiCompatible
            };
            draft.choose_provider(if official {
                provider.into()
            } else if app == "claude" {
                "custom_http".into()
            } else {
                "custom_openai_compatible".into()
            });
            draft.interface = interface;
            draft.auth = AuthScheme::Bearer;
            draft.auth_touched = true;
            draft.provider_touched = official;
            draft.interface_touched = true;
            draft.title = link.name;
            draft.domains = link
                .homepage
                .as_deref()
                .and_then(|value| Url::parse(value).ok())
                .and_then(|url| url.host_str().map(str::to_owned))
                .unwrap_or_default();
            draft.endpoint = lists::encode(endpoints);
            draft.api_key = link.api_key.unwrap_or_default();
            draft.model = link.model.or(link.sonnet_model).unwrap_or_default();
            draft.notes = link.notes.unwrap_or_default();
        }
        PendingDeepLink::AipassProvider(link) => {
            let explicit_provider = link.provider_id.is_some();
            let explicit_interface = link.interface_type.is_some();
            let explicit_auth = link.auth_scheme.is_some();
            if let Some(provider) = link.provider_id {
                let known = default_provider_definitions()
                    .iter()
                    .any(|p| p.id == provider);
                draft.choose_provider(if known {
                    provider
                } else if link.interface_type == Some(InterfaceType::OpenAiCompatible) {
                    "custom_openai_compatible".into()
                } else {
                    "custom_http".into()
                });
            } else {
                draft.choose_provider(
                    if link.interface_type == Some(InterfaceType::OpenAiCompatible) {
                        "custom_openai_compatible".into()
                    } else {
                        "custom_http".into()
                    },
                );
            }
            if let Some(interface) = link.interface_type {
                draft.choose_interface(interface);
            }
            if let Some(auth) = link.auth_scheme {
                draft.auth = auth;
            }
            draft.auth_touched = true;
            draft.title = link.title;
            draft.domains = lists::encode(link.domains);
            draft.endpoint = lists::encode(link.endpoints);
            draft.console = lists::encode(link.console_endpoints);
            draft.api_key = link.api_key.unwrap_or_default();
            draft.secret_label = link.secret_label.unwrap_or_default();
            draft.model = link.default_model.unwrap_or_default();
            draft.tags = lists::encode(link.tags);
            draft.notes = link.notes.unwrap_or_default();
            draft.add_fields.aliases = optional::encode_pairs(&link.model_aliases);
            // Temporary plain assignments exist only while preparing the secret editor.
            let escape = |value: &str| {
                value
                    .replace('\\', "\\\\")
                    .replace(',', "\\,")
                    .replace('=', "\\=")
            };
            let headers = Zeroizing::new(
                link.headers
                    .iter()
                    .map(|(key, value)| format!("{}={}", escape(key), escape(value.expose())))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            draft.add_fields.headers = headers.as_str().into();
            draft.imported = optional::ImportedMetadata {
                credential_kind: link.credential_kind,
                account_identity: link.account_identity,
                favicon_url: link.favicon_url,
                quota: link.quota,
            };
            draft.provider_touched = explicit_provider;
            draft.interface_touched = explicit_interface;
            draft.auth_touched = explicit_auth;
        }
        PendingDeepLink::CcSwitchError(error) => {
            return Err(match error.unsupported {
                Some(resource) => Message {
                    key: "deepLink.unsupportedResource",
                    parameter: "{type}",
                    value: resource,
                },
                None => Message {
                    key: "deepLink.importFailed",
                    parameter: "{message}",
                    value: error.message,
                },
            })
        }
        PendingDeepLink::AipassProviderError(error) => {
            return Err(Message {
                key: "deepLink.importFailed",
                parameter: "{message}",
                value: error.message,
            })
        }
    }
    Ok(draft)
}

pub fn duplicate<'a>(catalog: &'a Catalog, draft: &Draft) -> Option<&'a EntrySummary> {
    let endpoints: Vec<_> = lists::endpoints(&draft.endpoint)
        .iter()
        .map(|e| identity(e))
        .collect();
    catalog.active.iter().find(|entry| {
        (!draft.title.trim().is_empty() && entry.title == draft.title.trim())
            || entry.endpoints.iter().any(|endpoint| {
                endpoint.kind == EndpointKind::Api
                    && endpoint
                        .url
                        .as_ref()
                        .is_some_and(|url| endpoints.contains(&identity(url)))
            })
    })
}

fn identity(raw: &str) -> String {
    match Url::parse(raw.trim()) {
        Ok(url) => format!(
            "{}{}{}",
            url.origin().ascii_serialization(),
            url.path().trim_end_matches('/'),
            url.query().map(|q| format!("?{q}")).unwrap_or_default()
        ),
        Err(_) => raw.trim().trim_end_matches('/').to_ascii_lowercase(),
    }
}

#[cfg(test)]
mod tests;
