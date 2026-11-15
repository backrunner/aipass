//! New native subscription requests against the real authenticated, disposable Agent.
use super::*;
use aipass_agent_protocol::{AgentErrorCode, ProviderProxyOptions, SensitiveString};
use aipass_desktop_native::subscriptions::{
    options::{Action as OptionsAction, ResultData},
    Completion, Kind, Operation, Outcome, Request,
};
use aipass_proxy::UpstreamProxyMode;
use uuid::Uuid;
fn request(transport: &mut AgentTransport, operation: Operation) -> Completion {
    let kind = operation.kind();
    let Reply::Subscription(reply) = transport
        .execute(Command::Subscription(Box::new(Request {
            generation: 42,
            operation,
        })))
        .unwrap()
    else {
        panic!("subscription reply required")
    };
    assert_eq!(reply.generation, 42);
    assert_eq!(reply.kind, kind);
    *reply
}
fn load(transport: &mut AgentTransport, id: Uuid) -> aipass_agent_protocol::ProviderRuntimeOptions {
    let Outcome::Options(ResultData::Loaded(value, _, error)) =
        request(transport, Operation::Options(id, OptionsAction::Load))
            .result
            .unwrap()
    else {
        panic!()
    };
    assert!(error.is_none());
    *value
}
pub(super) fn workflow(transport: &mut AgentTransport, id: Uuid) {
    let Outcome::Catalog(catalog) = request(transport, Operation::Catalog).result.unwrap() else {
        panic!()
    };
    assert!(catalog.len() >= 41);
    let mut value = load(transport, id);
    value.quota_tracking = false;
    value.quota_refresh_seconds = 300;
    value.subscription.quota_use_limit_percent = Some(80);
    value.proxy = Some(ProviderProxyOptions {
        mode: UpstreamProxyMode::Custom,
        url: Some("http://127.0.0.1:17890".into()),
        username: Some(SensitiveString::new("fixture-user")),
        password: Some(SensitiveString::new("fixture-password")),
        has_credentials: false,
        clear_credentials: false,
    });
    let Outcome::Options(ResultData::Saved(saved)) = request(
        transport,
        Operation::Options(id, OptionsAction::Save(Box::new(value))),
    )
    .result
    .unwrap() else {
        panic!()
    };
    assert!(!saved.quota_tracking);
    assert_eq!(saved.quota_refresh_seconds, 300);
    let proxy = saved.proxy.as_ref().unwrap();
    assert!(proxy.has_credentials);
    assert!(proxy.username.is_none() && proxy.password.is_none());
    // An unrelated quota change must not erase the existing proxy credentials.
    let mut value = *saved;
    value.quota_refresh_seconds = 900;
    request(
        transport,
        Operation::Options(id, OptionsAction::Save(Box::new(value))),
    )
    .result
    .unwrap();
    let mut value = load(transport, id);
    assert_eq!(value.subscription.quota_use_limit_percent, Some(80));
    assert!(value.proxy.as_ref().unwrap().has_credentials);
    value.proxy.as_mut().unwrap().clear_credentials = true;
    request(
        transport,
        Operation::Options(id, OptionsAction::Save(Box::new(value))),
    )
    .result
    .unwrap();
    assert!(!load(transport, id).proxy.unwrap().has_credentials);
}
pub(super) fn locked(transport: &mut AgentTransport, id: Uuid) {
    let completion = request(transport, Operation::Options(id, OptionsAction::Load));
    assert_eq!(completion.kind, Kind::Options(id));
    assert!(completion
        .result
        .is_err_and(|e| e.code == Some(AgentErrorCode::Locked)));
}
