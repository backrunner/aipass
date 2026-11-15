use super::*;
pub fn native_host_status_snapshot() -> Result<NativeHostStatus, String> {
    let statuses = native_host_statuses_snapshot()?;
    let allowed_extension_ids = load_allowed_extension_ids().unwrap_or_default();
    preferred_native_host_status(&statuses, &allowed_extension_ids)
}

pub(super) fn native_host_statuses_snapshot() -> Result<Vec<NativeHostStatus>, String> {
    let host_path = native_host_binary_path()?;
    let host_status = native_host_binary_status(&host_path);
    let settings_path = native_host_settings_path().map_err(|err| err.to_string())?;
    let allowed_extension_ids = load_allowed_extension_ids().map_err(|err| err.to_string())?;
    repair_browser_targets()?
        .into_iter()
        .map(|target| {
            let allowed_origins = read_manifest_allowed_origins(&target.manifest_path);
            Ok(NativeHostStatus {
                browser: target.id.to_string(),
                browser_label: target.label.to_string(),
                host_exists: host_status.exists,
                host_usable: host_status.usable,
                host_error: host_status.error.clone(),
                host_path: host_path.clone(),
                manifest_exists: target.manifest_path.exists(),
                manifest_path: target.manifest_path,
                settings_path: settings_path.clone(),
                allowed_extension_ids: allowed_extension_ids.clone(),
                allowed_origins,
            })
        })
        .collect()
}

pub(super) fn preferred_native_host_status(
    statuses: &[NativeHostStatus],
    extension_ids: &[String],
) -> Result<NativeHostStatus, String> {
    statuses
        .iter()
        .find(|status| native_host_status_allows(status, extension_ids))
        .or_else(|| statuses.first())
        .cloned()
        .ok_or_else(|| "native host repair is not supported on this platform".to_string())
}

pub(super) fn native_host_status_allows(
    status: &NativeHostStatus,
    extension_ids: &[String],
) -> bool {
    status.host_usable
        && status.manifest_exists
        && extension_ids.iter().any(|extension_id| {
            let extension_id = normalized_extension_id(extension_id);
            status
                .allowed_extension_ids
                .iter()
                .any(|id| normalized_extension_id(id) == extension_id)
                || status
                    .allowed_origins
                    .iter()
                    .any(|origin| normalized_extension_id(origin) == extension_id)
        })
}

pub fn repair_native_host_manifest(extension_ids: Vec<String>) -> Result<NativeHostStatus, String> {
    let host_path = native_host_binary_path()?;
    ensure_native_host_binary_usable(&host_path)?;
    let extension_ids = merged_extension_ids_for_native_host(extension_ids);
    let targets = repair_browser_targets()?;
    let mut all_extension_ids: BTreeSet<String> = extension_ids.iter().cloned().collect();
    for target in &targets {
        let existing_origins = read_manifest_allowed_origins(&target.manifest_path);
        let target_extension_ids =
            merge_extension_ids(extension_ids.iter().cloned(), Vec::new(), existing_origins);
        all_extension_ids.extend(target_extension_ids.iter().cloned());
        let origins = allowed_origins(&target_extension_ids)?;
        if let Some(parent) = target.manifest_path.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let manifest = native_manifest(&host_path, &origins);
        let bytes = serde_json::to_vec_pretty(&manifest).map_err(|err| err.to_string())?;
        atomic_write_bytes(&target.manifest_path, &bytes).map_err(|err| err.to_string())?;
        install_native_manifest_reference(target, &target.manifest_path)?;
    }
    let extension_ids: Vec<String> = all_extension_ids.into_iter().collect();
    save_allowed_extension_ids(&extension_ids).map_err(|err| err.to_string())?;
    let statuses = native_host_statuses_snapshot()?;
    preferred_native_host_status(&statuses, &extension_ids)
}

pub(super) fn native_host_binary_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    let host_name = if cfg!(target_os = "windows") {
        "aipass-native-host.exe"
    } else {
        "aipass-native-host"
    };
    let mut candidates = aipass_agent::bundled::helper_candidates(&exe, host_name);
    if let Some(found) = candidates
        .iter()
        .find(|candidate| native_host_binary_status(candidate).usable)
        .cloned()
    {
        Ok(found)
    } else {
        Ok(candidates.remove(0))
    }
}

#[derive(Clone, Debug)]
pub(super) struct NativeHostBinaryStatus {
    pub(super) exists: bool,
    pub(super) usable: bool,
    pub(super) error: Option<String>,
}

pub(super) fn native_host_binary_status(path: &Path) -> NativeHostBinaryStatus {
    let Ok(metadata) = fs::metadata(path) else {
        return NativeHostBinaryStatus {
            exists: false,
            usable: false,
            error: Some("native host binary was not found".to_string()),
        };
    };
    if !metadata.is_file() {
        return NativeHostBinaryStatus {
            exists: true,
            usable: false,
            error: Some("native host path is not a file".to_string()),
        };
    }
    if metadata.len() == 0 {
        return NativeHostBinaryStatus {
            exists: true,
            usable: false,
            error: Some("native host binary is empty".to_string()),
        };
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o111 == 0 {
        return NativeHostBinaryStatus {
            exists: true,
            usable: false,
            error: Some("native host binary is not executable".to_string()),
        };
    }
    NativeHostBinaryStatus {
        exists: true,
        usable: true,
        error: None,
    }
}

pub(super) fn ensure_native_host_binary_usable(path: &Path) -> Result<(), String> {
    let status = native_host_binary_status(path);
    if status.usable {
        Ok(())
    } else {
        Err(format!(
            "native host binary is not usable at {}: {}",
            path.display(),
            status
                .error
                .unwrap_or_else(|| "unknown validation error".to_string())
        ))
    }
}

pub(super) fn allowed_origins(extension_ids: &[String]) -> Result<Vec<String>, String> {
    let origins = extension_ids
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| {
            if value.starts_with("chrome-extension://") {
                if value.ends_with('/') {
                    value.to_string()
                } else {
                    format!("{value}/")
                }
            } else {
                format!("chrome-extension://{value}/")
            }
        })
        .collect::<Vec<_>>();
    if origins.is_empty() {
        return Err("enter at least one browser extension id".to_string());
    }
    Ok(origins)
}

pub(super) fn normalized_extension_id(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("chrome-extension://")
        .trim_start_matches("chrome://")
        .trim_end_matches('/')
        .to_lowercase()
}

pub(super) fn read_manifest_allowed_origins(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| {
            value
                .get("allowed_origins")
                .and_then(|items| items.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(ToString::to_string))
                        .collect()
                })
        })
        .unwrap_or_default()
}

pub(super) fn install_native_manifest_reference(
    target: &BrowserTarget,
    manifest_path: &Path,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let status = ProcessCommand::new("reg")
            .args([
                "add",
                target.native_host_registry_key,
                "/ve",
                "/t",
                "REG_SZ",
                "/d",
                &manifest_path.display().to_string(),
                "/f",
            ])
            .status()
            .map_err(|err| err.to_string())?;
        if !status.success() {
            return Err("native host registry update failed".to_string());
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = target;
        let _ = manifest_path;
    }

    Ok(())
}
