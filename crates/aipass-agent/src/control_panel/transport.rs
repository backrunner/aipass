use super::{
    api,
    sessions::{cookie_name, Sessions},
};
use crate::session::{AgentState, ServiceError};
use aipass_agent_protocol::{AgentErrorCode, SensitiveString};
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::{body::Incoming, header, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use rustls::ServerConfig;
use serde::Deserialize;
use serde_json::json;
use std::{
    convert::Infallible,
    net::{IpAddr, SocketAddr, TcpListener},
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::{
    runtime::Runtime,
    sync::{watch, Semaphore},
};

type Body = Full<Bytes>;

pub(super) fn authority(address: SocketAddr, https: bool) -> String {
    if address.port() == if https { 443 } else { 80 } {
        match address.ip() {
            IpAddr::V4(ip) => ip.to_string(),
            IpAddr::V6(ip) => format!("[{ip}]"),
        }
    } else {
        address.to_string()
    }
}

pub(super) struct Listener {
    pub address: SocketAddr,
    pub socket: TcpListener,
    stop: watch::Sender<bool>,
    pub sessions: Arc<Sessions>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.sessions.revoke_all();
        let _ = self.stop.send(true);
        // The accept loop owns another socket handle. Wait for it to close before
        // a local stop/re-enable can bind the same address. Runtime shutdown below
        // remains bounded even if a blocking request is waiting on the panel lock.
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub(super) struct Prepared {
    socket: TcpListener,
    accept_socket: tokio::net::TcpListener,
    address: SocketAddr,
    config: Option<Arc<ServerConfig>>,
    runtime: Runtime,
}

impl Listener {
    pub fn prepare(
        address: SocketAddr,
        config: Option<Arc<ServerConfig>>,
        socket: Option<TcpListener>,
    ) -> anyhow::Result<Prepared> {
        let socket = socket
            .map(Ok)
            .unwrap_or_else(|| TcpListener::bind(address))?;
        socket.set_nonblocking(true)?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .max_blocking_threads(8)
            .enable_all()
            .build()?;
        let address = socket.local_addr()?;
        let accept_socket = {
            let _entered = runtime.enter();
            tokio::net::TcpListener::from_std(socket.try_clone()?)?
        };
        Ok(Prepared {
            socket,
            accept_socket,
            address,
            config,
            runtime,
        })
    }

    pub fn start(
        state: &Arc<AgentState>,
        address: SocketAddr,
        config: Option<Arc<ServerConfig>>,
        socket: Option<TcpListener>,
    ) -> anyhow::Result<Self> {
        Ok(Self::prepare(address, config, socket)?.launch(state))
    }
}

impl Prepared {
    pub fn launch(self, state: &Arc<AgentState>) -> Listener {
        let (stop, mut stopped) = watch::channel(false);
        let sessions = Arc::new(Sessions::default());
        let address = self.address;
        let mut listener = Listener {
            address,
            socket: self.socket,
            stop,
            sessions: sessions.clone(),
            worker: None,
        };
        let state = Arc::downgrade(state);
        listener.worker = Some(std::thread::spawn(move || {
            self.runtime.block_on(async move {
                let socket = self.accept_socket;
                let https = self.config.is_some();
                let acceptor = self.config.map(tokio_rustls::TlsAcceptor::from);
                let connections = Arc::new(Semaphore::new(32));
                let operations = Arc::new(Semaphore::new(8));
                loop {
                    let accepted = tokio::select! {
                        biased;
                        _ = stopped.changed() => break,
                        accepted = socket.accept() => accepted,
                    };
                    let Ok((stream, peer)) = accepted else {
                        break;
                    };
                    let Ok(permit) = connections.clone().try_acquire_owned() else {
                        continue;
                    };
                    let acceptor = acceptor.clone();
                    let sessions = sessions.clone();
                    let state = state.clone();
                    let mut stopped = stopped.clone();
                    let operations = operations.clone();
                    tokio::spawn(async move {
                        let _permit = permit;
                        let connection = async {
                            trait StreamIo:
                                tokio::io::AsyncRead
                                + tokio::io::AsyncWrite
                                + Unpin
                                + Send
                            {
                            }
                            impl<
                                    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
                                > StreamIo for T
                            {
                            }
                            let stream: Box<dyn StreamIo> = if let Some(acceptor) = acceptor {
                                let Ok(Ok(stream)) = tokio::time::timeout(
                                    Duration::from_secs(5),
                                    acceptor.accept(stream),
                                )
                                .await
                                else {
                                    return;
                                };
                                Box::new(stream)
                            } else {
                                Box::new(stream)
                            };
                            let service = hyper::service::service_fn(move |request| {
                                serve(
                                    request,
                                    peer.ip(),
                                    address,
                                    https,
                                    state.clone(),
                                    sessions.clone(),
                                    operations.clone(),
                                )
                            });
                            let _ = hyper::server::conn::http1::Builder::new()
                                .timer(TokioTimer::new())
                                .header_read_timeout(Duration::from_secs(10))
                                .max_headers(32)
                                .keep_alive(false)
                                .serve_connection(TokioIo::new(stream), service)
                                .await;
                        };
                        tokio::select! {
                            biased;
                            _ = stopped.changed() => {},
                            _ = tokio::time::timeout(Duration::from_secs(120), connection) => {},
                        }
                    });
                }
            });
            self.runtime.shutdown_timeout(Duration::from_secs(1));
        }));
        listener
    }
}

async fn serve(
    request: Request<Incoming>,
    peer: IpAddr,
    address: SocketAddr,
    https: bool,
    state: Weak<AgentState>,
    sessions: Arc<Sessions>,
    operations: Arc<Semaphore>,
) -> Result<Response<Body>, Infallible> {
    let response = serve_inner(request, peer, address, https, state, sessions, operations).await;
    Ok(response)
}

async fn serve_inner(
    request: Request<Incoming>,
    peer: IpAddr,
    address: SocketAddr,
    https: bool,
    state: Weak<AgentState>,
    sessions: Arc<Sessions>,
    operations: Arc<Semaphore>,
) -> Response<Body> {
    let authority = authority(address, https);
    let headers = request.headers();
    if headers.get(header::HOST).and_then(|h| h.to_str().ok()) != Some(authority.as_str()) {
        return error(StatusCode::FORBIDDEN, "Invalid host.");
    }
    let path = request.uri().path().to_owned();
    let method = request.method().clone();
    if request.uri().query().is_some() {
        return error(StatusCode::BAD_REQUEST, "Unexpected query.");
    }
    if method == hyper::Method::GET && !path.starts_with("/api/") {
        return match path.as_str() {
            "/" => response(
                StatusCode::OK,
                "text/html; charset=utf-8",
                include_bytes!("../../../../apps/control-panel/embedded/index.html").as_slice(),
            ),
            "/panel.js" => response(
                StatusCode::OK,
                "text/javascript; charset=utf-8",
                include_bytes!("../../../../apps/control-panel/embedded/panel.js").as_slice(),
            ),
            "/panel.css" => response(
                StatusCode::OK,
                "text/css; charset=utf-8",
                include_bytes!("../../../../apps/control-panel/embedded/panel.css").as_slice(),
            ),
            _ => error(StatusCode::NOT_FOUND, "Not found."),
        };
    }
    if headers.get("x-aipass-panel").and_then(|h| h.to_str().ok()) != Some("1") {
        return error(StatusCode::FORBIDDEN, "Invalid request.");
    }
    let cookie = cookie_name(https, address.port());
    let secure = if https { "; Secure" } else { "" };
    let origin = format!("{}://{authority}", if https { "https" } else { "http" });
    let request_origin = headers.get(header::ORIGIN).and_then(|h| h.to_str().ok());
    if request_origin.is_some_and(|value| value != origin)
        || (method != hyper::Method::GET && request_origin != Some(origin.as_str()))
        || headers
            .get("sec-fetch-site")
            .is_some_and(|value| value != "same-origin" && value != "none")
    {
        return error(StatusCode::FORBIDDEN, "Invalid origin.");
    }
    let csrf = headers
        .get("x-aipass-csrf")
        .and_then(|h| h.to_str().ok())
        .map(str::to_owned);
    let token = headers
        .get(header::COOKIE)
        .and_then(|h| h.to_str().ok())
        .and_then(|cookies| {
            cookies
                .split(';')
                .find_map(|part| part.trim().strip_prefix(&format!("{cookie}=")))
        })
        .filter(|token| token.len() == 43)
        .map(SensitiveString::from);
    if method != hyper::Method::GET
        && headers
            .get(header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            != Some("application/json")
    {
        return error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Expected application/json.",
        );
    }
    let Ok(permit) = operations.try_acquire_owned() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "Try again shortly.");
    };
    let body = match tokio::time::timeout(
        Duration::from_secs(10),
        Limited::new(request.into_body(), 64 * 1024).collect(),
    )
    .await
    {
        Ok(Ok(body)) => zeroize::Zeroizing::new(body.to_bytes().to_vec()),
        _ => return error(StatusCode::BAD_REQUEST, "Request too large or incomplete."),
    };
    let Some(state) = state.upgrade() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "Agent unavailable.");
    };
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        if crate::session::lock_if_idle(&state).is_err() {
            return error(StatusCode::SERVICE_UNAVAILABLE, "Agent unavailable.");
        }
        if method == hyper::Method::POST && path == "/api/login" {
            let Some(_login_permit) = sessions.login_permit(peer) else {
                let mut response = error(
                    StatusCode::TOO_MANY_REQUESTS,
                    "Too many login attempts. Wait one minute.",
                );
                response
                    .headers_mut()
                    .insert(header::RETRY_AFTER, "60".parse().unwrap());
                return response;
            };
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            #[serde(rename_all = "camelCase")]
            struct Login {
                access_code: SensitiveString,
            }
            let Ok(login) = serde_json::from_slice::<Login>(&body) else {
                return error(StatusCode::BAD_REQUEST, "Invalid login request.");
            };
            let generation = sessions.generation();
            return match super::ControlPanel::login(&state, &sessions, login.access_code.expose(), generation) {
                Err(err) if err.code == AgentErrorCode::Locked => error(
                    StatusCode::LOCKED,
                    "This code cannot unlock the vault. On the host, generate a new code with remote unlock enabled.",
                ),
                Err(err) if err.code == AgentErrorCode::PermissionDenied => error(
                    StatusCode::UNAUTHORIZED,
                    "Access code is invalid or revoked. Generate a new code on the host computer.",
                ),
                Ok((token, csrf)) => {
                    let mut reply = json_response(json!({ "csrf": csrf }));
                    reply.headers_mut().insert(
                        header::SET_COOKIE,
                        format!(
                            "{cookie}={}; Path=/; HttpOnly{secure}; SameSite=Strict",
                            token.expose()
                        )
                        .parse()
                        .unwrap(),
                    );
                    reply
                }
                Err(err) => service_error(err),
            };
        }
        let Some(token) = token else {
            return error(StatusCode::UNAUTHORIZED, "Sign in to continue.");
        };
        if method != hyper::Method::GET && csrf.is_none() {
            return error(StatusCode::FORBIDDEN, "Missing CSRF token.");
        }
        let result = sessions.authorized(
            token.expose(),
            if method == hyper::Method::GET {
                None
            } else {
                csrf.as_deref()
            },
            || {
                super::sessions::validate(&state)?;
                match (method.as_str(), path.as_str()) {
                    ("GET", "/api/state") => {
                        let mut data = api::snapshot(&state)?;
                        data["csrf"] = json!(sessions.csrf(token.expose())?);
                        Ok(data)
                    }
                    ("POST", "/api/logout") => {
                        sessions.logout(token.expose());
                        Ok(json!({"ok":true}))
                    }
                    ("POST", "/api/action") => {
                        let action = serde_json::from_slice(&body)
                            .map_err(|_| super::invalid("Invalid action."))?;
                        api::action(&state, &sessions, token.expose(), action)
                    }
                    _ => Err(ServiceError::new(AgentErrorCode::NotFound, "Not found.")),
                }
            },
        );
        match result {
            Ok(value) => {
                let mut reply = json_response(value);
                if path == "/api/logout" {
                    reply.headers_mut().insert(
                        header::SET_COOKIE,
                        format!("{cookie}=; Path=/; HttpOnly{secure}; SameSite=Strict; Max-Age=0")
                            .parse()
                            .unwrap(),
                    );
                }
                reply
            }
            Err(err) => service_error(err),
        }
    })
    .await;
    result.unwrap_or_else(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Operation failed."))
}

