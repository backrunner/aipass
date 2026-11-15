//! Permanent deletion stays with the Agent; the UI only holds a confirmed target.
use aipass_agent::{AgentClient, AgentCommandError};
use aipass_agent_protocol::AgentRequest;
use uuid::Uuid;

pub struct Request {
    pub generation: u64,
    pub target: Option<Uuid>,
}

pub struct Completion {
    pub generation: u64,
    pub result: Result<(), AgentCommandError>,
}

impl Request {
    pub fn execute(self, client: &AgentClient) -> Completion {
        let request = match self.target {
            Some(id) => AgentRequest::ProviderDelete { id },
            None => AgentRequest::TrashEmpty,
        };
        Completion {
            generation: self.generation,
            result: client.request::<serde_json::Value>(&request).map(|_| ()),
        }
    }
}

/// Same thirty-day retention display as ProviderDetailPane; cleanup remains Agent-owned.
pub fn days_remaining(deleted: time::OffsetDateTime, now: time::OffsetDateTime) -> i64 {
    let seconds = (deleted + time::Duration::days(30) - now).as_seconds_f64();
    (seconds / 86400.).ceil().max(0.) as i64
}

#[cfg(test)]
mod tests {
    #[test]
    fn retention_rounds_up_partial_days_and_stops_at_zero() {
        let deleted = time::OffsetDateTime::UNIX_EPOCH;
        assert_eq!(super::days_remaining(deleted, deleted), 30);
        assert_eq!(
            super::days_remaining(deleted, deleted + time::Duration::hours(1)),
            30
        );
        assert_eq!(
            super::days_remaining(deleted, deleted + time::Duration::days(29)),
            1
        );
        assert_eq!(
            super::days_remaining(deleted, deleted + time::Duration::days(30)),
            0
        );
        assert_eq!(
            super::days_remaining(deleted, deleted + time::Duration::days(31)),
            0
        );
    }
}
