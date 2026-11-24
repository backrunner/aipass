//! Public presentation data supplied by a subscription plugin. All text is inert.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum UiText {
    Plain(String),
    Localized {
        en: String,
        #[serde(rename = "zh-CN", default, skip_serializing_if = "Option::is_none")]
        zh_cn: Option<String>,
    },
}
impl UiText {
    pub fn valid(&self, max: usize) -> bool {
        let valid =
            |s: &str| !s.trim().is_empty() && s.len() <= max && !s.chars().any(char::is_control);
        match self {
            Self::Plain(s) => valid(s),
            Self::Localized { en, zh_cn } => valid(en) && zh_cn.as_deref().is_none_or(valid),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tone {
    Info,
    Success,
    Warning,
    Danger,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fact {
    pub label: UiText,
    pub value: UiText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copyable: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Block {
    Note {
        text: UiText,
        tone: Tone,
    },
    Facts {
        items: Vec<Fact>,
    },
    Status {
        label: UiText,
        value: UiText,
        tone: Tone,
    },
    Progress {
        label: UiText,
        percent: f64,
    },
    Table {
        columns: Vec<UiText>,
        rows: Vec<Vec<UiText>>,
    },
}
impl Block {
    fn valid(&self) -> bool {
        match self {
            Self::Note { text, .. } => text.valid(1024),
            Self::Facts { items } => {
                (1..=16).contains(&items.len())
                    && items
                        .iter()
                        .all(|f| f.label.valid(128) && f.value.valid(512))
            }
            Self::Status { label, value, .. } => label.valid(128) && value.valid(128),
            Self::Progress { label, percent } => {
                label.valid(128) && (0.0..=100.0).contains(percent)
            }
            Self::Table { columns, rows } => {
                (1..=6).contains(&columns.len())
                    && columns.iter().all(|c| c.valid(128))
                    && rows.len() <= 32
                    && rows
                        .iter()
                        .all(|r| r.len() == columns.len() && r.iter().all(|c| c.valid(512)))
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub title: UiText,
    pub blocks: Vec<Block>,
}
impl Section {
    pub fn valid(&self) -> bool {
        self.title.valid(128)
            && (1..=12).contains(&self.blocks.len())
            && self.blocks.iter().all(Block::valid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_bounded_rectangular_tables_and_valid_percentages_are_accepted() {
        for block in [
            json!({"type":"table","columns":["Model","Seats"],"rows":[["Solo"]]}),
            json!({"type":"table","columns":["Account"],"rows":vec![vec!["A"];33]}),
            json!({"type":"progress","label":{"en":"Seats"},"percent":101}),
            json!({"type":"note","text":"bad\nlabel","tone":"info"}),
        ] {
            let section: Section =
                serde_json::from_value(json!({"title":"Summary","blocks":[block]})).unwrap();
            assert!(!section.valid());
        }
        assert!(serde_json::from_value::<Block>(json!({"type":"html","html":"demo"})).is_err());
        assert!(serde_json::from_value::<Block>(
            json!({"type":"note","tone":"info","text":"demo","href":"https://example.test"})
        )
        .is_err());
    }
}
