use crate::*;

pub(crate) fn parse_headers(values: &[String]) -> Result<Vec<(String, String)>> {
    values
        .iter()
        .map(|value| {
            let (name, header_value) = value
                .split_once('=')
                .context("headers must use name=value format")?;
            let name = name.trim();
            if name.is_empty() {
                anyhow::bail!("header name cannot be empty");
            }
            Ok((name.to_string(), header_value.trim().to_string()))
        })
        .collect()
}

pub(crate) fn endpoints_from_cli(
    endpoints: Vec<String>,
    console_urls: Vec<String>,
) -> Result<Vec<ProviderEndpoint>> {
    let mut result = endpoints
        .into_iter()
        .map(|value| validated_endpoint(value, EndpointKind::Api))
        .collect::<Result<Vec<_>>>()?;
    result.extend(
        console_urls
            .into_iter()
            .map(|value| validated_endpoint(value, EndpointKind::Console))
            .collect::<Result<Vec<_>>>()?,
    );
    Ok(result)
}

pub(crate) fn update_endpoints_from_cli(
    existing: &[ProviderEndpoint],
    endpoint_values: Vec<String>,
    console_urls: Vec<String>,
) -> Result<Vec<ProviderEndpoint>> {
    let has_api = !endpoint_values.is_empty();
    let has_console = !console_urls.is_empty();
    let mut result = existing
        .iter()
        .filter(|item| {
            (!has_api || item.kind != EndpointKind::Api)
                && (!has_console || item.kind != EndpointKind::Console)
        })
        .cloned()
        .collect::<Vec<_>>();
    result.extend(endpoints_from_cli(endpoint_values, console_urls)?);
    Ok(result)
}

pub(crate) fn validated_endpoint(url: String, kind: EndpointKind) -> Result<ProviderEndpoint> {
    let url = url.trim().to_string();
    if url.is_empty() {
        anyhow::bail!("endpoint URL cannot be empty");
    }
    let parsed =
        reqwest::Url::parse(&url).with_context(|| format!("invalid endpoint URL: {url}"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        anyhow::bail!("endpoint must be an absolute HTTP or HTTPS URL: {url}");
    }
    Ok(match kind {
        EndpointKind::Api => ProviderEndpoint::api(url),
        EndpointKind::Console => ProviderEndpoint::console(url),
        _ => unreachable!("CLI endpoint helper only creates API or console endpoints"),
    })
}

pub(crate) fn secret_metadata_from_cli(
    group: Option<String>,
    interface_type: Option<InterfaceType>,
    billing_rate: Option<String>,
    billing_currency: Option<String>,
    billing_unit_price: Option<String>,
) -> SecretMetadataInput {
    let billing = [billing_rate, billing_currency, billing_unit_price]
        .into_iter()
        .collect::<Vec<_>>();
    let billing = if billing.iter().all(Option::is_none) {
        None
    } else {
        Some(aipass_provider_registry::BillingRule {
            rate: billing[0].clone(),
            currency: billing[1].clone(),
            unit_price: billing[2].clone(),
            note: None,
        })
    };
    SecretMetadataInput {
        group,
        interface_type,
        billing,
    }
}

pub(crate) fn parse_model_aliases(values: &[String]) -> Result<Vec<(String, String)>> {
    values
        .iter()
        .map(|value| {
            let (alias, model) = value
                .split_once('=')
                .context("model aliases must use alias=model format")?;
            let alias = alias.trim();
            let model = model.trim();
            if alias.is_empty() || model.is_empty() {
                anyhow::bail!("model alias and model cannot be empty");
            }
            Ok((alias.to_string(), model.to_string()))
        })
        .collect()
}

pub(crate) fn quota_from_parts(
    label: Option<String>,
    limit: Option<String>,
    used: Option<String>,
    remaining: Option<String>,
    reset_at: Option<String>,
) -> Option<QuotaInfo> {
    if label.is_none()
        && limit.is_none()
        && used.is_none()
        && remaining.is_none()
        && reset_at.is_none()
    {
        return None;
    }
    Some(QuotaInfo {
        unit: None,
        label,
        limit,
        used,
        remaining,
        reset_at,
    })
}
