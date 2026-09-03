use aipass_agent_protocol::SensitiveString;
use aipass_provider_registry::{AuthScheme, CredentialKind, InterfaceType, QuotaInfo};
use base64::Engine;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

const CCSWITCH_SCHEME: &str = "ccswitch";
const CCSWITCH_V1_HOST: &str = "v1";
const CCSWITCH_V1_IMPORT_PATH: &str = "/import";
const CCSWITCH_LEGACY_HOST: &str = "provider";
const CCSWITCH_DEFAULT_APP: &str = "claude";
const CCSWITCH_APPS: [&str; 7] = [
    "claude",
    "codex",
    "gemini",
    "grokbuild",
    "opencode",
    "openclaw",
    "hermes",
];
const AIPASS_PROVIDER_SCHEME: &str = "aipass-provider";
const AIPASS_PROVIDER_V1_HOST: &str = "v1";
const AIPASS_PROVIDER_ADD_PATH: &str = "/add";

/// A provider record expressed in AIPass storage vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AipassProviderLink {
    pub title: String,
    pub provider_id: Option<String>,
    pub credential_kind: Option<CredentialKind>,
    pub account_identity: Option<String>,
    pub domains: Vec<String>,
    pub endpoints: Vec<String>,
    pub console_endpoints: Vec<String>,
    pub favicon_url: Option<String>,
    pub interface_type: Option<InterfaceType>,
    pub auth_scheme: Option<AuthScheme>,
    pub api_key: Option<SensitiveString>,
    pub secret_label: Option<String>,
    pub default_model: Option<String>,
    pub model_aliases: Vec<(String, String)>,
    pub headers: Vec<(String, SensitiveString)>,
    pub quota: Option<QuotaInfo>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AipassProviderLinkErrorPayload {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AipassProviderLinkError {
    Invalid(String),
}

impl AipassProviderLinkError {
    pub(crate) fn payload(&self) -> AipassProviderLinkErrorPayload {
        match self {
            Self::Invalid(message) => AipassProviderLinkErrorPayload {
                message: message.clone(),
            },
        }
    }
}

/// Payload of the `ccswitch-provider-import` event, serialized camelCase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CcSwitchProviderLink {
    pub name: String,
    pub app: String,
    pub homepage: Option<String>,
    pub endpoint: Option<String>,
    pub api_key: Option<SensitiveString>,
    pub model: Option<String>,
    pub notes: Option<String>,
    pub haiku_model: Option<String>,
    pub sonnet_model: Option<String>,
    pub opus_model: Option<String>,
    pub icon: Option<String>,
}

/// A deep link buffered before the frontend is ready. Keeping both link types
/// in one queue preserves the order in which the operating system delivered
/// them during cold start.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "payload")]
pub(crate) enum PendingDeepLink {
    #[serde(rename = "ccSwitch")]
    CcSwitch(CcSwitchProviderLink),
    #[serde(rename = "aipassProvider")]
    AipassProvider(AipassProviderLink),
    #[serde(rename = "ccSwitchError")]
    CcSwitchError(CcSwitchLinkErrorPayload),
    #[serde(rename = "aipassProviderError")]
    AipassProviderError(AipassProviderLinkErrorPayload),
}

/// Payload of the `ccswitch-provider-import-error` event, serialized camelCase.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CcSwitchLinkErrorPayload {
    pub message: String,
    pub unsupported: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CcSwitchLinkError {
    /// The link is valid but imports a non-provider resource (prompt/mcp/skill).
    Unsupported(String),
    Invalid(String),
}

impl CcSwitchLinkError {
    pub(crate) fn payload(&self) -> CcSwitchLinkErrorPayload {
        match self {
            CcSwitchLinkError::Unsupported(resource) => CcSwitchLinkErrorPayload {
                message: format!("ccswitch link imports an unsupported resource: {resource}"),
                unsupported: Some(resource.clone()),
            },
            CcSwitchLinkError::Invalid(message) => CcSwitchLinkErrorPayload {
                message: message.clone(),
                unsupported: None,
            },
        }
    }
}

