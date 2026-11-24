use super::*;
use http_body_util::{BodyExt, Full};
use hyper::{
    body::{Body, Incoming},
    http::request::Parts,
    Request, Response,
};
use hyper_util::rt::TokioIo;
use std::sync::Mutex;
use tokio::net::{TcpListener, TcpStream};

fn account(root: &std::path::Path) -> (Context, mpsc::Sender<Value>, mpsc::Receiver<Value>) {
    std::fs::write(root.join("auth.json"),json!({"https://auth.x.ai":{"key":"new","email":"alice","expires_at":"2040-01-01T00:00:00Z"}}).to_string()).unwrap();
    let (mut c, tx, rx) =
        super::super::tests::context(Client::builder().no_proxy().build().unwrap());
    c.provider = "grok".into();
    c.auth = cli_accounts::reference("grok", root).unwrap();
    c.native_token = Some(aipass_agent_protocol::SensitiveString::new("old"));
    (c, tx, rx)
}

struct Fixture {
    url: String,
    requests: mpsc::UnboundedReceiver<Parts>,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl Fixture {
    async fn observed(mut self) -> Vec<Parts> {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(100);
        let mut requests = Vec::new();
        while let Ok(next) = tokio::time::timeout_at(deadline, self.requests.recv()).await {
            requests.push(next.expect("Grok fixture stopped before observation completed"));
        }
        requests
    }
}

async fn reply(
    request: Request<Incoming>,
    statuses: Arc<Mutex<std::vec::IntoIter<u16>>>,
    requests: mpsc::UnboundedSender<Parts>,
) -> std::result::Result<Response<Full<bytes::Bytes>>, hyper::Error> {
    let (parts, body) = request.into_parts();
    let probe = matches!(parts.method, hyper::Method::GET | hyper::Method::HEAD)
        && parts.uri == "/"
        && body.is_end_stream()
        && !["authorization", "x-api-key", "x-xai-token-auth", "upgrade"]
            .iter()
            .any(|name| parts.headers.contains_key(*name));
    let (status, text) = if probe {
        (204, "")
    } else {
        let status = statuses.lock().unwrap().next().unwrap_or(500);
        let _ = requests.send(parts);
        (
            status,
            if status == 200 {
                r#"{"ok":true}"#
            } else {
                r#"{"error":"unauthorized"}"#
            },
        )
    };
    body.collect().await?;
    Ok(Response::builder()
        .status(status)
        .body(Full::new(bytes::Bytes::from_static(text.as_bytes())))
        .unwrap())
}

async fn server(statuses: Vec<u16>) -> Fixture {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/fixture", listener.local_addr().unwrap());
    let statuses = Arc::new(Mutex::new(statuses.into_iter()));
    let (sender, requests) = mpsc::unbounded_channel();
    let server = tokio::spawn(async move {
        let mut connections = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                connection = listener.accept() => {
                    let (socket, _) = connection.unwrap();
                    let replies = statuses.clone();
                    let capture = sender.clone();
                    connections.spawn(async move {
                        let service = hyper::service::service_fn(move |request| {
                            reply(request, replies.clone(), capture.clone())
                        });
                        // A connect-only probe can close before the HTTP parser sees a request.
                        let connection = hyper::server::conn::http1::Builder::new()
                            .keep_alive(false)
                            .serve_connection(TokioIo::new(socket), service);
                        let _ = tokio::time::timeout(Duration::from_secs(3), connection).await;
                    });
                }
                Some(result) = connections.join_next(), if !connections.is_empty() => result.unwrap(),
            }
        }
    });
    Fixture {
        url,
        requests,
        server,
    }
}

#[tokio::test]
async fn only_discovery_traffic_is_excluded_from_grok_request_counts() {
    let client = Client::builder().no_proxy().build().unwrap();
    let fixture = server(vec![401]).await;
    let base = fixture.url.strip_suffix("/fixture").unwrap();
    let address = base.strip_prefix("http://").unwrap();
    drop(TcpStream::connect(address).await.unwrap());
    let idle_probe = TcpStream::connect(address).await.unwrap();
    assert_eq!(client.get(base).send().await.unwrap().status(), 204);
    assert_eq!(client.get(&fixture.url).send().await.unwrap().status(), 401);
    assert_eq!(client.head(base).send().await.unwrap().status(), 204);
    assert_eq!(
        client
            .post(&fixture.url)
            .body("generation")
            .send()
            .await
            .unwrap()
            .status(),
        500
    );
    assert_eq!(
        client
            .get(base)
            .bearer_auth("fixture")
            .send()
            .await
            .unwrap()
            .status(),
        500
    );
    let observed = fixture.observed().await;
    drop(idle_probe);
    assert_eq!(observed.len(), 3);
    assert_eq!(observed[0].uri, "/fixture");
    assert_eq!(observed[1].method, hyper::Method::POST);
    assert_eq!(observed[2].headers["authorization"], "Bearer fixture");
}

#[tokio::test]
async fn read_only_401_retries_once_and_other_failures_do_not_refresh() {
    for statuses in [
        vec![401, 200],
        vec![401, 401],
        vec![403],
        vec![429],
        vec![500],
    ] {
        let root = tempfile::tempdir().unwrap();
        let (mut c, _tx, _rx) = account(root.path());
        let fixture = server(statuses.clone()).await;
        let result = get(&mut c, &fixture.url).await;
        assert_eq!(result.is_ok(), statuses.last() == Some(&200));
        let requests = fixture.observed().await;
        assert_eq!(
            requests.len(),
            statuses.len(),
            "unexpected extra Grok request"
        );
        assert!(requests
            .iter()
            .all(|request| request.method == hyper::Method::GET && request.uri == "/fixture"));
        assert_eq!(requests[0].headers["authorization"], "Bearer old");
        if requests.len() == 2 {
            assert_eq!(requests[1].headers["authorization"], "Bearer new");
        } else {
            assert_eq!(c.token().unwrap(), "old");
        }
    }
}

#[tokio::test]
async fn generation_401_recovers_future_access_without_replaying() {
    let root = tempfile::tempdir().unwrap();
    let (mut c, tx, mut rx) = account(root.path());
    let fixture = server(vec![401]).await;
    let response = send(
        sign(&c, c.client.post(&fixture.url))
            .unwrap()
            .body("fixture"),
    )
    .await
    .unwrap();
    let peer = tokio::spawn(async move {
        let head = rx.recv().await.unwrap();
        assert_eq!(head["value"]["status"], 401);
        let data = rx.recv().await.unwrap();
        assert_eq!(data["type"], "data");
        tx.send(json!({"type":"ack","id":data["id"],"ok":true}))
            .await
            .unwrap();
        assert_eq!(rx.recv().await.unwrap()["type"], "end");
    });
    c.pipe(response).await.unwrap();
    peer.await.unwrap();
    let observed = fixture.observed().await;
    assert_eq!(observed.len(), 1, "unexpected replay of Grok generation");
    assert_eq!(observed[0].method, hyper::Method::POST);
    assert_eq!(observed[0].headers["authorization"], "Bearer old");
    assert_eq!(c.token().unwrap(), "new");
    assert!(c.feedback.revoked.is_none());
}
