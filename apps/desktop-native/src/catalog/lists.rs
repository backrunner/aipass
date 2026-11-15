//! Match the shared provider form's escaped comma lists and URL list boundary.
pub fn encode(values: impl IntoIterator<Item = impl AsRef<str>>) -> String {
    values
        .into_iter()
        .map(|value| value.as_ref().replace('\\', "\\\\").replace(',', "\\,"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn parse(value: &str) -> Vec<String> {
    split(value, false)
}
pub fn endpoints(value: &str) -> Vec<String> {
    split(value, true)
}

fn split(value: &str, urls: bool) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut chars = value.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if ch == '\\'
            && chars
                .peek()
                .is_some_and(|(_, next)| matches!(next, '\\' | ',' | '='))
        {
            current.push(chars.next().unwrap().1);
        } else if ch == ','
            && (!urls || {
                let rest = value[index + 1..].trim_start();
                rest.get(..7)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"))
                    || rest
                        .get(..8)
                        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
            })
        {
            if !current.trim().is_empty() {
                values.push(current.trim().to_owned());
            }
            current.clear();
        } else {
            current.push(ch);
        }
    }
    if !current.trim().is_empty() {
        values.push(current.trim().to_owned());
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escaped_comma_backslash_unicode_and_equals_round_trip() {
        let values = ["work, team", "path\\name", "a=b", "测试"];
        assert_eq!(parse(&encode(values)), values);
    }
    #[test]
    fn endpoints_keep_query_commas_and_split_only_a_new_http_url() {
        assert_eq!(
            endpoints("https://a.invalid/v1?models=a,b, https://b.invalid/v1"),
            ["https://a.invalid/v1?models=a,b", "https://b.invalid/v1"]
        );
        let values = [
            "https://a.invalid/?next=,https://b.invalid",
            "https://c.invalid",
        ];
        assert_eq!(endpoints(&encode(values)), values);
    }
}
