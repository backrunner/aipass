//! Lossless editable price metadata, independent of the native renderer.
use aipass_agent_protocol::{GroupPriceVersion, ModelPriceRule, OffPeakWindow, PricingGroup};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct Rule {
    pub model: String,
    /// Input, output, cache read, cache write and optional one-hour cache write.
    pub prices: [String; 5],
    pub off_peak: bool,
    pub start: String,
    pub end: String,
    pub off_peak_prices: [String; 5],
}
impl Default for Rule {
    fn default() -> Self {
        Self {
            model: String::new(),
            prices: std::array::from_fn(|_| String::new()),
            off_peak: false,
            start: "22:00".into(),
            end: "06:00".into(),
            off_peak_prices: std::array::from_fn(|_| String::new()),
        }
    }
}
fn prices(required: [u64; 4], optional: Option<u64>) -> [String; 5] {
    std::array::from_fn(|i| {
        let value = if i < 4 { Some(required[i]) } else { optional };
        value.map(format_micros).unwrap_or_default()
    })
}
/// Avoid float formatting losing a stored micro-dollar when reopening a draft.
fn format_micros(n: u64) -> String {
    let whole = n / 1_000_000;
    let fraction = n % 1_000_000;
    if fraction == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{fraction:06}")
            .trim_end_matches('0')
            .to_owned()
    }
}
impl From<&ModelPriceRule> for Rule {
    fn from(r: &ModelPriceRule) -> Self {
        let mut result = Self {
            model: r.model.clone(),
            prices: prices(
                [
                    r.input_micros_per_million,
                    r.output_micros_per_million,
                    r.cache_read_micros_per_million,
                    r.cache_creation_micros_per_million,
                ],
                r.cache_creation_1h_micros_per_million,
            ),
            ..Default::default()
        };
        if let Some(p) = &r.off_peak {
            result.off_peak = true;
            result.start = format!(
                "{:02}:{:02}",
                p.start_minute_utc / 60,
                p.start_minute_utc % 60
            );
            result.end = format!("{:02}:{:02}", p.end_minute_utc / 60, p.end_minute_utc % 60);
            result.off_peak_prices = prices(
                [
                    p.input_micros_per_million,
                    p.output_micros_per_million,
                    p.cache_read_micros_per_million,
                    p.cache_creation_micros_per_million,
                ],
                p.cache_creation_1h_micros_per_million,
            );
        }
        result
    }
}
pub fn multiplier(value: &str) -> Option<f64> {
    let value: f64 = value.trim().parse().ok()?;
    (value.is_finite() && value >= 0.).then_some(value)
}
fn micros(value: &str) -> Option<u64> {
    // Decimal rounding avoids losing a micro-dollar when reopening a large
    // stored integer through an f64 USD conversion.
    let value = value.trim().strip_prefix('+').unwrap_or(value.trim());
    let (coefficient, exponent) = match value.find(['e', 'E']) {
        Some(i) => (&value[..i], value[i + 1..].parse::<i32>().ok()?),
        None => (value, 0),
    };
    let negative = coefficient.starts_with('-');
    let coefficient = coefficient.strip_prefix('-').unwrap_or(coefficient);
    let (whole, fraction) = coefficient.split_once('.').unwrap_or((coefficient, ""));
    if whole.is_empty() && fraction.is_empty()
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let digits = format!("{whole}{fraction}");
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return Some(0);
    }
    if negative {
        return None;
    }
    let shift = i64::from(exponent) + 6 - i64::try_from(fraction.len()).ok()?;
    let n = if shift >= 0 {
        if digits.len() as i64 + shift > 16 {
            return None;
        }
        digits
            .parse::<u64>()
            .ok()?
            .checked_mul(10_u64.checked_pow(shift as u32)?)?
    } else {
        let keep = digits.len() as i64 + shift;
        if keep > 16 {
            return None;
        }
        if keep < 0 {
            0
        } else {
            let keep = keep as usize;
            let whole = if keep == 0 {
                0
            } else {
                digits[..keep].parse::<u64>().ok()?
            };
            whole + u64::from(digits.as_bytes()[keep] >= b'5')
        }
    };
    (n <= 9_007_199_254_740_991).then_some(n)
}
fn parse_prices(values: &[String; 5]) -> Option<[Option<u64>; 5]> {
    let mut result = [None; 5];
    for (i, value) in values.iter().enumerate() {
        if i == 4 && value.trim().is_empty() {
            continue;
        }
        result[i] = Some(micros(value)?);
    }
    Some(result)
}
pub fn minute(value: &str) -> Option<u16> {
    let (h, m) = value.split_once(':')?;
    if h.len() != 2 || m.len() != 2 || !h.bytes().chain(m.bytes()).all(|c| c.is_ascii_digit()) {
        return None;
    }
    let h: u16 = h.parse().ok()?;
    let m: u16 = m.parse().ok()?;
    (h < 24 && m < 60).then_some(h * 60 + m)
}
impl Rule {
    pub fn build(&self) -> Option<ModelPriceRule> {
        let model = self.model.trim();
        if model.is_empty() {
            return None;
        }
        let p = parse_prices(&self.prices)?;
        let off_peak = if self.off_peak {
            let p = parse_prices(&self.off_peak_prices)?;
            Some(OffPeakWindow {
                start_minute_utc: minute(&self.start)?,
                end_minute_utc: minute(&self.end)?,
                input_micros_per_million: p[0]?,
                output_micros_per_million: p[1]?,
                cache_read_micros_per_million: p[2]?,
                cache_creation_micros_per_million: p[3]?,
                cache_creation_1h_micros_per_million: p[4],
            })
        } else {
            None
        };
        Some(ModelPriceRule {
            model: model.to_owned(),
            input_micros_per_million: p[0]?,
            output_micros_per_million: p[1]?,
            cache_read_micros_per_million: p[2]?,
            cache_creation_micros_per_million: p[3]?,
            cache_creation_1h_micros_per_million: p[4],
            off_peak,
        })
    }
}
pub fn group(id: Uuid, name: &str, rules: &[Rule]) -> Option<PricingGroup> {
    if name.trim().is_empty() || rules.is_empty() {
        return None;
    }
    let rules = rules.iter().map(Rule::build).collect::<Option<Vec<_>>>()?;
    let mut names = std::collections::HashSet::new();
    if rules.iter().any(|r| !names.insert(&r.model)) {
        return None;
    }
    Some(PricingGroup {
        id,
        name: name.trim().to_owned(),
        manual: true,
        versions: vec![GroupPriceVersion {
            effective_from: 0,
            rules,
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_and_newly_enabled_off_peak_prices_require_explicit_values() {
        let mut rule = Rule {
            model: "model-".into(),
            ..Default::default()
        };
        assert!(rule.build().is_none());
        rule.prices = prices([0; 4], None);
        assert!(rule.build().is_some());
        rule.off_peak = true;
        assert!(rule.build().is_none());
        rule.off_peak_prices = prices([0; 4], None);
        assert_eq!(
            rule.build()
                .unwrap()
                .off_peak
                .unwrap()
                .input_micros_per_million,
            0
        );
    }
    #[test]
    fn invalid_prices_and_times_do_not_turn_into_free_usage() {
        for invalid in ["", "NaN", "inf", "-1", "1e308", "9007199255"] {
            assert!(micros(invalid).is_none());
        }
        for invalid in ["24:00", "00:60", "-1:00", "22:3", "00:00:00"] {
            assert!(minute(invalid).is_none());
        }
        assert_eq!(minute("23:59"), Some(1439));
        assert_eq!(micros("0.000001"), Some(1));
        assert_eq!(micros("5e-7"), Some(1));
        assert_eq!(micros("4e-7"), Some(0));
        assert_eq!(micros("9007199254.740991"), Some(9_007_199_254_740_991));
        assert_eq!(micros("9007199254.740992"), None);
    }
    #[test]
    fn preserves_absent_versus_explicit_zero_hour_rates_and_off_peak() {
        let mut rule = Rule {
            model: "claude-".into(),
            prices: prices([0; 4], None),
            off_peak_prices: prices([0; 4], None),
            off_peak: true,
            ..Default::default()
        };
        rule.prices[4] = "0".into();
        rule.off_peak_prices[0] = "1.234567".into();
        let built = rule.build().unwrap();
        assert_eq!(built.cache_creation_1h_micros_per_million, Some(0));
        assert_eq!(
            built
                .off_peak
                .as_ref()
                .unwrap()
                .cache_creation_1h_micros_per_million,
            None
        );
        assert_eq!(Rule::from(&built).build().unwrap(), built);
    }
    #[test]
    fn unique_trimmed_model_prefixes_and_nonempty_group_are_required() {
        let rule = Rule {
            model: " model ".into(),
            prices: prices([0; 4], None),
            ..Default::default()
        };
        assert!(group(Uuid::nil(), "x", &[rule.clone(), rule.clone()]).is_none());
        assert!(group(Uuid::nil(), " ", std::slice::from_ref(&rule)).is_none());
        assert_eq!(group(Uuid::nil(), " x ", &[rule]).unwrap().name, "x");
    }
}
