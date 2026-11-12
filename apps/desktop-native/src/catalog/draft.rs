//! Preserve untouched provider metadata while editing the same Agent DTOs.
use super::{lists, Action};
use aipass_agent_protocol::SensitiveString;
use aipass_provider_registry::{
    default_provider_definitions, provider_kind_for_id, AuthScheme, CredentialKind, EndpointKind,
    InterfaceType, ProviderEndpoint,
};
use aipass_vault::{EntrySummary, ProviderEntryInput, ProviderEntryUpdateInput};

pub struct Draft {
    pub original: Option<EntrySummary>,
    pub provider_id: String,
    pub interface: InterfaceType,
    pub auth: AuthScheme,
    pub auth_touched: bool,
    pub title: String,
    pub secret_label: String,
    pub api_key: SensitiveString,
    pub endpoint: String,
    pub console: String,
    pub domains: String,
    pub model: String,
    pub tags: String,
    pub notes: String,
    pub concurrency: String,
    pub concurrency_touched: bool,
    pub websockets: bool,
    pub websocket_touched: bool,
}

impl Draft {
    pub fn choose_interface(&mut self, interface: InterfaceType) {
        self.interface = interface;
        if !self.auth_touched || !auth_compatible(&self.auth, &self.interface) {
            self.auth = match self.interface {
                InterfaceType::OpenAiCompatible => AuthScheme::Bearer,
                InterfaceType::AnthropicMessages => AuthScheme::XApiKey,
                InterfaceType::Gemini => AuthScheme::GoogleApiKey,
                InterfaceType::AzureOpenAi => AuthScheme::AzureApiKey,
                InterfaceType::Bedrock => AuthScheme::AwsProfile,
                InterfaceType::CustomHttp => AuthScheme::CustomHeader,
            };
        }
    }
    pub fn new(original: Option<EntrySummary>) -> Self {
        let endpoint = |kind| {
            original
                .as_ref()
                .map(|e| {
                    lists::encode(
                        e.endpoints
                            .iter()
                            .filter(|ep| ep.kind == kind)
                            .filter_map(|ep| ep.url.as_deref()),
                    )
                })
                .unwrap_or_default()
        };
        let mut draft = Self {
            provider_id: "anthropic".into(),
            interface: InterfaceType::AnthropicMessages,
            auth: AuthScheme::XApiKey,
            auth_touched: original.is_some(),
            title: String::new(),
            secret_label: String::new(),
            api_key: SensitiveString::default(),
            endpoint: endpoint(EndpointKind::Api),
            console: endpoint(EndpointKind::Console),
            domains: String::new(),
            model: String::new(),
            tags: String::new(),
            notes: String::new(),
            concurrency: String::new(),
            concurrency_touched: false,
            websockets: true,
            websocket_touched: false,
            original: None,
        };
        if let Some(entry) = &original {
            draft.provider_id = entry
                .provider_id
                .clone()
                .unwrap_or_else(|| "custom_http".into());
            draft.interface = entry.interface_type.clone();
            draft.auth = entry.auth_scheme.clone();
            draft.title = entry.title.clone();
            draft.domains = lists::encode(&entry.domains);
            draft.model = entry.default_model.clone().unwrap_or_default();
            draft.tags = lists::encode(&entry.tags);
            draft.notes = entry.notes.clone().unwrap_or_default();
            draft.concurrency = entry
                .max_concurrent_requests
                .map(|n| n.to_string())
                .unwrap_or_default();
            draft.websockets = entry.supports_websockets.unwrap_or(true);
        }
        draft.original = original;
        draft
    }

    pub fn choose_provider(&mut self, id: String) {
        if let Some(definition) = default_provider_definitions()
            .into_iter()
            .find(|item| item.id == id)
        {
            if let Some(interface) = definition.interfaces.first() {
                self.interface = interface.clone();
            }
            if let Some(auth) = definition.auth_schemes.first() {
                self.auth = auth.clone();
            }
        }
        self.provider_id = id;
        self.auth_touched = true;
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.original.is_none() && self.api_key.expose().trim().is_empty() {
            return Err("providerForm.apiKeyRequired");
        }
        for value in lists::endpoints(&self.endpoint)
            .into_iter()
            .chain(lists::endpoints(&self.console))
        {
            if value.trim().is_empty() {
                continue;
            }
            if url::Url::parse(value.trim()).ok().is_none_or(|url| {
                !matches!(url.scheme(), "http" | "https")
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
            }) {
                return Err("providers.invalidEndpoint");
            }
        }
        if !self.concurrency.trim().is_empty() && self.concurrency.trim().parse::<u32>().is_err() {
            return Err("providerForm.invalidConcurrencyLimit");
        }
        Ok(())
    }

