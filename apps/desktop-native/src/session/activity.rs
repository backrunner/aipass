//! Coalesce real input; periodic status/usage polls never keep a vault unlocked.
use std::time::{Duration, Instant};
const INTERVAL: Duration = Duration::from_secs(30);
#[derive(Default)]
pub(super) struct Activity {
    last_sent: Option<Instant>,
    pending: Option<Instant>,
}
impl Activity {
    pub(super) fn record(&mut self, now: Instant) {
        if self
            .last_sent
            .is_none_or(|last| now.saturating_duration_since(last) >= INTERVAL)
        {
            self.pending = Some(now);
        }
    }
    pub(super) fn take(&mut self, now: Instant) -> bool {
        let Some(event) = self.pending.take() else {
            return false;
        };
        if now.saturating_duration_since(event) >= INTERVAL {
            return false;
        }
        self.last_sent = Some(now);
        true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn polls_do_not_touch_and_input_is_throttled() {
        let now = Instant::now();
        let mut activity = Activity::default();
        assert!(!activity.take(now));
        activity.record(now);
        assert!(activity.take(now));
        activity.record(now + Duration::from_secs(1));
        assert!(!activity.take(now + Duration::from_secs(2)));
        assert!(!activity.take(now + INTERVAL));
        activity.record(now + INTERVAL);
        assert!(activity.take(now + INTERVAL));
    }
    #[test]
    fn a_long_running_operation_cannot_replay_stale_activity() {
        let now = Instant::now();
        let mut activity = Activity::default();
        activity.record(now);
        assert!(!activity.take(now + INTERVAL));
        activity.record(now + INTERVAL);
        assert!(activity.take(now + INTERVAL));
    }
}
