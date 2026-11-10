//! AskUserQuestion needs deliverable answers, not an empty permission approval.
use super::*;

pub(super) fn answer(original: &Value, updated: &Value) -> Result<Value, String> {
    let questions = original["questions"]
        .as_array()
        .filter(|q| !q.is_empty())
        .ok_or("Claude question list is missing")?;
    let answers = updated["answers"]
        .as_object()
        .ok_or("AskUserQuestion requires answers")?;
    let mut normalized = serde_json::Map::new();
    for question in questions {
        let text = question["question"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .ok_or("invalid Claude question")?;
        let answer = answers
            .get(text)
            .or_else(|| question["header"].as_str().and_then(|h| answers.get(h)))
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or("answer every Claude question using its full text or header")?;
        normalized.insert(text.into(), json!(answer));
    }
    // The original schema is authoritative; a reply may consist of answers only.
    let mut input = original.clone();
    input["answers"] = Value::Object(normalized);
    Ok(input)
}