fn invalid(message: impl Into<String>) -> CcSwitchLinkError {
    CcSwitchLinkError::Invalid(message.into())
}

/// Returns None for absent or empty query params.
fn query_param(params: &[(String, String)], key: &str) -> Option<String> {
    params
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn query_params(params: &[(String, String)], key: &str) -> Vec<String> {
    params
        .iter()
        .filter(|(name, value)| name == key && !value.trim().is_empty())
        .map(|(_, value)| value.trim().to_string())
        .collect()
}

fn parse_json_param<T: DeserializeOwned>(
    params: &[(String, String)],
    key: &str,
) -> Result<Option<T>, AipassProviderLinkError> {
    let Some(raw) = query_param(params, key) else {
        return Ok(None);
    };
    serde_json::from_str(&raw).map(Some).map_err(|_| {
        AipassProviderLinkError::Invalid(format!("aipass-provider link has invalid {key}"))
    })
}

fn parse_enum_param<T: DeserializeOwned>(
    params: &[(String, String)],
    key: &str,
) -> Result<Option<T>, AipassProviderLinkError> {
    query_param(params, key)
        .map(|value| {
            serde_json::from_value(Value::String(value)).map_err(|_| {
                AipassProviderLinkError::Invalid(format!("aipass-provider link has invalid {key}"))
            })
        })
        .transpose()
}

fn require_http_url(value: &str, field: &str) -> Result<(), CcSwitchLinkError> {
    let parsed = url::Url::parse(value)
        .map_err(|_| invalid(format!("ccswitch link has an invalid {field} URL")))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || parsed.host_str().is_some_and(|host| host.contains(','))
    {
        return Err(invalid(format!("ccswitch link {field} must use http(s)")));
    }
    Ok(())
}

fn split_endpoint_list(raw: &str) -> Vec<&str> {
    // Commas are valid in URL paths and query values. Treat a comma as a list
    // separator only when the next item clearly starts another HTTP(S) URL.
    let mut items = Vec::new();
    let mut start = 0;
    for (index, _) in raw.match_indices(',') {
        let rest = raw[index + 1..].trim_start();
        if rest.starts_with("http://") || rest.starts_with("https://") {
            let item = raw[start..index].trim();
            if !item.is_empty() {
                items.push(item);
            }
            start = index + 1;
        }
    }
    let item = raw[start..].trim();
    if !item.is_empty() {
        items.push(item);
    }
    items
}

/// `endpoint` may be a comma-separated list; every item must be http(s).
fn validate_endpoint(endpoint: &Option<String>) -> Result<(), CcSwitchLinkError> {
    if let Some(endpoint) = endpoint {
        let items = split_endpoint_list(endpoint);
        if items.is_empty() {
            return Err(invalid("ccswitch link endpoint list is empty"));
        }
        for item in items {
            require_http_url(item, "endpoint")?;
        }
    }
    Ok(())
}

fn validate_aipass_endpoints(
    values: &[String],
    field: &str,
) -> Result<(), AipassProviderLinkError> {
    for value in values {
        let parsed = url::Url::parse(value).map_err(|_| {
            AipassProviderLinkError::Invalid(format!(
                "aipass-provider link has an invalid {field} URL"
            ))
        })?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return Err(AipassProviderLinkError::Invalid(format!(
                "aipass-provider link {field} must use http(s)"
            )));
        }
    }
    Ok(())
}

