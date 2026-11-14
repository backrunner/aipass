//! Parse the Agent's compact line diff, retaining file coordinates and UTF-8.
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Side {
    pub line: Option<usize>,
    pub text: String,
    pub changed: Option<Range<usize>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub old: Option<Side>,
    pub next: Option<Side>,
    pub changed: bool,
}

fn replacement(mut old: Side, mut next: Side) -> Row {
    let prefix = old
        .text
        .chars()
        .zip(next.text.chars())
        .take_while(|(a, b)| a == b)
        .map(|(c, _)| c.len_utf8())
        .sum::<usize>();
    let suffix = old.text[prefix..]
        .chars()
        .rev()
        .zip(next.text[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(c, _)| c.len_utf8())
        .sum::<usize>();
    old.changed = Some(prefix..old.text.len() - suffix);
    next.changed = Some(prefix..next.text.len() - suffix);
    Row {
        old: Some(old),
        next: Some(next),
        changed: true,
    }
}

fn pair(rows: &mut Vec<Row>, old: &[Side], next: &[Side]) {
    for i in 0..old.len().max(next.len()) {
        rows.push(match (old.get(i), next.get(i)) {
            (Some(a), Some(b)) => replacement(a.clone(), b.clone()),
            (a, b) => Row {
                old: a.cloned(),
                next: b.cloned(),
                changed: true,
            },
        });
    }
}

fn align(rows: &mut Vec<Row>, old: &[Side], next: &[Side]) {
    let width = next.len() + 1;
    let cells = (old.len() + 1).checked_mul(width);
    if cells.is_none_or(|cells| cells > 1_000_000) {
        pair(rows, old, next);
        return;
    }
    let mut lcs = vec![0_u32; cells.unwrap()];
    for i in (0..old.len()).rev() {
        for j in (0..next.len()).rev() {
            lcs[i * width + j] = if old[i].text == next[j].text {
                lcs[(i + 1) * width + j + 1] + 1
            } else {
                lcs[(i + 1) * width + j].max(lcs[i * width + j + 1])
            };
        }
    }
    let (mut i, mut j, mut a, mut b) = (0, 0, 0, 0);
    while i < old.len() && j < next.len() {
        if old[i].text == next[j].text {
            pair(rows, &old[a..i], &next[b..j]);
            rows.push(Row {
                old: Some(old[i].clone()),
                next: Some(next[j].clone()),
                changed: false,
            });
            i += 1;
            j += 1;
            a = i;
            b = j;
        } else if lcs[(i + 1) * width + j] >= lcs[i * width + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    pair(rows, &old[a..], &next[b..]);
}

fn coordinates(line: &str) -> Option<(usize, usize)> {
    let rest = line.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(" +")?;
    let (next, _) = rest.split_once(" @@")?;
    Some((
        old.split(',').next()?.parse::<usize>().ok()?.max(1),
        next.split(',').next()?.parse::<usize>().ok()?.max(1),
    ))
}

pub fn parse(input: &str) -> Vec<Row> {
    if input.is_empty() || input.trim() == "(no changes)" {
        return vec![];
    }
    let (mut old_line, mut next_line) = (None, None);
    let (mut old, mut next, mut rows) = (Vec::new(), Vec::new(), Vec::new());
    for line in input.split('\n') {
        if let Some((a, b)) = coordinates(line) {
            align(&mut rows, &old, &next);
            old.clear();
            next.clear();
            old_line = Some(a);
            next_line = Some(b);
        } else if line.starts_with("- ") || line == "-" {
            old.push(Side {
                line: old_line,
                text: line.get(2..).unwrap_or("").into(),
                changed: None,
            });
            old_line = old_line.map(|n| n.saturating_add(1));
        } else if line.starts_with("+ ") || line == "+" {
            next.push(Side {
                line: next_line,
                text: line.get(2..).unwrap_or("").into(),
                changed: None,
            });
            next_line = next_line.map(|n| n.saturating_add(1));
        } else {
            align(&mut rows, &old, &next);
            old.clear();
            next.clear();
            let text = line.strip_prefix("  ").unwrap_or(line).to_owned();
            rows.push(Row {
                old: Some(Side {
                    line: old_line,
                    text: text.clone(),
                    changed: None,
                }),
                next: Some(Side {
                    line: next_line,
                    text,
                    changed: None,
                }),
                changed: false,
            });
            old_line = old_line.map(|n| n.saturating_add(1));
            next_line = next_line.map(|n| n.saturating_add(1));
        }
    }
    align(&mut rows, &old, &next);
    rows
}

pub fn changes(rows: &[Row]) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter_map(|(i, row)| (row.changed && (i == 0 || !rows[i - 1].changed)).then_some(i))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_file_lines_alignment_and_utf8_word_ranges() {
        let rows = parse("@@ -30,3 +41,4 @@\n- {\n-   \"名称\": \"旧🙂\",\n- }\n+ {\n+   \"名称\": \"新🐈\",\n+   \"ok\": true\n+ }");
        assert_eq!(rows.len(), 4);
        assert!(!rows[0].changed && !rows[3].changed);
        let old = rows[1].old.as_ref().unwrap();
        let next = rows[1].next.as_ref().unwrap();
        assert_eq!((old.line, next.line), (Some(31), Some(42)));
        assert_eq!(&old.text[old.changed.clone().unwrap()], "旧🙂");
        assert_eq!(&next.text[next.changed.clone().unwrap()], "新🐈");
        assert!(rows[2].old.is_none());
        assert_eq!(changes(&rows), [1]);
        assert_eq!(rows[3].old.as_ref().unwrap().line, Some(32));
    }
    #[test]
    fn legacy_empty_and_multiple_hunks_do_not_invent_coordinates() {
        assert!(parse("(no changes)").is_empty());
        assert!(parse("").is_empty());
        let rows =
            parse("- <script>\n+ literal & text\n@@ -0,0 +2,1 @@\n+\n@@ -99 +101 @@\n  tail");
        assert_eq!(rows[0].old.as_ref().unwrap().line, None);
        assert_eq!(rows[0].next.as_ref().unwrap().text, "literal & text");
        assert_eq!(rows[1].next.as_ref().unwrap().line, Some(2));
        assert_eq!(rows[1].next.as_ref().unwrap().text, "");
        assert_eq!(rows[2].old.as_ref().unwrap().line, Some(99));
    }
    #[test]
    fn large_replacements_use_bounded_alignment() {
        let input = (0..1100)
            .map(|i| format!("- before{i}"))
            .chain((0..1100).map(|i| format!("+ after{i}")))
            .collect::<Vec<_>>()
            .join("\n");
        let rows = parse(&input);
        assert_eq!(rows.len(), 1100);
        assert!(rows.iter().all(|r| r.changed));
    }
}
