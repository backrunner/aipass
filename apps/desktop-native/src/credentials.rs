//! Credential operations use the existing authenticated Agent. Request scopes
//! prevent an old read/save from changing a new provider selection or editor.
use aipass_agent::{AgentClient, AgentCommandError};
use aipass_agent_protocol::{AgentRequest, SecretValue, SensitiveString};
use aipass_provider_registry::InterfaceType;
use aipass_vault::SecretMetadataInput;
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Scope {
    pub entry: Uuid,
    pub generation: u64,
}

pub struct Request {
    pub scope: Scope,
    pub operation: Operation,
}

pub enum Operation {
    Reveal(String),
    Copy {
        secret_id: String,
        permit: aipass_desktop_platform::clipboard::CopyPermit,
        seconds: u16,
    },
    Add {
        label: String,
        value: SensitiveString,
        metadata: SecretMetadataInput,
    },
    Update {
        secret_id: String,
        label: String,
        value: SensitiveString,
        metadata: SecretMetadataInput,
    },
    Remove(String),
}

pub enum Outcome {
    Revealed(SensitiveString),
    Applied,
    Copied(bool),
}

pub struct Completion {
    pub scope: Scope,
    pub result: Result<Outcome, AgentCommandError>,
}

impl Request {
    pub(crate) fn execute(self, client: &AgentClient) -> Completion {
        let id = self.scope.entry;
        let result = match self.operation {
            Operation::Copy {
                secret_id,
                permit,
                seconds,
            } => {
                if !permit.is_current() {
                    Ok(Outcome::Copied(false))
                } else {
                    client
                        .request::<SecretValue>(&AgentRequest::SecretRevealId { id, secret_id })
                        .and_then(|value| {
                            permit
                                .write(value.secret, seconds)
                                .map(Outcome::Copied)
                                .map_err(|message| AgentCommandError {
                                    code: None,
                                    message,
                                })
                        })
                }
            }
            Operation::Reveal(secret_id) => client
                .request::<SecretValue>(&AgentRequest::SecretRevealId { id, secret_id })
                .map(|value| Outcome::Revealed(value.secret)),
            Operation::Add {
                label,
                value,
                metadata,
            } => client
                .request::<serde_json::Value>(&AgentRequest::SecretAdd {
                    id,
                    label,
                    secret: value,
                    metadata: Some(metadata),
                })
                .map(|_| Outcome::Applied),
            Operation::Update {
                secret_id,
                label,
                value,
                metadata,
            } => client
                .request::<serde_json::Value>(&AgentRequest::SecretUpdate {
                    id,
                    secret_id,
                    label,
                    secret: Some(value),
                    metadata: Some(metadata),
                })
                .map(|_| Outcome::Applied),
            Operation::Remove(secret_id) => client
                .request::<serde_json::Value>(&AgentRequest::SecretRemove {
                    id,
                    label: secret_id,
                })
                .map(|_| Outcome::Applied),
        };
        Completion {
            scope: self.scope,
            result,
        }
    }
}

pub const INTERFACES: [InterfaceType; 6] = [
    InterfaceType::OpenAiCompatible,
    InterfaceType::AnthropicMessages,
    InterfaceType::AzureOpenAi,
    InterfaceType::Gemini,
    InterfaceType::Bedrock,
    InterfaceType::CustomHttp,
];

/// Return shared locale keys; never include a secret in a validation error.
pub fn validate(label: &str, value: &str, endpoint: &str) -> Result<(), &'static str> {
    if label.trim().is_empty() {
        return Err("providerDetail.secretLabel");
    }
    if value.trim().is_empty() {
        return Err("providerForm.pasteApiKey");
    }
    if !endpoint.trim().is_empty() {
        let valid = url::Url::parse(endpoint.trim()).is_ok_and(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
        });
        if !valid {
            return Err("providers.invalidEndpoint");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_validation_allows_inheritance_but_not_invalid_or_embedded_auth_urls() {
        for endpoint in [
            "",
            "https://gateway.invalid/v1?models=a,b",
            "http://localhost:7000",
        ] {
            assert!(validate("same label", "synthetic", endpoint).is_ok());
        }
        for endpoint in [
            "file:///etc/passwd",
            "https://user:secret@gateway.invalid",
            "relative",
        ] {
            assert!(validate("label", "synthetic", endpoint).is_err());
        }
        assert!(validate("  ", "synthetic", "").is_err());
        assert!(validate("label", " \n", "").is_err());
    }
}
