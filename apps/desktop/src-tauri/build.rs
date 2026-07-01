fn main() {
    if std::env::var("PROFILE").as_deref() == Ok("debug") {
        ensure_debug_bundle_resource_placeholders();
    }
    tauri_build::build()
}

fn ensure_debug_bundle_resource_placeholders() {
    let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") else {
        return;
    };
    let workspace_root = std::path::Path::new(&manifest_dir)
        .ancestors()
        .nth(3)
        .map(std::path::Path::to_path_buf);
    let Some(workspace_root) = workspace_root else {
        return;
    };
    let release_dir = workspace_root.join("target").join("release");
    if std::fs::create_dir_all(&release_dir).is_err() {
        return;
    }
    for name in [agent_binary_name(), native_host_binary_name()] {
        let exact_binary_path = release_dir.join(name);
        if std::fs::metadata(&exact_binary_path)
            .map(|metadata| metadata.len() == 0)
            .unwrap_or(false)
        {
            let _ = std::fs::remove_file(&exact_binary_path);
        }

        let path = release_dir.join(format!("{name}.resource-placeholder"));
        if path.exists() {
            continue;
        }
        if std::fs::write(&path, []).is_err() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
        }
    }

    let extension_build_dir = workspace_root.join("apps").join("extension").join("build");
    if std::fs::create_dir_all(&extension_build_dir).is_err() {
        return;
    }
    let crx_path = extension_build_dir.join("aipass-extension.crx");
    if !crx_path.exists() {
        let _ = std::fs::write(&crx_path, []);
    }
    let metadata_path = extension_build_dir.join("aipass-extension.json");
    if !metadata_path.exists() {
        let _ = std::fs::write(
            &metadata_path,
            r#"{
  "id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "name": "AIPass",
  "version": "0.0.0",
  "crx": "aipass-extension.crx",
  "zip": "aipass-extension.zip"
}
"#,
        );
    }
}

fn agent_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "aipass-agent.exe"
    } else {
        "aipass-agent"
    }
}

fn native_host_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "aipass-native-host.exe"
    } else {
        "aipass-native-host"
    }
}