fn validate_aipass_favicon(value: &str) -> Result<(), AipassProviderLinkError> {
    const MAX_DATA_URI_BYTES: usize = 512 * 1024;
    let value = value.trim();
    if value.is_empty() || value.len() > MAX_DATA_URI_BYTES || value.chars().any(char::is_control) {
        return Err(AipassProviderLinkError::Invalid(
            "aipass-provider link has an invalid faviconUrl".to_string(),
        ));
    }
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        let parsed = url::Url::parse(value).map_err(|_| {
            AipassProviderLinkError::Invalid(
                "aipass-provider link has an invalid faviconUrl URL".to_string(),
            )
        })?;
        if parsed.host_str().is_none() {
            return Err(AipassProviderLinkError::Invalid(
                "aipass-provider link faviconUrl must include a host".to_string(),
            ));
        }
        return Ok(());
    }

    let Some((metadata, encoded)) = value.split_once(",") else {
        return Err(AipassProviderLinkError::Invalid(
            "aipass-provider link faviconUrl must be http(s) or a base64 data URI".to_string(),
        ));
    };
    let metadata = metadata.to_ascii_lowercase();
    if !metadata.starts_with("data:image/") || !metadata.contains(";base64") || encoded.is_empty() {
        return Err(AipassProviderLinkError::Invalid(
            "aipass-provider link faviconUrl must be a base64 image data URI".to_string(),
        ));
    }
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| {
            AipassProviderLinkError::Invalid(
                "aipass-provider link faviconUrl has invalid base64 data".to_string(),
            )
        })?;
    Ok(())
}

pub(crate) fn parse_aipass_provider_link(
    url: &url::Url,
) -> Result<AipassProviderLink, AipassProviderLinkError> {
    if url.scheme() != AIPASS_PROVIDER_SCHEME {
        return Err(AipassProviderLinkError::Invalid(format!(
            "unsupported aipass-provider link scheme: {}",
            url.scheme()
        )));
    }
    if url.host_str() != Some(AIPASS_PROVIDER_V1_HOST) || url.path() != AIPASS_PROVIDER_ADD_PATH {
        return Err(AipassProviderLinkError::Invalid(
            "unsupported aipass-provider link path; expected aipass-provider://v1/add".to_string(),
        ));
    }
    let params: Vec<(String, String)> = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    let title = query_param(&params, "title").ok_or_else(|| {
        AipassProviderLinkError::Invalid("aipass-provider link is missing a title".to_string())
    })?;
    let domains = query_params(&params, "domain");
    let endpoints = query_params(&params, "endpoint");
    let console_endpoints = query_params(&params, "consoleEndpoint");
    validate_aipass_endpoints(&endpoints, "endpoint")?;
    validate_aipass_endpoints(&console_endpoints, "consoleEndpoint")?;
    if let Some(favicon) = query_param(&params, "faviconUrl") {
        validate_aipass_favicon(&favicon)?;
    }
    Ok(AipassProviderLink {
        title,
        provider_id: query_param(&params, "providerId"),
        credential_kind: parse_enum_param(&params, "credentialKind")?,
        account_identity: query_param(&params, "accountIdentity"),
        domains,
        endpoints,
        console_endpoints,
        favicon_url: query_param(&params, "faviconUrl"),
        interface_type: parse_enum_param(&params, "interfaceType")?,
        auth_scheme: parse_enum_param(&params, "authScheme")?,
        api_key: query_param(&params, "apiKey").map(SensitiveString::new),
        secret_label: query_param(&params, "secretLabel"),
        default_model: query_param(&params, "defaultModel"),
        model_aliases: parse_json_param(&params, "modelAliases")?.unwrap_or_default(),
        headers: parse_json_param::<Vec<(String, String)>>(&params, "headers")?
            .unwrap_or_default()
            .into_iter()
            .map(|(name, value)| (name, SensitiveString::new(value)))
            .collect(),
        quota: parse_json_param(&params, "quota")?,
        tags: query_params(&params, "tag"),
        notes: query_param(&params, "notes"),
    })
}

pub(crate) fn parse_ccswitch_link(
    url: &url::Url,
) -> Result<CcSwitchProviderLink, CcSwitchLinkError> {
    if url.scheme() != CCSWITCH_SCHEME {
        return Err(invalid(format!(
            "unsupported ccswitch link scheme: {}",
            url.scheme()
        )));
    }
    let params: Vec<(String, String)> = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    match url.host_str() {
        Some(CCSWITCH_V1_HOST) => parse_v1_import(url, &params),
        Some(CCSWITCH_LEGACY_HOST) => parse_legacy_provider(&params),
        Some(host) => Err(invalid(format!("unsupported ccswitch link host: {host}"))),
        None => Err(invalid("ccswitch link is missing a host")),
    }
}

