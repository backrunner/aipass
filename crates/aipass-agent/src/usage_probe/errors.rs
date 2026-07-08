use aipass_agent_protocol::{UsageProbeResult, UsageProbeSource};

pub(super) fn missing_api_key(
    provider_id: Option<String>,
    endpoint: Option<String>,
    source: UsageProbeSource,
) -> UsageProbeResult {
    validation_failure(provider_id, endpoint, source, "provider API key is empty")
}

pub(super) fn validation_failure(
    provider_id: Option<String>,
    endpoint: Option<String>,
    source: UsageProbeSource,
    error: impl Into<String>,
) -> UsageProbeResult {
    UsageProbeResult {
        ok: false,
        provider_id,
        source,
        endpoint,
        status: None,
        quota: None,
        gateway: None,
        plan_name: None,
        message: None,
        error: Some(error.into()),
    }
}

pub(super) fn parse_failure(
    provider_id: Option<String>,
    endpoint: Option<String>,
    source: UsageProbeSource,
    status: u16,
    error: impl Into<String>,
) -> UsageProbeResult {
    UsageProbeResult {
        ok: false,
        provider_id,
        source,
        endpoint,
        status: Some(status),
        quota: None,
        gateway: None,
        plan_name: None,
        message: None,
        error: Some(error.into()),
    }
}
