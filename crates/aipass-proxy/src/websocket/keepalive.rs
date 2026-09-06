//! Hop-local heartbeats; control frames never count as response progress.
use super::*;
use tokio::io::{AsyncRead, AsyncWrite};

pub(super) const INTERVAL: Duration = Duration::from_secs(20);
const PONG_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) struct Heartbeat {
    next: tokio::time::Instant,
    pending: Option<Bytes>,
}

impl Heartbeat {
    pub fn new() -> Self {
        Self {
            next: tokio::time::Instant::now() + INTERVAL,
            pending: None,
        }
    }

    pub fn deadline(&self) -> tokio::time::Instant {
        self.next
    }

    pub fn pong(&mut self, data: &Bytes) {
        if self.pending.as_ref() == Some(data) {
            self.pending = None;
            self.next = tokio::time::Instant::now() + INTERVAL;
        }
    }

    pub async fn ping<S>(&mut self, socket: &mut WebSocketStream<S>) -> Result<(), ()>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        if self.pending.is_some() {
            return Err(());
        }
        let data = Bytes::copy_from_slice(Uuid::new_v4().as_bytes());
        self.pending = Some(data.clone());
        self.next = tokio::time::Instant::now() + PONG_TIMEOUT;
        tokio::time::timeout(PONG_TIMEOUT, socket.send(Message::Ping(data)))
            .await
            .map_err(|_| ())?
            .map_err(|_| ())
    }

    /// Before reusing an idle socket, prove it is still alive without sending
    /// a generation. Any application event here makes the socket unsafe to use.
    pub async fn validate<S>(socket: &mut WebSocketStream<S>) -> Result<(), ()>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        tokio::time::timeout(PONG_TIMEOUT, async {
            let mut heartbeat = Self::new();
            heartbeat.ping(socket).await?;
            loop {
                match socket.next().await {
                    Some(Ok(Message::Pong(data))) => {
                        heartbeat.pong(&data);
                        if heartbeat.pending.is_none() {
                            return Ok(());
                        }
                    }
                    Some(Ok(Message::Ping(_))) => socket.flush().await.map_err(|_| ())?,
                    _ => return Err(()),
                }
            }
        })
        .await
        .map_err(|_| ())?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn heartbeat_requires_a_matching_pong_before_another_ping() {
        let (client, server) = tokio::io::duplex(1024);
        let mut client = WebSocketStream::from_raw_socket(client, Role::Client, None).await;
        let mut server = WebSocketStream::from_raw_socket(server, Role::Server, None).await;
        let mut heartbeat = Heartbeat::new();
        heartbeat.ping(&mut client).await.unwrap();
        let Message::Ping(data) = server.next().await.unwrap().unwrap() else {
            panic!("expected heartbeat ping");
        };
        heartbeat.pong(&Bytes::from_static(b"unrelated-pong"));
        assert!(heartbeat.ping(&mut client).await.is_err());
        heartbeat.pong(&data);
        assert!(heartbeat.ping(&mut client).await.is_ok());
    }

    #[tokio::test]
    async fn reuse_validation_rejects_unread_application_events() {
        let (client, server) = tokio::io::duplex(1024);
        let mut client = WebSocketStream::from_raw_socket(client, Role::Client, None).await;
        let mut server = WebSocketStream::from_raw_socket(server, Role::Server, None).await;
        server
            .send(Message::text(r#"{"type":"response.completed"}"#))
            .await
            .unwrap();
        assert!(Heartbeat::validate(&mut client).await.is_err());
    }
}