fn parse_v1_import(
    url: &url::Url,
    params: &[(String, String)],
) -> Result<CcSwitchProviderLink, CcSwitchLinkError> {
    if url.path() != CCSWITCH_V1_IMPORT_PATH {
        return Err(invalid(format!(
            "unsupported ccswitch v1 path: {}",
            url.path()
        )));
    }
    // config/configFormat/configUrl/usage*/enabled params are intentionally ignored.
    if let Some(resource) = query_param(params, "resource") {
        if resource != "provider" {
            return Err(CcSwitchLinkError::Unsupported(resource));
        }
    }
    let app = match query_param(params, "app") {
        Some(app) if CCSWITCH_APPS.contains(&app.as_str()) => app,
        Some(app) => {
            return Err(invalid(format!("unsupported ccswitch app: {app}")));
        }
        None => CCSWITCH_DEFAULT_APP.to_string(),
    };
    let name = query_param(params, "name")
        .ok_or_else(|| invalid("ccswitch link is missing a provider name"))?;
    let homepage = query_param(params, "homepage");
    if let Some(homepage) = &homepage {
        require_http_url(homepage, "homepage")?;
    }
    let endpoint = query_param(params, "endpoint");
    validate_endpoint(&endpoint)?;
    Ok(CcSwitchProviderLink {
        name,
        app,
        homepage,
        endpoint,
        api_key: query_param(params, "apiKey").map(SensitiveString::new),
        model: query_param(params, "model"),
        notes: query_param(params, "notes"),
        haiku_model: query_param(params, "haikuModel"),
        sonnet_model: query_param(params, "sonnetModel"),
        opus_model: query_param(params, "opusModel"),
        icon: query_param(params, "icon"),
    })
}

