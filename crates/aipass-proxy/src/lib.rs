//! Local proxy: routes inbound traffic to upstream targets with failover,
//! usage accounting, and cross-protocol conversion.
//!
//! Module layout:
//! - `config`: route/target/retry/proxy configuration types
//! - `state`: runtime state, resolved routes, health, errors
//! - `runtime`: `ProxyHandle` lifecycle and accept loop
//! - `selection`: route matching, target ordering, affinity, circuit breaking
//! - `upstream`: upstream clients, outbound proxy rules, headers, URLs
//! - `forward`: request body handling and the forwarding pipeline
//! - `endpoints`: locally-served endpoints (models, health)
//! - `sse`: SSE parsing, prefetching, conversion, usage tracking
//! - `usage`: SQLite usage store and record types
//! - `util`: small shared helpers

mod concurrency;
mod config;
mod diagnostics;
mod endpoints;
mod forward;
mod images;
mod routing;
mod runtime;
mod selection;
mod shell_env;
mod sse;
mod state;
mod upstream;
mod usage;
mod util;
mod websocket;

pub use aipass_proxy_conversion::{supports, ConversionError, ProxyProtocol as Protocol};
pub use config::*;
pub use runtime::*;
pub use state::*;
pub use upstream::*;
pub use usage::*;
pub use websocket::capability::{
    websocket_config_key, Observation as WebsocketObservation, WebsocketCapabilityEvent,
};
pub use websocket::{probe_websocket, WebsocketProbeResult};

pub(crate) use aipass_proxy_conversion::{
    BuiltinConversionPlugin, ConversionPlugin, ProxyProtocol, StreamConverter, TokenUsage,
};
pub(crate) use bytes::Bytes;
pub(crate) use concurrency::{capacity_response, ProviderAtCapacity, ProviderPermit};
pub(crate) use endpoints::*;
pub(crate) use forward::*;
pub(crate) use futures_util::{stream, Stream, StreamExt};
pub(crate) use http::{header, HeaderMap, HeaderValue, Request, Response, StatusCode};
pub(crate) use http_body_util::{BodyExt, Full, StreamBody};
pub(crate) use hyper::body::{Frame, Incoming};
pub(crate) use hyper::service::service_fn;
pub(crate) use hyper_util::rt::TokioIo;
pub(crate) use routing::*;
pub(crate) use rusqlite::{params, Connection};
pub(crate) use selection::*;
pub(crate) use serde::{Deserialize, Serialize};
pub(crate) use sse::*;
pub(crate) use std::collections::{HashMap, HashSet, VecDeque};
pub(crate) use std::convert::Infallible;
pub(crate) use std::error::Error as StdError;
pub(crate) use std::net::SocketAddr;
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::pin::Pin;
pub(crate) use std::sync::atomic::{AtomicU64, Ordering};
pub(crate) use std::sync::{Arc, Mutex, RwLock};
pub(crate) use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
pub(crate) use thiserror::Error;
pub(crate) use tokio::io::AsyncWriteExt;
pub(crate) use tokio::net::TcpListener;
pub(crate) use tokio::sync::oneshot;
pub(crate) use util::*;
pub(crate) use uuid::Uuid;
pub(crate) use zeroize::Zeroize;

#[cfg(test)]
mod tests;
