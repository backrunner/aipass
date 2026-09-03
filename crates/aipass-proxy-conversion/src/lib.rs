use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

mod request;
mod response;
mod stream;

pub use stream::StreamConverter;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ProxyProtocol {
    OpenAiResponses,
    OpenAiChatCompletions,
    AnthropicMessages,
}

impl ProxyProtocol {
    pub fn path(self) -> &'static str {
        match self {
            Self::OpenAiResponses => "/v1/responses",
            Self::OpenAiChatCompletions => "/v1/chat/completions",
            Self::AnthropicMessages => "/v1/messages",
        }
    }

    pub fn from_path(path: &str) -> Option<Self> {
        match path.trim_end_matches('/') {
            "/v1/responses" => Some(Self::OpenAiResponses),
            "/v1/chat/completions" => Some(Self::OpenAiChatCompletions),
            "/v1/messages" => Some(Self::AnthropicMessages),
            _ => None,
        }
    }
}

/// `input_tokens` contains only non-cached input. Cache read and creation
/// tokens are separate fields, regardless of the upstream wire protocol.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
}

#[derive(Debug, Error)]
pub enum ConversionError {
    #[error("invalid {protocol:?} payload: {message}")]
    InvalidPayload {
        protocol: ProxyProtocol,
        message: String,
    },
    #[error("protocol conversion is unavailable: {0:?} to {1:?}")]
    Unsupported(ProxyProtocol, ProxyProtocol),
    #[error("invalid server-sent event: {0}")]
    InvalidEvent(String),
}

pub trait ConversionPlugin: Send + Sync {
    fn convert_request(
        &self,
        from: ProxyProtocol,
        to: ProxyProtocol,
        payload: Value,
    ) -> Result<Value, ConversionError>;
    fn convert_response(
        &self,
        from: ProxyProtocol,
        to: ProxyProtocol,
        payload: Value,
    ) -> Result<Value, ConversionError>;
    /// Stateless same-protocol passthrough only. Cross-protocol SSE
    /// conversion is stateful and lives in [`StreamConverter`]; calling this
    /// for a cross-protocol pair returns [`ConversionError::Unsupported`].
    fn convert_stream_event(
        &self,
        from: ProxyProtocol,
        to: ProxyProtocol,
        event: &str,
    ) -> Result<Vec<String>, ConversionError>;
    fn extract_usage(&self, protocol: ProxyProtocol, payload: &Value) -> TokenUsage;
}

/// Same-protocol forwarding is lossless. Cross-protocol conversion covers
/// Anthropic Messages ↔ OpenAI Chat Completions and Anthropic Messages ↔
/// OpenAI Responses for requests and non-streaming responses; streaming
/// conversion for those pairs is provided by [`StreamConverter`]. Use
/// [`supports`] to check a pair before routing.
#[derive(Clone, Default)]
pub struct BuiltinConversionPlugin;

impl BuiltinConversionPlugin {
    pub fn supports(from: ProxyProtocol, to: ProxyProtocol) -> bool {
        supports(from, to)
    }
}

/// True for same-protocol pairs and every implemented cross-protocol pair
/// (Anthropic Messages ↔ either OpenAI wire format).
pub fn supports(from: ProxyProtocol, to: ProxyProtocol) -> bool {
    use ProxyProtocol::{
        AnthropicMessages as AM, OpenAiChatCompletions as CC, OpenAiResponses as RS,
    };
    from == to || matches!((from, to), (AM, CC) | (CC, AM) | (AM, RS) | (RS, AM))
}

impl ConversionPlugin for BuiltinConversionPlugin {
    fn convert_request(
        &self,
        from: ProxyProtocol,
        to: ProxyProtocol,
        payload: Value,
    ) -> Result<Value, ConversionError> {
        if from == to {
            Ok(payload)
        } else {
            request::convert(from, to, payload)
        }
    }

    fn convert_response(
        &self,
        from: ProxyProtocol,
        to: ProxyProtocol,
        payload: Value,
    ) -> Result<Value, ConversionError> {
        if from == to {
            Ok(payload)
        } else {
            response::convert(from, to, payload)
        }
    }

    fn convert_stream_event(
        &self,
        from: ProxyProtocol,
        to: ProxyProtocol,
        event: &str,
    ) -> Result<Vec<String>, ConversionError> {
        if from == to {
            Ok(vec![event.to_string()])
        } else {
            Err(ConversionError::Unsupported(from, to))
        }
    }

    fn extract_usage(&self, protocol: ProxyProtocol, payload: &Value) -> TokenUsage {
        extract_usage(protocol, payload)
    }
}