fn parse_legacy_provider(
    params: &[(String, String)],
) -> Result<CcSwitchProviderLink, CcSwitchLinkError> {
    let name = query_param(params, "name")
        .ok_or_else(|| invalid("ccswitch link is missing a provider name"))?;
    let endpoint = query_param(params, "base_url");
    validate_endpoint(&endpoint)?;
    Ok(CcSwitchProviderLink {
        name,
        app: CCSWITCH_DEFAULT_APP.to_string(),
        homepage: None,
        endpoint,
        api_key: query_param(params, "api_key").map(SensitiveString::new),
        model: None,
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        icon: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Result<CcSwitchProviderLink, CcSwitchLinkError> {
        let url = url::Url::parse(input).expect("test url should parse");
        parse_ccswitch_link(&url)
    }

    #[test]
    fn parses_full_v1_link() {
        let link = parse(
            "ccswitch://v1/import?resource=provider&app=codex&name=My%20Provider&homepage=https%3A%2F%2Fexample.com&endpoint=https%3A%2F%2Fapi.example.com%2Fv1&apiKey=sk-test&model=gpt-5&haikuModel=h&sonnetModel=s&opusModel=o&notes=hello&icon=https%3A%2F%2Fexample.com%2Ficon.png&enabled=true&configFormat=json",
        )
        .expect("v1 link should parse");
        assert_eq!(link.name, "My Provider");
        assert_eq!(link.app, "codex");
        assert_eq!(link.homepage.as_deref(), Some("https://example.com"));
        assert_eq!(link.endpoint.as_deref(), Some("https://api.example.com/v1"));
        assert_eq!(
            link.api_key.as_ref().map(SensitiveString::expose),
            Some("sk-test")
        );
        assert_eq!(link.model.as_deref(), Some("gpt-5"));
        assert_eq!(link.haiku_model.as_deref(), Some("h"));
        assert_eq!(link.sonnet_model.as_deref(), Some("s"));
        assert_eq!(link.opus_model.as_deref(), Some("o"));
        assert_eq!(link.notes.as_deref(), Some("hello"));
        assert_eq!(link.icon.as_deref(), Some("https://example.com/icon.png"));
    }

    #[test]
    fn preserves_comma_separated_endpoints() {
        let link = parse(
            "ccswitch://v1/import?resource=provider&app=claude&name=Multi&endpoint=https%3A%2F%2Fa.example.com%2Chttps%3A%2F%2Fb.example.com%2Fv2",
        )
        .expect("multi-endpoint link should parse");
        assert_eq!(
            link.endpoint.as_deref(),
            Some("https://a.example.com,https://b.example.com/v2")
        );
    }

    #[test]
    fn preserves_commas_in_endpoint_query_values() {
        let link = parse(
            "ccswitch://v1/import?resource=provider&app=claude&name=Query&endpoint=https%3A%2F%2Fapi.example.com%2Fv1%3Fids%3Da%2Cb",
        )
        .expect("query comma should remain part of the URL");
        assert_eq!(
            link.endpoint.as_deref(),
            Some("https://api.example.com/v1?ids=a,b")
        );
    }

    #[test]
    fn parses_legacy_provider_link() {
        let link = parse(
            "ccswitch://provider?name=Legacy&api_key=sk-old&base_url=https%3A%2F%2Fapi.legacy.com",
        )
        .expect("legacy link should parse");
        assert_eq!(link.name, "Legacy");
        assert_eq!(link.app, "claude");
        assert_eq!(
            link.api_key.as_ref().map(SensitiveString::expose),
            Some("sk-old")
        );
        assert_eq!(link.endpoint.as_deref(), Some("https://api.legacy.com"));
        assert_eq!(link.homepage, None);
    }

    #[test]
    fn rejects_missing_name() {
        let err = parse("ccswitch://v1/import?resource=provider&app=claude&name=")
            .expect_err("missing name should fail");
        assert!(matches!(err, CcSwitchLinkError::Invalid(_)));

        let err = parse("ccswitch://provider?api_key=sk-old")
            .expect_err("legacy link without name should fail");
        assert!(matches!(err, CcSwitchLinkError::Invalid(_)));
    }

    #[test]
    fn rejects_wrong_version_host() {
        let err = parse("ccswitch://v2/import?resource=provider&app=claude&name=X")
            .expect_err("v2 host should fail");
        assert!(matches!(err, CcSwitchLinkError::Invalid(_)));
    }

    #[test]
    fn rejects_wrong_path() {
        let err = parse("ccswitch://v1/export?resource=provider&app=claude&name=X")
            .expect_err("wrong path should fail");
        assert!(matches!(err, CcSwitchLinkError::Invalid(_)));
    }

    #[test]
    fn reports_unsupported_resources() {
        for resource in ["prompt", "mcp", "skill"] {
            let err = parse(&format!(
                "ccswitch://v1/import?resource={resource}&app=claude&name=X"
            ))
            .expect_err("non-provider resource should fail");
            assert_eq!(err, CcSwitchLinkError::Unsupported(resource.to_string()));
            let payload = err.payload();
            assert_eq!(payload.unsupported.as_deref(), Some(resource));
        }
    }

    #[test]
    fn rejects_non_http_endpoint() {
        let err = parse("ccswitch://v1/import?resource=provider&app=claude&name=X&endpoint=ftp%3A%2F%2Fexample.com")
            .expect_err("ftp endpoint should fail");
        assert!(matches!(err, CcSwitchLinkError::Invalid(_)));

        let err = parse("ccswitch://v1/import?resource=provider&app=claude&name=X&endpoint=,%2C")
            .expect_err("empty endpoint list should fail");
        assert!(matches!(err, CcSwitchLinkError::Invalid(_)));

        let err = parse(
            "ccswitch://v1/import?resource=provider&app=claude&name=X&endpoint=https%3A%2F%2Fok.example.com%2Cjavascript%3Aalert(1)",
        )
        .expect_err("mixed endpoint list with non-http item should fail");
        assert!(matches!(err, CcSwitchLinkError::Invalid(_)));
    }

    #[test]
    fn rejects_non_http_homepage() {
        let err = parse("ccswitch://v1/import?resource=provider&app=claude&name=X&homepage=ftp%3A%2F%2Fexample.com")
            .expect_err("ftp homepage should fail");
        assert!(matches!(err, CcSwitchLinkError::Invalid(_)));
    }

    #[test]
    fn rejects_unknown_app() {
        let err = parse("ccswitch://v1/import?resource=provider&app=unknown&name=X")
            .expect_err("unknown app should fail");
        assert!(matches!(err, CcSwitchLinkError::Invalid(_)));
    }

    #[test]
    fn ignores_unknown_extra_params() {
        let link = parse(
            "ccswitch://v1/import?resource=provider&app=claude&name=X&config=abc&configFormat=json&configUrl=https%3A%2F%2Fcfg.example.com&usageFoo=1&enabled=true&somethingElse=y",
        )
        .expect("extra params should be ignored");
        assert_eq!(link.name, "X");
    }

    #[test]
    fn rejects_wrong_scheme() {
        let url = url::Url::parse("aipass://v1/import?resource=provider&app=claude&name=X")
            .expect("test url should parse");
        let err = parse_ccswitch_link(&url).expect_err("wrong scheme should fail");
        assert!(matches!(err, CcSwitchLinkError::Invalid(_)));
    }

    #[test]
    fn serializes_camel_case() {
        let link = parse("ccswitch://provider?name=Legacy&api_key=sk-old")
            .expect("legacy link should parse");
        let json = serde_json::to_value(&link).expect("payload should serialize");
        assert_eq!(json["apiKey"], "sk-old");
        assert_eq!(json["app"], "claude");
        assert!(json.get("api_key").is_none());
    }

    #[test]
    fn parses_aipass_provider_storage_link() {
        let url = url::Url::parse("aipass-provider://v1/add?title=Relay&providerId=custom_http&domain=relay.example.com&endpoint=https%3A%2F%2Frelay.example.com%2Fv1&consoleEndpoint=https%3A%2F%2Frelay.example.com%2Fadmin&interfaceType=openai_compatible&authScheme=bearer&modelAliases=%5B%5B%22fast%22%2C%22gpt-5%22%5D%5D&headers=%5B%5B%22X-Tenant%22%2C%22demo%22%5D%5D&tag=relay&apiKey=sk-test").unwrap();
        let link = parse_aipass_provider_link(&url).expect("provider link should parse");
        assert_eq!(link.title, "Relay");
        assert_eq!(link.provider_id.as_deref(), Some("custom_http"));
        assert_eq!(link.endpoints, vec!["https://relay.example.com/v1"]);
        assert_eq!(
            link.console_endpoints,
            vec!["https://relay.example.com/admin"]
        );
        assert_eq!(link.model_aliases, vec![("fast".into(), "gpt-5".into())]);
        assert_eq!(
            link.headers
                .iter()
                .map(|(name, value)| (name.as_str(), value.expose()))
                .collect::<Vec<_>>(),
            vec![("X-Tenant", "demo")]
        );
    }

    #[test]
    fn rejects_aipass_provider_non_http_endpoint_and_wrong_path() {
        let url =
            url::Url::parse("aipass-provider://v1/add?title=X&endpoint=ftp%3A%2F%2Fexample.com")
                .unwrap();
        assert!(parse_aipass_provider_link(&url).is_err());
        let url = url::Url::parse("aipass-provider://v2/add?title=X").unwrap();
        assert!(parse_aipass_provider_link(&url).is_err());
    }

    #[test]
    fn accepts_bounded_base64_image_favicon() {
        let url = url::Url::parse(
            "aipass-provider://v1/add?title=X&faviconUrl=data%3Aimage%2Fpng%3Bbase64%2CiVBORw0KGgo%3D",
        )
        .unwrap();
        assert!(parse_aipass_provider_link(&url).is_ok());
    }
}