fn service_error(error: ServiceError) -> Response<Body> {
    match error.code {
        AgentErrorCode::Locked
        | AgentErrorCode::PermissionDenied
        | AgentErrorCode::InvalidPassword => {
            error_response(StatusCode::UNAUTHORIZED, "Sign in again.")
        }
        AgentErrorCode::ValidationFailed => error_response(
            StatusCode::BAD_REQUEST,
            "Invalid configuration or expired preview. Check the selection and preview again.",
        ),
        AgentErrorCode::NotFound => error_response(
            StatusCode::NOT_FOUND,
            "Item no longer exists. Refresh the panel.",
        ),
        AgentErrorCode::Conflict => error_response(
            StatusCode::CONFLICT,
            "Configuration changed. Preview or refresh again.",
        ),
        _ => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Operation failed. Check the Agent logs on this computer.",
        ),
    }
}

fn json_response(value: serde_json::Value) -> Response<Body> {
    response(
        StatusCode::OK,
        "application/json",
        serde_json::to_vec(&value).unwrap_or_default(),
    )
}
fn error(status: StatusCode, message: &str) -> Response<Body> {
    error_response(status, message)
}
fn error_response(status: StatusCode, message: &str) -> Response<Body> {
    response(
        status,
        "application/json",
        serde_json::to_vec(&json!({"error":message})).unwrap(),
    )
}
fn response(status: StatusCode, content_type: &str, body: impl Into<Bytes>) -> Response<Body> {
    Response::builder().status(status)
        .header(header::CONTENT_TYPE, content_type).header(header::CACHE_CONTROL, "no-store")
        .header("content-security-policy", "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; base-uri 'none'; form-action 'self'; frame-ancestors 'none'")
        .header("x-content-type-options", "nosniff").header("referrer-policy", "no-referrer")
        .header("x-frame-options", "DENY").header("permissions-policy", "camera=(), microphone=(), geolocation=()")
        .body(Full::new(body.into())).expect("static response headers")
}
