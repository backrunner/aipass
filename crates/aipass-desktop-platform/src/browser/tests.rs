use super::*;
use uuid::Uuid;
#[test]
fn extension_package_metadata_reads_zip_field_with_default_fallback() {
    let metadata_dir = Path::new("/bundle/browser-extension");
    let with_zip =
        r#"{"id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "version": "1.2.3", "zip": "custom.zip"}"#;
    let package = parse_extension_package_metadata(with_zip, metadata_dir).unwrap();
    assert_eq!(package.id, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    assert_eq!(package.version, "1.2.3");
    assert_eq!(package.zip_path, metadata_dir.join("custom.zip"));

    let without_zip = r#"{"id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "version": "1.2.3"}"#;
    let package = parse_extension_package_metadata(without_zip, metadata_dir).unwrap();
    assert_eq!(package.zip_path, metadata_dir.join("aipass-extension.zip"));
    // Older metadata has no manifest_version; it falls back to version.
    assert_eq!(package.manifest_version, "1.2.3");
}

#[test]
fn extension_package_metadata_prefers_manifest_version() {
    let metadata_dir = Path::new("/bundle/browser-extension");
    let nightly = r#"{"id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "version": "0.3.0-nightly.20260905", "manifest_version": "0.3.0.1045"}"#;
    let package = parse_extension_package_metadata(nightly, metadata_dir).unwrap();
    assert_eq!(package.version, "0.3.0-nightly.20260905");
    assert_eq!(package.manifest_version, "0.3.0.1045");
}

#[test]
fn parse_chrome_version_accepts_only_browser_compatible_versions() {
    assert_eq!(parse_chrome_version("0.3.0.1045"), Some([0, 3, 0, 1045]));
    assert_eq!(parse_chrome_version("1.2"), Some([1, 2, 0, 0]));
    assert_eq!(parse_chrome_version("0.3.0-nightly.20260905"), None);
    assert_eq!(parse_chrome_version("0.3.0.65536"), None);
    assert_eq!(parse_chrome_version("1.2.3.4.5"), None);
    assert_eq!(parse_chrome_version(""), None);
}

#[test]
fn chrome_version_arrays_order_like_browser_versions() {
    let older = parse_chrome_version("0.3.0.1044").unwrap();
    let newer = parse_chrome_version("0.3.0.1045").unwrap();
    let stable = parse_chrome_version("0.3.0").unwrap();
    assert!(older < newer);
    assert!(stable < newer);
}

#[test]
fn latest_installed_extension_version_picks_highest_valid_version_dir() {
    let root = std::env::temp_dir().join(format!("aipass-ext-ver-{}", Uuid::new_v4()));
    let extension_dir = root
        .join("Extensions")
        .join("aabbccddeeffgghhiijjkkllmmnnoopp");
    fs::create_dir_all(extension_dir.join("0.3.0.1044")).unwrap();
    fs::write(
        extension_dir.join("0.3.0.1044").join("manifest.json"),
        b"{}",
    )
    .unwrap();
    fs::create_dir_all(extension_dir.join("0.3.0.1045")).unwrap();
    fs::write(
        extension_dir.join("0.3.0.1045").join("manifest.json"),
        b"{}",
    )
    .unwrap();
    // Invalid names and manifest-less dirs are ignored.
    fs::create_dir_all(extension_dir.join("0.3.0-nightly.20260905")).unwrap();
    fs::create_dir_all(extension_dir.join("0.3.0.9999")).unwrap();

    assert_eq!(
        latest_installed_extension_version(&extension_dir),
        Some([0, 3, 0, 1045])
    );
    assert_eq!(
        latest_installed_extension_version(&root.join("missing")),
        None
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn extract_extension_package_skips_entries_escaping_target_dir() {
    use std::io::Write;

    let root = std::env::temp_dir().join(format!("aipass-zip-test-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let zip_path = root.join("package.zip");
    {
        let file = fs::File::create(&zip_path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("../evil.txt", options).unwrap();
        writer.write_all(b"evil").unwrap();
        writer.start_file("nested/manifest.json", options).unwrap();
        writer.write_all(b"{}").unwrap();
        writer.finish().unwrap();
    }

    let extract_dir = root.join("extract");
    extract_extension_package(&zip_path, &extract_dir).unwrap();

    assert!(!root.join("evil.txt").exists());
    assert_eq!(
        fs::read_to_string(extract_dir.join("nested").join("manifest.json")).unwrap(),
        "{}"
    );

    // Re-extraction clears stale files from a previous version.
    fs::write(extract_dir.join("stale.txt"), b"stale").unwrap();
    extract_extension_package(&zip_path, &extract_dir).unwrap();
    assert!(!extract_dir.join("stale.txt").exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn native_messaging_dir_alone_does_not_count_as_browser_profile() {
    let root = std::env::temp_dir().join(format!("aipass-profile-test-{}", Uuid::new_v4()));
    fs::create_dir_all(root.join("NativeMessagingHosts")).unwrap();

    assert!(!profile_root_has_browser_data(&root));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn browser_profile_preferences_count_as_browser_profile() {
    let root = std::env::temp_dir().join(format!("aipass-profile-test-{}", Uuid::new_v4()));
    fs::create_dir_all(root.join("Default")).unwrap();
    fs::write(root.join("Default").join("Preferences"), "{}").unwrap();

    assert!(profile_root_has_browser_data(&root));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn aipass_manifest_signal_matches_short_name_when_name_is_placeholder() {
    let manifest = serde_json::json!({
        "name": "__MSG_extensionName__",
        "short_name": "AIPass",
        "permissions": ["nativeMessaging"]
    });

    assert!(value_contains_aipass_manifest_signal(&manifest));
}

#[test]
fn aipass_manifest_signal_ignores_unrelated_manifests() {
    let native_messaging_only = serde_json::json!({
        "name": "Some Other Tool",
        "permissions": ["nativeMessaging"]
    });
    let unrelated_short_name = serde_json::json!({
        "name": "__MSG_extensionName__",
        "short_name": "Definitely Not AIPass"
    });

    assert!(!value_contains_aipass_manifest_signal(
        &native_messaging_only
    ));
    assert!(!value_contains_aipass_manifest_signal(
        &unrelated_short_name
    ));
}

#[test]
fn merge_extension_ids_unions_normalizes_and_dedups() {
    let merged = merge_extension_ids(
        vec!["chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/".to_string()],
        vec!["BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB".to_string()],
        vec![
            "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            "chrome-extension://cccccccccccccccccccccccccccccccc/".to_string(),
        ],
    );

    assert_eq!(
        merged,
        vec![
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
            "cccccccccccccccccccccccccccccccc".to_string(),
        ]
    );
}

#[test]
fn merge_extension_ids_handles_empty_and_partial_inputs() {
    let merged = merge_extension_ids(Vec::new(), Vec::new(), Vec::new());
    assert!(merged.is_empty());

    let merged = merge_extension_ids(
        Vec::<String>::new(),
        Vec::new(),
        vec!["chrome-extension://dddddddddddddddddddddddddddddddd/".to_string()],
    );
    assert_eq!(merged, vec!["dddddddddddddddddddddddddddddddd".to_string()]);
}
