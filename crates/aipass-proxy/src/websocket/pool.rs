//! Idle upstream sockets belong to one downstream connection. Requests lease
//! sockets exclusively, so concurrent responses cannot consume each other's events.
use super::*;
use keepalive::Heartbeat;
use sha2::{Digest, Sha256};

pub(super) type Socket = WebSocketStream<reqwest::Upgraded>;
pub(super) struct Connection {
    pub id: Uuid,
    pub socket: Socket,
}
pub(super) type Key = [u8; 32];
const MAX_IDLE: usize = 32;
const IDLE_TIMEOUT: Duration = Duration::from_secs(120);

pub(crate) struct Pool {
    idle: Mutex<Vec<(Key, Idle)>>,
    closed: tokio::sync::watch::Sender<bool>,
}

impl Default for Pool {
    fn default() -> Self {
        Self {
            idle: Mutex::new(Vec::new()),
            closed: tokio::sync::watch::channel(false).0,
        }
    }
}

struct Idle {
    take: oneshot::Sender<oneshot::Sender<Connection>>,
    done: oneshot::Receiver<()>,
}

/// Hash effective handshake metadata instead of retaining credentials in the pool.
pub(super) fn key(
    route_id: Uuid,
    target: &ResolvedTarget,
    incoming: &HeaderMap,
    query: Option<&str>,
) -> Option<Key> {
    let headers = upstream_headers(incoming, target).ok()?;
    let mut entries: Vec<_> = headers.iter().collect();
    entries.sort_by(|(a, _), (b, _)| a.as_str().cmp(b.as_str()));
    let mut hash = Sha256::new();
    for bytes in [
        route_id.as_bytes().as_slice(),
        target.config.id.as_bytes().as_slice(),
        target.config.base_url.as_bytes(),
        query.unwrap_or_default().as_bytes(),
        // These headers are local metadata, but still define session isolation.
        incoming
            .get("x-aipass-session-id")
            .map_or(&[][..], HeaderValue::as_bytes),
        incoming
            .get("x-aipass-session")
            .map_or(&[][..], HeaderValue::as_bytes),
    ] {
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    for (name, value) in entries {
        for bytes in [name.as_str().as_bytes(), value.as_bytes()] {
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(bytes);
        }
    }
    Some(hash.finalize().into())
}

impl Pool {
    pub(super) fn close(&self) {
        self.closed.send_replace(true);
        if let Ok(mut idle) = self.idle.lock() {
            idle.clear();
        }
    }

    pub(super) async fn take(&self, key: Key) -> Option<Connection> {
        if *self.closed.borrow() {
            return None;
        }
        loop {
            let idle = {
                let mut pool = self.idle.lock().ok()?;
                pool.retain_mut(|(_, idle)| {
                    matches!(
                        idle.done.try_recv(),
                        Err(oneshot::error::TryRecvError::Empty)
                    )
                });
                let index = pool.iter().position(|(candidate, _)| *candidate == key)?;
                pool.swap_remove(index).1
            };
            let (tx, rx) = oneshot::channel();
            if idle.take.send(tx).is_ok() {
                if let Ok(socket) = rx.await {
                    return Some(socket);
                }
            }
        }
    }

    pub(super) fn put(
        &self,
        key: Key,
        mut connection: Connection,
        mut config_changed: ConfigWatch,
    ) {
        let Ok(mut pool) = self.idle.lock() else {
            return;
        };
        pool.retain_mut(|(_, idle)| {
            matches!(
                idle.done.try_recv(),
                Err(oneshot::error::TryRecvError::Empty)
            )
        });
        if pool.len() >= MAX_IDLE
            || *self.closed.borrow()
            || config_changed.has_changed().unwrap_or(true)
        {
            return;
        }
        let (take, mut requested) = oneshot::channel::<oneshot::Sender<Connection>>();
        let (finished, done) = oneshot::channel();
        pool.push((key, Idle { take, done }));
        let mut closed = self.closed.subscribe();
        tokio::spawn(async move {
            let _finished = finished;
            let expires = tokio::time::Instant::now() + IDLE_TIMEOUT;
            let mut heartbeat = Heartbeat::new();
            loop {
                tokio::select! {
                    biased;
                    _ = closed.changed() => break,
                    _ = config_changed.changed() => break,
                    _ = tokio::time::sleep_until(expires) => break,
                    request = &mut requested => {
                        if let Ok(reply) = request {
                            let valid = tokio::select! {
                                biased;
                                _ = closed.changed() => false,
                                _ = config_changed.changed() => false,
                                valid = Heartbeat::validate(&mut connection.socket) => valid.is_ok(),
                            };
                            if valid { let _ = reply.send(connection); return; }
                        }
                        break;
                    }
                    _ = tokio::time::sleep_until(heartbeat.deadline()) => {
                        let result = tokio::select! {
                            biased;
                            _ = closed.changed() => break,
                            _ = config_changed.changed() => break,
                            result = heartbeat.ping(&mut connection.socket) => result,
                        };
                        if result.is_err() { break; }
                    }
                    message = connection.socket.next() => match message {
                        Some(Ok(Message::Ping(_))) => {
                            if tokio::time::timeout(Duration::from_secs(1), connection.socket.flush()).await.ok().and_then(Result::ok).is_none() { break; }
                        }
                        Some(Ok(Message::Pong(data))) => heartbeat.pong(&data),
                        _ => break,
                    }
                }
            }
            let _ =
                tokio::time::timeout(Duration::from_secs(1), connection.socket.close(None)).await;
        });
    }
}