pub(crate) fn invalid(protocol: ProxyProtocol, message: impl Into<String>) -> ConversionError {
    ConversionError::InvalidPayload {
        protocol,
        message: message.into(),
    }
}

pub(crate) fn number(value: Option<&Value>) -> u64 {
    value.and_then(Value::as_u64).unwrap_or_default()
}

fn extract_usage(protocol: ProxyProtocol, payload: &Value) -> TokenUsage {
    let usage = payload.get("usage").unwrap_or(payload);
    match protocol {
        ProxyProtocol::AnthropicMessages => TokenUsage {
            input_tokens: number(usage.get("input_tokens")),
            output_tokens: number(usage.get("output_tokens")),
            cache_read_tokens: number(usage.get("cache_read_input_tokens")),
            cache_creation_tokens: number(usage.get("cache_creation_input_tokens")),
        },
        ProxyProtocol::OpenAiResponses | ProxyProtocol::OpenAiChatCompletions => {
            let total_input =
                number(usage.get("input_tokens")).max(number(usage.get("prompt_tokens")));
            let cache_read = number(usage.pointer("/input_tokens_details/cached_tokens")).max(
                number(usage.pointer("/prompt_tokens_details/cached_tokens")),
            );
            let cache_creation =
                number(usage.pointer("/input_tokens_details/cache_creation_tokens")).max(number(
                    usage.pointer("/prompt_tokens_details/cache_creation_tokens"),
                ));
            TokenUsage {
                input_tokens: total_input
                    .saturating_sub(cache_read)
                    .saturating_sub(cache_creation),
                output_tokens: number(usage.get("output_tokens"))
                    .max(number(usage.get("completion_tokens"))),
                cache_read_tokens: cache_read,
                cache_creation_tokens: cache_creation,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_protocol_events_are_lossless() {
        let event = "event: response.output_text.delta\r\ndata: {\"delta\":\"hello\"}\r\n\r\n";
        assert_eq!(
            BuiltinConversionPlugin
                .convert_stream_event(
                    ProxyProtocol::OpenAiResponses,
                    ProxyProtocol::OpenAiResponses,
                    event
                )
                .unwrap(),
            vec![event]
        );
    }

    #[test]
    fn unimplemented_pairs_are_explicitly_unavailable() {
        assert!(matches!(
            BuiltinConversionPlugin.convert_request(
                ProxyProtocol::OpenAiChatCompletions,
                ProxyProtocol::OpenAiResponses,
                serde_json::json!({})
            ),
            Err(ConversionError::Unsupported(_, _))
        ));
        assert!(matches!(
            BuiltinConversionPlugin.convert_stream_event(
                ProxyProtocol::AnthropicMessages,
                ProxyProtocol::OpenAiChatCompletions,
                "data: {}\n\n"
            ),
            Err(ConversionError::Unsupported(_, _))
        ));
    }

    #[test]
    fn supports_covers_same_protocol_and_anthropic_openai_pairs() {
        use ProxyProtocol::{
            AnthropicMessages as AM, OpenAiChatCompletions as CC, OpenAiResponses as RS,
        };
        for protocol in [AM, CC, RS] {
            assert!(supports(protocol, protocol));
            assert!(BuiltinConversionPlugin::supports(protocol, protocol));
        }
        for (from, to) in [(AM, CC), (CC, AM), (AM, RS), (RS, AM)] {
            assert!(supports(from, to), "{from:?} -> {to:?}");
        }
        for (from, to) in [(CC, RS), (RS, CC)] {
            assert!(!supports(from, to), "{from:?} -> {to:?}");
        }
    }

    #[test]
    fn usage_normalizes_openai_cache_and_anthropic_input() {
        let openai = BuiltinConversionPlugin.extract_usage(
            ProxyProtocol::OpenAiResponses,
            &serde_json::json!({"usage": {"input_tokens": 100, "output_tokens": 4, "input_tokens_details": {"cached_tokens": 60, "cache_creation_tokens": 10}}}),
        );
        assert_eq!(openai.input_tokens, 30);
        assert_eq!(openai.cache_read_tokens, 60);
        let anthropic = BuiltinConversionPlugin.extract_usage(
            ProxyProtocol::AnthropicMessages,
            &serde_json::json!({"usage": {"input_tokens": 30, "output_tokens": 4, "cache_read_input_tokens": 60, "cache_creation_input_tokens": 10}}),
        );
        assert_eq!(anthropic.input_tokens, 30);
        assert_eq!(anthropic.cache_read_tokens, 60);
    }
}
