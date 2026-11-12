//! Native provider presentation state and typed Agent requests. No storage.
use aipass_agent_protocol::AgentRequest;
use aipass_provider_registry::{CredentialKind, ProviderKind};
use aipass_vault::{EntrySummary, ProviderEntryInput, ProviderEntryUpdateInput};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod draft;
mod lists;

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Catalog {
    pub active: Vec<EntrySummary>,
    pub archived: Vec<EntrySummary>,
    pub trash: Vec<EntrySummary>,
}

pub enum Action {
    Add(Box<ProviderEntryInput>),
    Update(Uuid, Box<ProviderEntryUpdateInput>),
    Favorite(Uuid, bool),
    Archive(Uuid),
    Restore(Uuid),
    Trash(Uuid),
}

impl Action {
    pub fn saves_form(&self) -> bool {
        matches!(self, Self::Add(_) | Self::Update(..))
    }
    pub fn selected(&self) -> Option<Uuid> {
        match self {
            Self::Update(id, _) | Self::Favorite(id, _) | Self::Restore(id) => Some(*id),
            _ => None,
        }
    }
    pub fn into_request(self) -> AgentRequest {
        match self {
            Self::Add(input) => AgentRequest::ProviderAdd { input: *input },
            Self::Update(id, input) => AgentRequest::ProviderUpdate { id, input: *input },
            Self::Favorite(id, favorite) => AgentRequest::ProviderFavorite { id, favorite },
            Self::Archive(id) => AgentRequest::ProviderArchive { id },
            Self::Restore(id) => AgentRequest::ProviderRestore { id },
            Self::Trash(id) => AgentRequest::ProviderTrash { id },
        }
    }
}

impl Catalog {
    #[cfg(any(test, feature = "parity-render"))]
    pub fn fixture() -> Self {
        serde_json::from_str(include_str!(
            "../../../tests/desktop-parity/fixtures/providers.json"
        ))
        .expect("synthetic provider fixture")
    }

    pub fn entries(&self, section: usize, query: &str) -> Vec<&EntrySummary> {
        if matches!(section, 7 | 8) {
            return vec![];
        }
        let source = match section {
            9 => &self.archived,
            10 => &self.trash,
            _ => &self.active,
        };
        let terms: Vec<_> = query.split_whitespace().map(str::to_lowercase).collect();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |time| time.as_secs() as i64);
        source
            .iter()
            .filter(|entry| {
                let matches = match section {
                    1 => entry.favorite,
                    2 => entry.last_used_at.is_some(),
                    3 => entry.provider_kind == ProviderKind::Official,
                    4 => entry.provider_kind == ProviderKind::ThirdParty,
                    5 => entry.provider_kind == ProviderKind::SelfHosted,
                    6 => entry.provider_kind == ProviderKind::Unknown,
                    11 => entry.quota.as_ref().is_some_and(|quota| {
                        numeric(quota.remaining.as_deref()).is_some_and(|remaining| {
                            numeric(quota.limit.as_deref())
                                .filter(|limit| *limit > 0.)
                                .map_or(remaining <= 0., |limit| remaining / limit <= 0.2)
                        })
                    }),
                    12 => [
                        entry
                            .quota
                            .as_ref()
                            .and_then(|quota| quota.reset_at.as_deref()),
                        entry
                            .subscription
                            .as_ref()
                            .and_then(|s| s.subscription_expires_at.as_deref()),
                        entry
                            .subscription
                            .as_ref()
                            .and_then(|s| s.credential_expires_at.as_deref()),
                    ]
                    .into_iter()
                    .flatten()
                    .filter_map(|date| {
                        time::OffsetDateTime::parse(
                            date,
                            &time::format_description::well_known::Rfc3339,
                        )
                        .ok()
                    })
                    .any(|date| date.unix_timestamp() <= now + 30 * 86400),
                    13 => entry.credential_kind == CredentialKind::OAuth,
                    14 => entry.credential_kind == CredentialKind::Api,
                    _ => true,
                };
                let searchable = format!(
                    "{} {} {} {} {}",
                    entry.title,
                    entry.domains.join(" "),
                    entry.tags.join(" "),
                    entry.account_identity.as_deref().unwrap_or_default(),
                    entry
                        .endpoints
                        .iter()
                        .filter_map(|e| e.url.as_deref())
                        .collect::<Vec<_>>()
                        .join(" ")
                )
                .to_lowercase();
                matches && terms.iter().all(|term| searchable.contains(term))
            })
            .collect()
    }

    pub fn find(&self, id: Uuid) -> Option<&EntrySummary> {
        self.active
            .iter()
            .chain(&self.archived)
            .chain(&self.trash)
            .find(|entry| entry.id == id)
    }
}

fn numeric(value: Option<&str>) -> Option<f64> {
    let value = value?.replace(',', "");
    let start = value.find(|ch: char| ch.is_ascii_digit())?;
    let bytes = &value.as_bytes()[start..];
    let mut end = bytes
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if bytes.get(end) == Some(&b'.') && bytes.get(end + 1).is_some_and(u8::is_ascii_digit) {
        end += 1;
        end += bytes[end..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
    }
    let number = &value[start..start + end];
    number.parse::<f64>().ok().filter(|value| value.is_finite())
}

#[cfg(test)]
mod tests;
