use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

const ROOT: &str = "AIPASS_GROK_PROCESS_FIXTURE_ROOT";
const URL: &str = "AIPASS_GROK_PROCESS_FIXTURE_URL";
const TEST: &str = "subscriptions::cli_accounts::grok::process_tests::";

#[tokio::test]
async fn single_use_cli() {
    let Some(root) = std::env::var_os(ROOT) else {
        return;
    };
    let body = Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(std::env::var(URL).unwrap())
        .body("single-use-grant")
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    let root = PathBuf::from(root);
    std::fs::write(root.join("next.json"), body).unwrap();
    std::fs::rename(root.join("next.json"), root.join("auth.json")).unwrap();
}

#[tokio::test]
async fn renewal_child() {
    let Some(root) = std::env::var_os(ROOT) else {
        return;
    };
    let root = PathBuf::from(root);
    let mut c = super::tests::context(&root);
    let id = std::env::var("AIPASS_GROK_PROCESS_FIXTURE_ID").unwrap();
    std::fs::write(root.join(format!("ready-{id}")), b"ready").unwrap();
    while !root.join("start").exists() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    renew_with(&mut c, true, Some("old"), |c, path| {
        let mut cmd = native_cli::command(Path::new("/bin/sh"), c);
        cmd.current_dir(path)
            .args(["-c", "exec \"$AIPASS_GROK_FIXTURE_EXE\" --exact \"$AIPASS_GROK_FIXTURE_TEST\" --nocapture"])
            .env("AIPASS_GROK_FIXTURE_EXE", std::env::current_exe().unwrap())
            .env("AIPASS_GROK_FIXTURE_TEST", format!("{TEST}single_use_cli"))
            .env(ROOT, path).env(URL, std::env::var(URL).unwrap());
        Ok(cmd)
    }).await.unwrap();
    assert_eq!(c.token().unwrap(), "new");
}

#[tokio::test]
async fn two_processes_rotate_one_source_once_and_both_use_the_new_token() {
    let root = tempfile::tempdir().unwrap();
    super::tests::write(root.path(), "alice", "old");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    let server = tokio::spawn(async move {
        'accept: loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut data = Vec::new();
            loop {
                let mut chunk = [0; 1024];
                let n = socket.read(&mut chunk).await.unwrap();
                if n == 0 && data.is_empty() {
                    continue 'accept;
                }
                assert!(n > 0 && data.len() < 8192);
                data.extend_from_slice(&chunk[..n]);
                if data.windows(4).any(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&data).to_ascii_lowercase();
                    if (headers.starts_with("get / http/1.")
                        || headers.starts_with("head / http/1."))
                        && !headers.contains("\r\nauthorization:")
                    {
                        socket.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
                        continue 'accept;
                    }
                    assert!(
                        headers.starts_with("post / http/1."),
                        "unexpected grant request"
                    );
                }
                if data.ends_with(b"single-use-grant") {
                    break;
                }
            }
            let first = calls.fetch_add(1, Ordering::SeqCst) == 0;
            tokio::time::sleep(Duration::from_millis(150)).await;
            let status = if first { "200 OK" } else { "400 Bad Request" };
            let body = if first {
                json!({"https://auth.x.ai":{"email":"alice","key":"new","expires_at":"2040-01-01T00:00:00Z"}})
            } else { json!({"error":"invalid_grant"}) }.to_string();
            socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        }
    });
    // Local listener discovery must not spend the fixture's single-use grant.
    drop(
        tokio::net::TcpStream::connect(url.trim_start_matches("http://"))
            .await
            .unwrap(),
    );
    assert_eq!(
        Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(&url)
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::NO_CONTENT
    );
    assert_eq!(count.load(Ordering::SeqCst), 0);
    let mut children = Vec::new();
    for id in ["a", "b"] {
        children.push(
            tokio::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", &format!("{TEST}renewal_child"), "--nocapture"])
                .env(ROOT, root.path())
                .env(URL, &url)
                .env("AIPASS_GROK_PROCESS_FIXTURE_ID", id)
                .kill_on_drop(true)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap(),
        );
    }
    tokio::time::timeout(Duration::from_secs(15), async {
        while !root.path().join("ready-a").exists() || !root.path().join("ready-b").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    std::fs::write(root.path().join("start"), b"start").unwrap();
    for child in children {
        let output = tokio::time::timeout(Duration::from_secs(15), child.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    server.abort();
    assert_eq!(count.load(Ordering::SeqCst), 1);
}
