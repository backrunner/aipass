//! Literal syntax spans; configuration text is never interpreted as markup.
use std::ops::Range;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Key,
    String,
    Number,
    Keyword,
    Section,
    Comment,
    Variable,
}
pub fn spans(text: &str, path: &str) -> Vec<(Range<usize>, Kind)> {
    let path = path.to_ascii_lowercase();
    if path.ends_with(".json") || path.ends_with(".jsonc") {
        return json(text);
    }
    let trimmed = text.trim_start();
    let start = text.len() - trimmed.len();
    if path.ends_with(".toml") {
        if trimmed.starts_with('#') {
            return vec![(start..text.len(), Kind::Comment)];
        }
        if trimmed.starts_with('[') {
            if let Some(end) = trimmed.rfind(']') {
                return vec![(start..start + end + 1, Kind::Section)];
            }
        }
        let Some(eq) = text.find('=') else {
            return vec![];
        };
        let mut result = vec![(start..text[..eq].trim_end().len(), Kind::Key)];
        let value = &text[eq + 1..];
        let mut quoted = false;
        let mut escaped = false;
        let comment = value.char_indices().find_map(|(i, c)| {
            if escaped {
                escaped = false;
                return None;
            }
            if c == '\\' {
                escaped = true;
            } else if c == '"' {
                quoted = !quoted;
            }
            (!quoted && c == '#').then_some(eq + 1 + i)
        });
        let end = comment.unwrap_or(text.len());
        let value = text[eq + 1..end].trim();
        let a = eq + 1 + text[eq + 1..end].len() - text[eq + 1..end].trim_start().len();
        let kind = if value.starts_with('"') && value.ends_with('"') {
            Some(Kind::String)
        } else if matches!(value, "true" | "false") {
            Some(Kind::Keyword)
        } else if !value.is_empty() && value.parse::<f64>().is_ok() {
            Some(Kind::Number)
        } else {
            None
        };
        if let Some(kind) = kind {
            result.push((a..a + value.len(), kind));
        }
        if let Some(comment) = comment {
            result.push((comment..text.len(), Kind::Comment));
        }
        return result;
    }
    if path.ends_with(".env")
        || path.contains(".env.")
        || path.ends_with(".ini")
        || path.ends_with(".conf")
    {
        if trimmed.starts_with('#') {
            return vec![(0..text.len(), Kind::Comment)];
        }
        let Some(eq) = text.find('=') else {
            return vec![];
        };
        let mut result = vec![(0..eq, Kind::Key)];
        let value = &text[eq + 1..];
        if value.starts_with('"') && value.ends_with('"') {
            result.push((eq + 1..text.len(), Kind::String));
        } else if value.starts_with('$') {
            result.push((eq + 1..text.len(), Kind::Variable));
        }
        return result;
    }
    vec![]
}
fn json(text: &str) -> Vec<(Range<usize>, Kind)> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        if bytes[i] == b'"' {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(bytes.len());
                } else if bytes[i] == b'"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            let kind = if text[i..].trim_start().starts_with(':') {
                Kind::Key
            } else {
                Kind::String
            };
            spans.push((start..i, kind));
        } else if (bytes[i].is_ascii_digit() || bytes[i] == b'-')
            && (i == 0 || b" \t\r\n,:{[".contains(&bytes[i - 1]))
        {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || b".eE+-".contains(&bytes[i])) {
                i += 1;
            }
            spans.push((start..i, Kind::Number));
        } else if bytes[i].is_ascii_alphabetic() {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            if matches!(&text[start..i], "true" | "false" | "null") {
                spans.push((start..i, Kind::Keyword));
            }
        } else {
            i += text[i..].chars().next().unwrap().len_utf8();
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn literal_unicode_and_escaped_json_have_byte_aligned_spans() {
        let text = "  \"名称🙂\": \"<script>\\\" & 新🐈\",";
        let spans = spans(text, "auth.json");
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].1, Kind::Key);
        assert_eq!(&text[spans[1].0.clone()], "\"<script>\\\" & 新🐈\"");
        assert_eq!(spans[1].1, Kind::String);
    }
    #[test]
    fn toml_comments_do_not_start_inside_quotes() {
        let text = "name = \"before#after\" # note";
        let spans = spans(text, "config.toml");
        assert_eq!(
            spans
                .iter()
                .map(|(r, _)| &text[r.clone()])
                .collect::<Vec<_>>(),
            ["name", "\"before#after\"", "# note"]
        );
        assert_eq!(super::spans("KEY=$OTHER", "tool.env")[1].1, Kind::Variable);
    }
}