    pub fn into_action(self) -> Result<Action, &'static str> {
        self.validate()?;
        let optional = |text: &str| (!text.trim().is_empty()).then(|| text.trim().to_owned());
        let title = if self.title.trim().is_empty() {
            default_provider_definitions()
                .into_iter()
                .find(|definition| definition.id == self.provider_id)
                .map(|definition| definition.display_name.to_owned())
                .unwrap_or_else(|| self.provider_id.clone())
        } else {
            self.title.trim().to_owned()
        };
        let original = self.original.as_ref();
        let mut endpoints = original.map(|e| e.endpoints.clone()).unwrap_or_default();
        for (kind, value) in [
            (EndpointKind::Api, self.endpoint.trim()),
            (EndpointKind::Console, self.console.trim()),
        ] {
            let values = lists::endpoints(value);
            let previous: Vec<_> = endpoints
                .iter()
                .filter(|endpoint| endpoint.kind == kind)
                .cloned()
                .collect();
            if values.iter().map(String::as_str).eq(previous
                .iter()
                .filter_map(|endpoint| endpoint.url.as_deref()))
            {
                continue;
            }
            endpoints.retain(|endpoint| endpoint.kind != kind);
            let mut used = std::collections::HashSet::new();
            let reserved: std::collections::HashSet<_> = previous
                .iter()
                .enumerate()
                .filter(|(_, endpoint)| {
                    endpoint
                        .url
                        .as_ref()
                        .is_some_and(|url| values.contains(url))
                })
                .map(|(index, _)| index)
                .collect();
            endpoints.extend(values.iter().enumerate().map(|(index, url)| {
                let found = previous
                    .iter()
                    .enumerate()
                    .find(|(index, endpoint)| {
                        !used.contains(index) && endpoint.url.as_deref() == Some(url)
                    })
                    .map(|(index, _)| index)
                    .or_else(|| {
                        (index < previous.len()
                            && !used.contains(&index)
                            && !reserved.contains(&index))
                        .then_some(index)
                    });
                let mut endpoint = if let Some(index) = found {
                    used.insert(index);
                    previous[index].clone()
                } else {
                    let mut endpoint = if kind == EndpointKind::Api {
                        ProviderEndpoint::api(url)
                    } else {
                        ProviderEndpoint::console(url)
                    };
                    endpoint.id = format!("{}-{}", endpoint.id, uuid::Uuid::new_v4());
                    endpoint
                };
                endpoint.url = Some(url.clone());
                endpoint
            }));
        }
        let kind = original
            .filter(|entry| entry.provider_id.as_deref() == Some(&self.provider_id))
            .map(|entry| entry.provider_kind.clone())
            .unwrap_or_else(|| provider_kind_for_id(Some(&self.provider_id)));
        let concurrency = (original.is_none() || self.concurrency_touched)
            .then(|| self.concurrency.trim().parse::<u32>().unwrap_or(0));
        let websockets = (original.is_none() || self.websocket_touched).then_some(self.websockets);
        if let Some(entry) = original {
            Ok(Action::Update(
                entry.id,
                Box::new(ProviderEntryUpdateInput {
                    title,
                    provider_kind: kind,
                    provider_id: Some(self.provider_id),
                    credential_kind: None,
                    account_identity: None,
                    domains: lists::parse(&self.domains),
                    favicon_url: entry.favicon_url.clone(),
                    endpoints,
                    interface_type: self.interface,
                    auth_scheme: self.auth,
                    api_key: optional(self.api_key.expose()),
                    secret_label: None,
                    max_concurrent_requests: concurrency,
                    supports_websockets: websockets,
                    default_model: optional(&self.model),
                    model_aliases: entry.model_aliases.clone(),
                    headers: None,
                    quota: entry.quota.clone(),
                    subscription: None,
                    gateway: entry.gateway.clone(),
                    tags: lists::parse(&self.tags),
                    notes: optional(&self.notes),
                    secret_metadata: Default::default(),
                }),
            ))
        } else {
            Ok(Action::Add(Box::new(ProviderEntryInput {
                title,
                provider_kind: kind,
                provider_id: Some(self.provider_id),
                credential_kind: CredentialKind::Api,
                account_identity: None,
                domains: lists::parse(&self.domains),
                favicon_url: None,
                endpoints,
                interface_type: self.interface,
                auth_scheme: self.auth,
                api_key: self.api_key.expose().trim().into(),
                secret_label: optional(&self.secret_label),
                max_concurrent_requests: concurrency,
                supports_websockets: websockets,
                default_model: optional(&self.model),
                model_aliases: vec![],
                headers: vec![],
                quota: None,
                subscription: None,
                gateway: None,
                tags: lists::parse(&self.tags),
                notes: optional(&self.notes),
                secret_metadata: Default::default(),
            })))
        }
    }
}

/// Keep the same choices as the shared provider form and proxy protocols.
pub fn auth_compatible(auth: &AuthScheme, interface: &InterfaceType) -> bool {
    match interface {
        InterfaceType::OpenAiCompatible => matches!(auth, AuthScheme::Bearer),
        InterfaceType::AnthropicMessages => {
            matches!(auth, AuthScheme::XApiKey | AuthScheme::Bearer)
        }
        InterfaceType::Gemini => matches!(auth, AuthScheme::GoogleApiKey),
        InterfaceType::AzureOpenAi => matches!(auth, AuthScheme::AzureApiKey | AuthScheme::Bearer),
        InterfaceType::Bedrock => matches!(auth, AuthScheme::AwsProfile),
        InterfaceType::CustomHttp => matches!(
            auth,
            AuthScheme::Bearer
                | AuthScheme::XApiKey
                | AuthScheme::AzureApiKey
                | AuthScheme::CustomHeader
        ),
    }
}
