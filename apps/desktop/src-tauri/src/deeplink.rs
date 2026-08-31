use serde::Serialize;

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

/// Payload of the `ccswitch-provider-import` event, serialized camelCase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CcSwitchProviderLink {
    pub name: String,
    pub app: String,
    pub homepage: Option<String>,
    pub endpoint: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub notes: Option<String>,
    pub haiku_model: Option<String>,
    pub sonnet_model: Option<String>,
    pub opus_model: Option<String>,
    pub icon: Option<String>,
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

fn require_http_url(value: &str, field: &str) -> Result<(), CcSwitchLinkError> {
    let parsed = url::Url::parse(value)
        .map_err(|_| invalid(format!("ccswitch link has an invalid {field} URL")))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(invalid(format!("ccswitch link {field} must use http(s)")));
    }
    Ok(())
}

/// `endpoint` may be a comma-separated list; every item must be http(s).
fn validate_endpoint(endpoint: &Option<String>) -> Result<(), CcSwitchLinkError> {
    if let Some(endpoint) = endpoint {
        for item in endpoint.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            require_http_url(item, "endpoint")?;
        }
    }
    Ok(())
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
        api_key: query_param(params, "apiKey"),
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
        api_key: query_param(params, "api_key"),
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
        assert_eq!(link.api_key.as_deref(), Some("sk-test"));
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
    fn parses_legacy_provider_link() {
        let link = parse(
            "ccswitch://provider?name=Legacy&api_key=sk-old&base_url=https%3A%2F%2Fapi.legacy.com",
        )
        .expect("legacy link should parse");
        assert_eq!(link.name, "Legacy");
        assert_eq!(link.app, "claude");
        assert_eq!(link.api_key.as_deref(), Some("sk-old"));
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
}
