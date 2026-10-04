//! Bounded protobuf, Connect envelopes, and incremental JSON lines used by the
//! native provider codecs. Unknown protobuf fields remain safely skippable.
use serde_json::{json, Value};
pub const MAX_FRAME: usize = 8 * 1024 * 1024;
pub fn varint(mut v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    while v > 127 {
        out.push((v as u8 & 127) | 128);
        v >>= 7;
    }
    out.push(v as u8);
    out
}
fn read_varint(bytes: &[u8], cursor: &mut usize) -> Result<u64, String> {
    let mut v = 0;
    for shift in (0..70).step_by(7) {
        let b = *bytes.get(*cursor).ok_or("truncated protobuf varint")?;
        *cursor += 1;
        if shift == 63 && b > 1 {
            return Err("protobuf varint overflow".into());
        }
        v |= ((b & 127) as u64) << shift;
        if b < 128 {
            return Ok(v);
        }
    }
    Err("protobuf varint overflow".into())
}
pub fn uint(field: u32, value: u64) -> Vec<u8> {
    [varint((field as u64) << 3), varint(value)].concat()
}
pub fn bytes(field: u32, value: &[u8]) -> Vec<u8> {
    [
        varint((field as u64) << 3 | 2),
        varint(value.len() as u64),
        value.to_vec(),
    ]
    .concat()
}
pub fn string(field: u32, value: &str) -> Vec<u8> {
    bytes(field, value.as_bytes())
}
#[derive(Clone, Debug)]
pub enum Field<'a> {
    Int(u64),
    Bytes(&'a [u8]),
    Fixed32(u32),
    Fixed64(u64),
}
pub fn fields(bytes: &[u8]) -> Result<Vec<(u32, Field<'_>)>, String> {
    if bytes.len() > MAX_FRAME {
        return Err("protobuf message exceeds limit".into());
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let tag = read_varint(bytes, &mut i)?;
        let id = (tag >> 3) as u32;
        if id == 0 {
            return Err("invalid protobuf field".into());
        }
        let v = match tag & 7 {
            0 => Field::Int(read_varint(bytes, &mut i)?),
            1 => {
                let b = bytes.get(i..i + 8).ok_or("truncated protobuf fixed64")?;
                i += 8;
                Field::Fixed64(u64::from_le_bytes(b.try_into().unwrap()))
            }
            2 => {
                let n = usize::try_from(read_varint(bytes, &mut i)?)
                    .map_err(|_| "protobuf length overflow")?;
                let end = i
                    .checked_add(n)
                    .filter(|n| *n <= bytes.len())
                    .ok_or("truncated protobuf bytes")?;
                let b = &bytes[i..end];
                i = end;
                Field::Bytes(b)
            }
            5 => {
                let b = bytes.get(i..i + 4).ok_or("truncated protobuf fixed32")?;
                i += 4;
                Field::Fixed32(u32::from_le_bytes(b.try_into().unwrap()))
            }
            _ => return Err("unsupported protobuf wire type".into()),
        };
        out.push((id, v));
        if out.len() > 65536 {
            return Err("too many protobuf fields".into());
        }
    }
    Ok(out)
}
pub fn field_bytes<'a>(fields: &[(u32, Field<'a>)], id: u32) -> Option<&'a [u8]> {
    fields.iter().find_map(|(n, v)| {
        if *n == id {
            if let Field::Bytes(b) = v {
                Some(*b)
            } else {
                None
            }
        } else {
            None
        }
    })
}
pub fn field_text(fields: &[(u32, Field<'_>)], id: u32) -> String {
    field_bytes(fields, id)
        .and_then(|b| std::str::from_utf8(b).ok())
        .unwrap_or("")
        .to_owned()
}
pub fn field_int(fields: &[(u32, Field<'_>)], id: u32) -> u64 {
    fields
        .iter()
        .find_map(|(n, v)| {
            if *n == id {
                if let Field::Int(v) = v {
                    Some(*v)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .unwrap_or(0)
}
pub fn frame(flags: u8, data: &[u8]) -> Vec<u8> {
    [
        vec![flags],
        (data.len() as u32).to_be_bytes().to_vec(),
        data.to_vec(),
    ]
    .concat()
}
#[derive(Default)]
pub struct Frames {
    buffer: Vec<u8>,
}
impl Frames {
    pub fn push(&mut self, data: &[u8]) -> Result<Vec<(u8, Vec<u8>)>, String> {
        self.buffer.extend_from_slice(data);
        let mut out = Vec::new();
        let mut pos = 0;
        while self.buffer.len() - pos >= 5 {
            let n = u32::from_be_bytes(self.buffer[pos + 1..pos + 5].try_into().unwrap()) as usize;
            if n > MAX_FRAME {
                return Err("Connect frame exceeds limit".into());
            }
            if self.buffer.len() - pos < 5 + n {
                break;
            }
            out.push((self.buffer[pos], self.buffer[pos + 5..pos + 5 + n].to_vec()));
            pos += 5 + n;
        }
        self.buffer.drain(..pos);
        if self.buffer.len() > MAX_FRAME + 5 {
            return Err("Connect frame exceeds limit".into());
        }
        Ok(out)
    }
    pub fn finish(&self) -> Result<(), String> {
        if self.buffer.is_empty() {
            Ok(())
        } else {
            Err("truncated Connect frame".into())
        }
    }
}
#[derive(Default)]
pub struct Lines {
    buffer: Vec<u8>,
}
impl Lines {
    pub fn push(&mut self, data: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        let mut out = vec![];
        for part in data.split_inclusive(|b| *b == b'\n') {
            if self.buffer.len() + part.len() > MAX_FRAME {
                return Err("provider line exceeds limit".into());
            }
            self.buffer.extend_from_slice(part);
            if part.last() == Some(&b'\n') {
                out.push(std::mem::take(&mut self.buffer));
            }
        }
        Ok(out)
    }
    pub fn finish(&mut self) -> Vec<Vec<u8>> {
        if self.buffer.is_empty() {
            vec![]
        } else {
            vec![std::mem::take(&mut self.buffer)]
        }
    }
}
pub struct ChatStream {
    pub model: String,
    pub id: String,
    pub done: bool,
    pub tools: usize,
    pub begun: bool,
}
/// Incremental function arguments must form an object before completion.
/// Keep this check in provider decoders: same-protocol SSE is lossless passthrough.
#[derive(Default)]
pub struct ToolArguments(String);
impl ToolArguments {
    pub fn push(&mut self, text: &str) -> Result<(), String> {
        if self.0.len().saturating_add(text.len()) > MAX_FRAME {
            return Err("provider tool arguments exceed limit".into());
        }
        self.0.push_str(text);
        Ok(())
    }
    pub fn finish(&self) -> Result<(), String> {
        let value: Value = serde_json::from_str(&self.0)
            .map_err(|_| "provider returned incomplete tool arguments")?;
        if !value.is_object() {
            return Err("provider tool arguments must be an object".into());
        }
        Ok(())
    }
}
impl ChatStream {
    pub fn new(model: &str, id: &str) -> Self {
        Self {
            model: model.into(),
            id: id.into(),
            done: false,
            tools: 0,
            begun: false,
        }
    }
    pub fn delta(&mut self, delta: Value) -> Vec<Vec<u8>> {
        let mut out = vec![];
        if !self.begun {
            self.begun = true;
            out.push(self.chunk(json!({"role":"assistant","content":""}), Value::Null, None));
        }
        out.push(self.chunk(delta, Value::Null, None));
        out
    }
    fn chunk(&self, delta: Value, reason: Value, usage: Option<Value>) -> Vec<u8> {
        let mut v = json!({"id":self.id,"object":"chat.completion.chunk","created":0,"model":self.model,"choices":[{"index":0,"delta":delta,"finish_reason":reason}]});
        if let Some(u) = usage {
            v["usage"] = u;
        }
        format!("data: {v}\n\n").into_bytes()
    }
    pub fn stop(&mut self, reason: &str, usage: Option<Value>) -> Result<Vec<Vec<u8>>, String> {
        if self.done {
            return Err("duplicate provider stream completion".into());
        }
        self.done = true;
        Ok(vec![
            self.chunk(
                json!({}),
                json!(if self.tools > 0 && reason == "stop" {
                    "tool_calls"
                } else {
                    reason
                }),
                usage,
            ),
            b"data: [DONE]\n\n".to_vec(),
        ])
    }
    pub fn finish(&self) -> Result<(), String> {
        if self.done {
            Ok(())
        } else {
            Err("provider stream ended before completion".into())
        }
    }
}
pub fn text(content: &Value) -> String {
    if let Some(s) = content.as_str() {
        s.into()
    } else {
        content
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| p["type"] == "text")
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("")
    }
}

pub fn uncompress(flags: u8, data: &[u8]) -> Result<Vec<u8>, String> {
    if flags & !3 != 0 {
        return Err("unsupported Connect frame flags".into());
    }
    if flags & 1 == 0 {
        return Ok(data.to_vec());
    }
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(data)
        .take(MAX_FRAME as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|_| "invalid compressed Connect frame")?;
    if out.len() > MAX_FRAME {
        return Err("decompressed Connect frame exceeds limit".into());
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_frames_accept_fragmented_prefix_and_require_full_payload() {
        let data = frame(0, b"hello");
        let mut r = Frames::default();
        for b in &data[..9] {
            let _ = r.push(&[*b]).unwrap();
        }
        assert!(r.finish().is_err());
        assert_eq!(r.push(&data[9..]).unwrap()[0].1, b"hello");
        r.finish().unwrap();
        assert!(Frames::default().push(&[0, 255, 255, 255, 255]).is_err());
    }
    #[test]
    fn protobuf_rejects_overflow_and_truncation() {
        assert!(fields(&[10, 5, 1]).is_err());
        let data = [uint(1, 42), string(2, "text")].concat();
        let f = fields(&data).unwrap();
        assert_eq!(field_int(&f, 1), 42);
        assert_eq!(field_text(&f, 2), "text");
    }
}
