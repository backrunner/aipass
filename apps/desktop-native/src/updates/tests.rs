use super::*;

const PAYLOAD: &[u8] = b"signed native updater regression payload";

pub(super) fn updater(dir: &std::path::Path) -> (Updater, Release) {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("test-signature.json")).unwrap();
    let config = Config {
        current_version: "0.2.0".parse().unwrap(),
        endpoint: "https://example.invalid/latest.json".parse().unwrap(),
        target: "darwin-native-aarch64".into(),
        public_key: fixture["publicKey"].as_str().unwrap().into(),
        cache_dir: dir.into(),
        app_path: dir.join("App.app"),
        identifier: "com.alkinum.aipass.gpui-validation".into(),
    };
    let release = Release {
        version: "0.2.1".parse().unwrap(),
        notes: None,
        url: "https://example.invalid/package".parse().unwrap(),
        signature: fixture["signature"].as_str().unwrap().into(),
    };
    (Updater::new(config).unwrap(), release)
}

fn cache(updater: &Updater, release: &Release, payload: &[u8]) {
    fs::write(updater.config.cache_dir.join("package"), payload).unwrap();
    fs::write(
        updater.config.cache_dir.join("metadata.json"),
        serde_json::to_vec(&Cached {
            endpoint: updater.config.endpoint.clone(),
            target: updater.config.target.clone(),
            release: release.clone(),
        })
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn signatures_are_required_on_download_and_cache_readback() {
    let dir = tempfile::tempdir().unwrap();
    let (updater, release) = updater(dir.path());
    updater.verify(&release, PAYLOAD).unwrap();
    assert!(updater.verify(&release, b"tampered").is_err());
    cache(&updater, &release, PAYLOAD);
    assert_eq!(updater.cached(&release).unwrap().unwrap(), PAYLOAD);
    fs::write(dir.path().join("package"), b"tampered after download").unwrap();
    assert!(updater.cached(&release).unwrap().is_none());
    assert!(!updater.has_pending());
}

#[test]
fn cache_is_bound_to_feed_target_and_exact_release() {
    let dir = tempfile::tempdir().unwrap();
    let (mut updater, release) = updater(dir.path());
    cache(&updater, &release, PAYLOAD);
    updater.config.target = "darwin-native-x86_64".into();
    assert!(updater.cached(&release).unwrap().is_none());
    cache(&updater, &release, PAYLOAD);
    updater.config.endpoint = "https://example.invalid/nightly.json".parse().unwrap();
    assert!(updater.cached(&release).unwrap().is_none());
    cache(&updater, &release, PAYLOAD);
    let mut changed = release.clone();
    changed.version = "0.2.2".parse().unwrap();
    assert!(updater.cached(&changed).unwrap().is_none());
}

#[test]
fn partial_and_malformed_cache_are_removed() {
    let dir = tempfile::tempdir().unwrap();
    let (updater, release) = updater(dir.path());
    fs::write(dir.path().join("package"), PAYLOAD).unwrap();
    assert!(updater.cached(&release).unwrap().is_none());
    assert!(!dir.path().join("package").exists());
    fs::write(dir.path().join("metadata.json"), b"broken").unwrap();
    assert!(updater.cached(&release).unwrap().is_none());
    assert!(!updater.has_pending());
}

#[test]
fn cache_cleanup_failure_blocks_installation() {
    let dir = tempfile::tempdir().unwrap();
    let (updater, _) = updater(dir.path());
    fs::create_dir(dir.path().join("package")).unwrap();
    assert!(updater.clear().is_err());
}

#[test]
fn schedule_recovers_corruption_future_clocks_and_persists_dismissal() {
    let dir = tempfile::tempdir().unwrap();
    let (updater, release) = updater(dir.path());
    assert_eq!(updater.initial_delay(100), Duration::from_secs(3));
    updater.checked(100).unwrap();
    assert_eq!(
        updater.initial_delay(101),
        INTERVAL - Duration::from_secs(1)
    );
    assert_eq!(updater.initial_delay(99), Duration::from_secs(3));
    assert_eq!(updater.initial_delay(100_000), Duration::ZERO);
    updater.dismiss(release.version.clone()).unwrap();
    assert_eq!(updater.schedule().dismissed, Some(release.version));
    fs::write(dir.path().join("schedule.json"), b"broken").unwrap();
    assert_eq!(updater.initial_delay(100), Duration::from_secs(3));
}

#[test]
fn production_urls_require_https_and_native_targets() {
    for url in [
        "http://example.com/update",
        "http://127.0.0.1/update",
        "file:///tmp/update",
        "https://user:password@example.com/update",
    ] {
        assert!(validate_url(&url.parse().unwrap(), false).is_err());
    }
    assert!(validate_url(&"http://127.0.0.1/update".parse().unwrap(), true).is_ok());
    assert!(validate_url(&"http://localhost/update".parse().unwrap(), true).is_err());
    let dir = tempfile::tempdir().unwrap();
    let (updater, _) = updater(dir.path());
    let mut config = updater.config.clone();
    config.target = "darwin-aarch64".into();
    assert!(Updater::new(config).is_err());
}

#[test]
fn version_family_selects_feed_and_never_downgrades() {
    for (version, expected) in [
        ("0.2.0", "official"),
        ("0.3.0-beta.1", "beta"),
        ("0.3.0-nightly.20261112.1", "nightly"),
    ] {
        assert_eq!(channel(&version.parse().unwrap()), expected);
    }
    assert_eq!(
        endpoint(&"0.2.0".parse().unwrap()),
        "https://github.com/backrunner/aipass/releases/latest/download/latest.json"
    );
}

#[test]
fn bounded_input_stops_oversized_responses() {
    assert!(bounded_read(&b"12345"[..], 4, |_| {}).is_err());
    assert_eq!(bounded_read(&b"1234"[..], 4, |_| {}).unwrap(), b"1234");
}

#[test]
fn feed_rejects_older_same_versions_and_tauri_targets() {
    use std::{io::Write, net::TcpListener};
    for (version, target, available) in [
        ("0.1.9", "darwin-native-aarch64", Some(false)),
        ("0.2.0", "darwin-native-aarch64", Some(false)),
        ("0.2.1", "darwin-aarch64", None),
        ("0.2.1", "darwin-native-aarch64", Some(true)),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (updater, release) = updater(dir.path());
        let body = serde_json::to_vec(&serde_json::json!({"version":version, "platforms":{target:{"url":release.url, "signature":release.signature}}})).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0; 4096];
            assert!(stream.read(&mut buffer).unwrap() > 0);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
        });
        let mut config = updater.config.clone();
        config.endpoint = format!("http://{address}/latest.json").parse().unwrap();
        let updater = Updater::validation(config).unwrap();
        assert_eq!(
            updater.check().map(|result| result.is_some()).ok(),
            available
        );
        server.join().unwrap();
    }
}
