use super::*;

#[derive(Default)]
struct FakeClipboard {
    text: String,
    fail_read: bool,
    fail_write: bool,
    fail_clear: bool,
    clears: usize,
}
impl Clipboard for FakeClipboard {
    fn read(&mut self) -> Result<SensitiveString, String> {
        if self.fail_read {
            Err("unavailable".into())
        } else {
            Ok(self.text.clone().into())
        }
    }
    fn write(&mut self, value: &str) -> Result<(), String> {
        if self.fail_write {
            return Err("unavailable".into());
        }
        self.text = value.into();
        Ok(())
    }
    fn clear(&mut self) -> Result<(), String> {
        if self.fail_clear {
            return Err("unavailable".into());
        }
        self.clears += 1;
        self.text.clear();
        Ok(())
    }
}
fn copied() -> ClipboardState<FakeClipboard> {
    let mut state = ClipboardState::default();
    assert!(state.copy(0, "synthetic-key".into(), 45).unwrap());
    state
}
#[test]
fn clears_only_the_secret_it_copied() {
    let mut state = copied();
    state.clear_matching().unwrap();
    assert_eq!(state.clipboard.clears, 1);
    assert!(state.clipboard.text.is_empty());
    assert!(state.pending.is_none());
}
#[test]
fn preserves_later_user_content() {
    let mut state = copied();
    state.clipboard.text = "user-content".into();
    state.clear_matching().unwrap();
    assert_eq!(state.clipboard.text, "user-content");
    assert_eq!(state.clipboard.clears, 0);
    assert!(state.pending.is_none());
}
#[test]
fn read_failure_never_falls_back_to_a_blind_clear() {
    let mut state = copied();
    state.clipboard.text = "user-content".into();
    state.clipboard.fail_read = true;
    assert!(state.clear_matching().is_err());
    assert_eq!(state.clipboard.clears, 0);
    assert!(state.pending.is_some());
    state.clipboard.fail_read = false;
    state.clear_matching().unwrap();
    assert_eq!(state.clipboard.text, "user-content");
}
#[test]
fn retries_a_failed_clear_without_forgetting_ownership() {
    let mut state = copied();
    state.clipboard.fail_clear = true;
    assert!(state.invalidate().is_err());
    assert!(state.pending.is_some());
    state.clipboard.fail_clear = false;
    state.clear_matching().unwrap();
    assert!(state.pending.is_none());
}
#[test]
fn latest_copy_replaces_the_deadline_and_value() {
    let mut state = copied();
    let original_deadline = state.pending.as_ref().unwrap().deadline;
    assert!(state.copy(0, "new-key".into(), 60).unwrap());
    assert!(state.pending.as_ref().unwrap().deadline > original_deadline);
    state.clear_matching().unwrap();
    assert_eq!(state.clipboard.clears, 1);
}
#[test]
fn failed_copy_retains_the_previous_cleanup() {
    let mut state = copied();
    state.clipboard.fail_write = true;
    assert!(state.copy(0, "new-key".into(), 60).is_err());
    assert_eq!(
        state.pending.as_ref().unwrap().digest,
        <[u8; 32]>::from(Sha256::digest(b"synthetic-key"))
    );
    state.clear_matching().unwrap();
    assert_eq!(state.clipboard.clears, 1);
}
#[test]
fn lock_cancels_copies_waiting_for_confirmation_or_agent() {
    let mut state = copied();
    state.invalidate().unwrap();
    assert!(!state.copy(0, "late-key".into(), 45).unwrap());
    assert!(state.clipboard.text.is_empty());
    assert!(state.pending.is_none());
    assert!(state.copy(state.epoch, "fresh-key".into(), 45).unwrap());
}

#[test]
fn prolonged_clipboard_failure_does_not_abandon_cleanup() {
    let mut state = copied();
    state.clipboard.fail_read = true;
    for _ in 0..300 {
        state.clear_or_retry();
        let pending = state.pending.as_ref().expect("keep cleanup ownership");
        assert!(
            pending.deadline.saturating_duration_since(Instant::now()) <= Duration::from_secs(30)
        );
    }
    state.clipboard.fail_read = false;
    state.clear_or_retry();
    assert!(state.clipboard.text.is_empty());
    assert!(state.pending.is_none());
}

#[test]
fn user_content_is_preserved_when_clipboard_recovers_after_many_failures() {
    let mut state = copied();
    state.clipboard.fail_clear = true;
    for _ in 0..5 {
        state.clear_or_retry();
    }
    state.clipboard.text = "new user content".into();
    state.clipboard.fail_clear = false;
    state.clear_or_retry();
    assert_eq!(state.clipboard.text, "new user content");
    assert_eq!(state.clipboard.clears, 0);
    assert!(state.pending.is_none());
}

#[test]
fn cancelled_and_invalidated_permits_never_write_and_release_exclusivity() {
    let permit = CopyPermit::begin().unwrap().unwrap();
    assert!(CopyPermit::begin().unwrap().is_none());
    assert!(permit.is_current());
    permit.cancellation().cancel();
    assert!(!permit.is_current());
    assert!(!permit.write("synthetic-no-os-write".into(), 45).unwrap());
    let permit = CopyPermit::begin().unwrap().unwrap();
    invalidate();
    assert!(!permit.is_current());
    assert!(!permit.write("synthetic-no-os-write".into(), 45).unwrap());
    assert!(CopyPermit::begin().unwrap().is_some());
}
