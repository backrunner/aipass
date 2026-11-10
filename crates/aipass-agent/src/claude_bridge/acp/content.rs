//! Loss-aware ACP content conversion. Images remain images, including handoffs.
use super::*;

pub(super) fn to_acp(value: &Value) -> Result<Value, String> {
    if let Some(text) = value.as_str() {
        return Ok(json!([{"type":"text","text":text}]));
    }
    let blocks = value
        .as_array()
        .ok_or("Claude content must be text or blocks")?;
    blocks.iter().map(|block| {
        let mut mapped = match block["type"].as_str() {
            Some("text") if block["text"].is_string() => json!({"type":"text","text":block["text"]}),
            Some("image") if block["source"]["type"] == "base64" && block["source"]["data"].is_string() && block["source"]["media_type"].is_string() => json!({"type":"image","data":block["source"]["data"],"mimeType":block["source"]["media_type"]}),
            Some("document") if block["source"]["type"] == "base64" && block["source"]["data"].is_string() && block["source"]["media_type"].is_string() => json!({"type":"resource","resource":{"uri":"aipass://prompt/document","blob":block["source"]["data"],"mimeType":block["source"]["media_type"]}}),
            Some("document") if block["source"]["type"] == "text" && block["source"]["data"].is_string() => json!({"type":"resource","resource":{"uri":"aipass://prompt/document","text":block["source"]["data"],"mimeType":"text/plain"}}),
            _ => return Err("content cannot be represented by Claude ACP; use text, base64 images or embedded documents".into()),
        };
        // Preserve provider-specific cache/citation fields across our ACP hop.
        mapped["_meta"] = json!({"aipass":{"nativeBlock":block}});
        Ok(mapped)
    }).collect::<Result<Vec<_>, _>>().map(Value::Array)
}
pub(super) fn to_native(value: &Value) -> Result<Value, String> {
    let blocks = value
        .as_array()
        .ok_or("ACP prompt must contain content blocks")?;
    blocks.iter().map(|block| {
        if let Some(native) = block.pointer("/_meta/aipass/nativeBlock") {
            // This extension originates inside the trusted adapter only.
            return Ok(native.clone());
        }
        match block["type"].as_str() {
            Some("text") if block["text"].is_string() => Ok(json!({"type":"text","text":block["text"]})),
            Some("image") if block["data"].is_string() && block["mimeType"].is_string() => Ok(json!({"type":"image","source":{"type":"base64","data":block["data"],"media_type":block["mimeType"]}})),
            Some("resource") if block["resource"]["text"].is_string() => Ok(json!({"type":"text","text":block["resource"]["text"]})),
            Some("resource") if block["resource"]["blob"].is_string() => Ok(json!({"type":"document","source":{"type":"base64","data":block["resource"]["blob"],"media_type":block["resource"]["mimeType"]}})),
            _ => Err("unsupported ACP content block".into()),
        }
    }).collect::<Result<Vec<_>, _>>().map(Value::Array)
}
