//! Typed provider preferences. The Agent remains the sole settings authority.
use super::*;
use aipass_agent_protocol::ProviderRuntimeOptions;

pub enum Action {
    Load,
    Save(Box<ProviderRuntimeOptions>),
    Probe,
    Test(Uuid),
    Reset,
}
pub enum ResultData {
    Loaded(
        Box<ProviderRuntimeOptions>,
        Vec<(Uuid, String)>,
        Option<String>,
    ),
    Saved(Box<ProviderRuntimeOptions>),
    Balance(String),
    Reset(String),
    Sent,
}
pub fn execute(
    id: Uuid,
    action: Action,
    client: &AgentClient,
) -> Result<ResultData, AgentCommandError> {
    Ok(match action {
        Action::Load => {
            let options = client.request(&AgentRequest::ProviderRuntimeGet { id })?;
            let routes: Result<aipass_proxy::ProxyConfig, _> =
                client.request(&AgentRequest::ServerConfigGet);
            let (routes, error) = match routes {
                Ok(config) => (
                    config
                        .routes
                        .into_iter()
                        .filter(|r| {
                            r.enabled
                                && r.targets
                                    .iter()
                                    .any(|t| t.enabled && t.provider_entry_id == id)
                        })
                        .map(|r| (r.id, r.name))
                        .collect(),
                    None,
                ),
                Err(error) => (vec![], Some(error.message)),
            };
            ResultData::Loaded(Box::new(options), routes, error)
        }
        Action::Save(options) => ResultData::Saved(Box::new(client.request(
            &AgentRequest::ProviderRuntimeSet {
                id,
                options: *options,
            },
        )?)),
        Action::Probe => {
            #[derive(Deserialize)]
            struct Balance {
                balance: f64,
                unit: String,
            }
            let value: Balance = client.request(&AgentRequest::ProviderRuntimeProbe { id })?;
            ResultData::Balance(format!("{} {}", value.balance, value.unit))
        }
        Action::Test(webhook_id) => {
            let _: serde_json::Value =
                client.request(&AgentRequest::ProviderWebhookTest { id, webhook_id })?;
            ResultData::Sent
        }
        Action::Reset => {
            #[derive(Deserialize)]
            struct Reset {
                outcome: String,
            }
            let value: Reset = client.request(&AgentRequest::SubscriptionReset {
                entry_id: id,
                request_id: None,
                credit_id: None,
            })?;
            ResultData::Reset(value.outcome)
        }
    })
}
