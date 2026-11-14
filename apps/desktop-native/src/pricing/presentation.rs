//! The supported English and Chinese locales share USD grouping and precision.
use aipass_agent_protocol::UiLocale;

pub fn display_time(value: &str, locale: UiLocale) -> String {
    let Some(minute) = super::draft::minute(value) else {
        return value.into();
    };
    if locale == UiLocale::ZhCn {
        return value.into();
    }
    let hour = minute / 60;
    format!(
        "{:02}:{:02} {}",
        if hour % 12 == 0 { 12 } else { hour % 12 },
        minute % 60,
        if hour < 12 { "AM" } else { "PM" }
    )
}
pub fn canonical_time(value: &str) -> Option<String> {
    let value = value.trim();
    if super::draft::minute(value).is_some() {
        return Some(value.into());
    }
    let (clock, period) = value.rsplit_once(' ')?;
    let minutes = super::draft::minute(clock)?;
    let hour = minutes / 60;
    if !(1..=12).contains(&hour) {
        return None;
    }
    let hour = match period.to_ascii_uppercase().as_str() {
        "AM" => hour % 12,
        "PM" => hour % 12 + 12,
        _ => return None,
    };
    Some(format!("{hour:02}:{:02}", minutes % 60))
}

pub fn estimate(value: Option<f64>) -> String {
    let Some(value) = value.filter(|n| n.is_finite() && *n >= 0.) else {
        return "—".into();
    };
    if value >= 1e6 {
        return format!("${value:.2e}").replace('e', "E");
    }
    let value = format!("{value:.6}");
    let (whole, fraction) = value.split_once('.').unwrap();
    let grouped = whole
        .chars()
        .enumerate()
        .fold(String::new(), |mut s, (i, c)| {
            if i > 0 && (whole.len() - i).is_multiple_of(3) {
                s.push(',');
            }
            s.push(c);
            s
        });
    let fraction = fraction.trim_end_matches('0');
    format!(
        "${grouped}.{fraction}{}",
        "0".repeat(2_usize.saturating_sub(fraction.len()))
    )
}

pub fn local_date(timestamp: i64, locale: UiLocale) -> String {
    #[cfg(target_pointer_width = "64")]
    let epoch: libc::time_t = timestamp;
    #[cfg(target_pointer_width = "32")]
    let Ok(epoch) = libc::time_t::try_from(timestamp) else {
        return timestamp.to_string();
    };
    let mut local = std::mem::MaybeUninit::<libc::tm>::uninit();
    #[cfg(unix)]
    // SAFETY: both pointers reference live, correctly aligned storage. The
    // reentrant API initializes `local` exactly when it returns non-null.
    let valid = unsafe { !libc::localtime_r(&epoch, local.as_mut_ptr()).is_null() };
    #[cfg(windows)]
    // SAFETY: MSVC localtime_s writes the supplied tm on success.
    let valid = unsafe { libc::localtime_s(local.as_mut_ptr(), &epoch) == 0 };
    if !valid {
        return timestamp.to_string();
    }
    // SAFETY: checked successful initialization above.
    let local = unsafe { local.assume_init() };
    let (year, month, day) = (local.tm_year + 1900, local.tm_mon + 1, local.tm_mday);
    let (hour, minute, second) = (local.tm_hour, local.tm_min, local.tm_sec);
    match locale {
        UiLocale::ZhCn => format!("{year}/{month}/{day} {hour:02}:{minute:02}:{second:02}"),
        UiLocale::En => format!(
            "{month}/{day}/{year}, {}:{minute:02}:{second:02} {}",
            if hour % 12 == 0 { 12 } else { hour % 12 },
            if hour < 12 { "AM" } else { "PM" }
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clock_fields_round_trip_both_locales_and_reject_invalid_periods() {
        for minute in ["00:00", "06:00", "12:00", "22:15", "23:59"] {
            for locale in [UiLocale::En, UiLocale::ZhCn] {
                assert_eq!(
                    canonical_time(&display_time(minute, locale)).as_deref(),
                    Some(minute)
                );
            }
        }
        for invalid in ["00:00 PM", "13:00 AM", "10:60 AM", "10:00 afternoon"] {
            assert!(canonical_time(invalid).is_none());
        }
    }
    #[test]
    fn preview_precision_grouping_and_invalid_values() {
        for (value, expected) in [
            (0., "$0.00"),
            (0.5, "$0.50"),
            (1.234567, "$1.234567"),
            (1000., "$1,000.00"),
            (1e6, "$1.00E6"),
        ] {
            assert_eq!(estimate(Some(value)), expected);
        }
        for value in [None, Some(-1.), Some(f64::NAN), Some(f64::INFINITY)] {
            assert_eq!(estimate(value), "—");
        }
    }
}
