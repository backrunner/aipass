//! Provider form assignments and optional metadata for new credentials.
use aipass_agent_protocol::SensitiveString;
use aipass_provider_registry::BillingRule;
use aipass_vault::SecretMetadataInput;

#[derive(Default)]
pub struct AddFields {
    pub aliases: String,
    pub headers: SensitiveString,
    pub group: String,
    pub rate: String,
    pub currency: String,
    pub unit_price: String,
}

impl AddFields {
    pub fn metadata(&self) -> SecretMetadataInput {
        let optional = |value: &str| (!value.trim().is_empty()).then(|| value.trim().to_owned());
        let billing = BillingRule {
            rate: optional(&self.rate),
            currency: optional(&self.currency),
            unit_price: optional(&self.unit_price),
            ..Default::default()
        };
        SecretMetadataInput {
            group: optional(&self.group),
            billing: (billing.rate.is_some()
                || billing.currency.is_some()
                || billing.unit_price.is_some())
            .then_some(billing),
            ..Default::default()
        }
    }
}

/// Split at unescaped commas and the first unescaped equals sign, matching
/// App.svelte's parseAssignment. Empty/invalid assignments are ignored there.
pub fn pairs(value: &str) -> Vec<(String, String)> {
    let mut result = Vec::new();
    let (mut key, mut value_part) = (String::new(), String::new());
    let mut separator = false;
    let mut chars = value.chars().peekable();
    loop {
        match chars.next() {
            Some('\\')
                if chars
                    .peek()
                    .is_some_and(|ch| matches!(ch, '\\' | ',' | '=')) =>
            {
                let ch = chars.next().unwrap();
                if separator {
                    value_part.push(ch);
                } else {
                    key.push(ch);
                }
            }
            Some('=') if !separator => separator = true,
            end @ (Some(',') | None) => {
                if separator && !key.trim().is_empty() {
                    result.push((key.trim().to_owned(), value_part.trim().to_owned()));
                }
                key.clear();
                value_part.clear();
                separator = false;
                if end.is_none() {
                    break;
                }
            }
            Some(ch) => {
                if separator {
                    value_part.push(ch);
                } else {
                    key.push(ch);
                }
            }
        }
    }
    result
}

pub fn encode_pairs(pairs: &[(String, String)]) -> String {
    let escape = |value: &str| {
        value
            .replace('\\', "\\\\")
            .replace(',', "\\,")
            .replace('=', "\\=")
    };
    pairs
        .iter()
        .map(|(name, value)| format!("{}={}", escape(name), escape(value)))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assignment_escaping_preserves_commas_equals_unicode_and_values() {
        assert_eq!(
            pairs(r"fast\,cheap=gpt\=latest, x-version=a=b, 路径=c:\\dir, invalid, empty="),
            [
                ("fast,cheap".into(), "gpt=latest".into()),
                ("x-version".into(), "a=b".into()),
                ("路径".into(), r"c:\dir".into()),
                ("empty".into(), String::new())
            ]
        );
    }
}
