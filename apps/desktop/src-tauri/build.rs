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
        let path = release_dir.join(name);
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
