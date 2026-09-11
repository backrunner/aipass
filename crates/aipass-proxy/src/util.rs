use super::*;

pub(crate) fn tokens_match(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.as_bytes()
        .iter()
        .zip(right.as_bytes())
        .fold(0_u8, |diff, (left, right)| diff | (left ^ right))
        == 0
}

pub(crate) fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

pub(crate) fn local_day_start(timestamp: i64, timezone_offset_seconds: i64) -> i64 {
    (timestamp + timezone_offset_seconds).div_euclid(86_400) * 86_400 - timezone_offset_seconds
}
