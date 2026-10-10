use super::*;

pub(super) async fn wait_for_successful_attempts(store: &UsageStore, expected: u64) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let summary = store.summary(|_| 0).unwrap();
        if summary.successful_attempts >= expected {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "usage persistence stalled: expected {expected}, got {}",
            summary.successful_attempts
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
