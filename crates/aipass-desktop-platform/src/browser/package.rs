use super::*;
pub fn browser_extension_status_snapshot(
    metadata_path: &Path,
) -> Result<BrowserExtensionStatus, String> {
    let package = bundled_extension_package(metadata_path)?;
    let targets = detected_browser_targets();
    let extension_ids = extension_ids_for_native_host(&package.id);
    let native_hosts = native_host_statuses_snapshot()?;
    let native_host = preferred_native_host_status(&native_hosts, &extension_ids)?;
    let primary_target = preferred_browser_target(&targets);
    let browser_path = primary_target.and_then(find_browser_path);
    let installed_paths = installed_extension_paths(&extension_ids);
    let native_host_configured = native_hosts
        .iter()
        .any(|status| native_host_status_allows(status, &extension_ids));

    let zip_exists = package.zip_path.exists()
        && fs::metadata(&package.zip_path)
            .map(|metadata| metadata.len() > 0)
            .unwrap_or(false)
        && package.version != "0.0.0";

    Ok(BrowserExtensionStatus {
        browser: primary_target
            .map(|target| target.id.to_string())
            .unwrap_or_else(|| "chromium".to_string()),
        detected_browsers: targets
            .iter()
            .map(|target| target.label.to_string())
            .collect(),
        chrome_installed: !targets.is_empty(),
        chrome_path: browser_path,
        extension_id: package.id,
        discovered_extension_ids: extension_ids,
        extension_version: package.version,
        zip_exists,
        zip_path: package.zip_path,
        extension_installed: !installed_paths.is_empty(),
        installed_paths,
        native_host_configured,
        native_host,
        native_hosts,
    })
}

pub fn install_browser_extension(
    metadata_path: &Path,
) -> Result<BrowserExtensionInstallResult, String> {
    let package = bundled_extension_package(metadata_path)?;
    if !package.zip_path.exists()
        || fs::metadata(&package.zip_path)
            .map(|metadata| metadata.len() == 0)
            .unwrap_or(true)
        || package.version == "0.0.0"
    {
        return Err(format!(
            "bundled Chrome extension package is missing: {}",
            package.zip_path.display()
        ));
    }
    let targets = detected_browser_targets();
    let Some(target) = preferred_browser_target(&targets) else {
        return Err("A supported Chromium browser is not installed".to_string());
    };
    if find_browser_path(target).is_none() {
        return Err(format!("{} is not installed", target.label));
    }

    repair_native_host_manifest(vec![package.id.clone()])?;

    let extract_dir = bundled_extension_extract_dir(&package.id)?;
    extract_extension_package(&package.zip_path, &extract_dir).map_err(|err| {
        format!(
            "failed to extract bundled extension package to {}: {err}",
            extract_dir.display()
        )
    })?;

    let opened_chrome = open_browser_extensions_page(target).is_ok();
    let opened_package = reveal_path(&extract_dir).is_ok();
    let status = browser_extension_status_snapshot(metadata_path)?;
    Ok(BrowserExtensionInstallResult {
        status,
        opened_chrome,
        opened_package,
    })
}

#[derive(Clone, Debug)]
pub(super) struct ExtensionPackage {
    pub(super) id: String,
    pub(super) version: String,
    pub(super) manifest_version: String,
    pub(super) zip_path: PathBuf,
}

pub(super) fn bundled_extension_package(metadata_path: &Path) -> Result<ExtensionPackage, String> {
    let metadata_text = fs::read_to_string(metadata_path).map_err(|err| {
        format!(
            "failed to read bundled extension metadata at {}: {err}",
            metadata_path.display()
        )
    })?;
    let metadata_dir = metadata_path
        .parent()
        .ok_or_else(|| "bundled extension metadata path has no parent".to_string())?;
    parse_extension_package_metadata(&metadata_text, metadata_dir)
}

pub(super) fn parse_extension_package_metadata(
    metadata_text: &str,
    metadata_dir: &Path,
) -> Result<ExtensionPackage, String> {
    let metadata: serde_json::Value = serde_json::from_str(metadata_text)
        .map_err(|err| format!("failed to parse bundled extension metadata: {err}"))?;
    let id = metadata
        .get("id")
        .and_then(|value| value.as_str())
        .map(normalized_extension_id)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "bundled extension metadata is missing id".to_string())?;
    let version = metadata
        .get("version")
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "bundled extension metadata is missing version".to_string())?;
    let zip_name = metadata
        .get("zip")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("aipass-extension.zip");
    // The version written into manifest.json inside the zip. Nightly builds
    // stamp a Chrome-compatible numeric manifest version while `version`
    // keeps the full semver for display; older metadata lacks the field.
    let manifest_version = metadata
        .get("manifest_version")
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| version.clone());
    Ok(ExtensionPackage {
        id,
        version,
        manifest_version,
        zip_path: metadata_dir.join(zip_name),
    })
}

pub(super) fn bundled_extension_extract_dir(extension_id: &str) -> Result<PathBuf, String> {
    let dirs = aipass_agent::paths::project_dirs().map_err(|error| error.to_string())?;
    Ok(dirs.data_dir().join("browser-extension").join(extension_id))
}

pub(super) fn extract_extension_package(zip_path: &Path, extract_dir: &Path) -> Result<(), String> {
    if extract_dir.exists() {
        fs::remove_dir_all(extract_dir).map_err(|err| {
            format!(
                "failed to clear extension directory {}: {err}",
                extract_dir.display()
            )
        })?;
    }
    fs::create_dir_all(extract_dir).map_err(|err| {
        format!(
            "failed to create extension directory {}: {err}",
            extract_dir.display()
        )
    })?;
    let archive_file = fs::File::open(zip_path)
        .map_err(|err| format!("failed to open {}: {err}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(archive_file)
        .map_err(|err| format!("{} is not a valid zip archive: {err}", zip_path.display()))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| format!("failed to read zip entry {index}: {err}"))?;
        // Skip entries that would escape the extraction directory (zip-slip).
        let Some(relative_path) = entry.enclosed_name() else {
            continue;
        };
        let out_path = extract_dir.join(relative_path);
        if entry.is_dir() {
            fs::create_dir_all(&out_path)
                .map_err(|err| format!("failed to create {}: {err}", out_path.display()))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
        }
        let mut out_file = fs::File::create(&out_path)
            .map_err(|err| format!("failed to create {}: {err}", out_path.display()))?;
        std::io::copy(&mut entry, &mut out_file)
            .map_err(|err| format!("failed to write {}: {err}", out_path.display()))?;
    }
    Ok(())
}

// Chrome/Edge only accept extension versions of one to four dot-separated
// integers, each at most 65535. Returns the value zero-padded to four parts
// so arrays compare with the same ordering browsers use.
pub(super) fn parse_chrome_version(version: &str) -> Option<[u64; 4]> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.is_empty() || parts.len() > 4 {
        return None;
    }
    let mut parsed = [0u64; 4];
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() || !part.chars().all(|ch| ch.is_ascii_digit()) {
            return None;
        }
        let value: u64 = part.parse().ok()?;
        if value > 65535 {
            return None;
        }
        parsed[index] = value;
    }
    Some(parsed)
}

// The highest Chrome-compatible version directory containing a manifest under
// a browser profile's Extensions/<id> directory.
pub(super) fn latest_installed_extension_version(extension_dir: &Path) -> Option<[u64; 4]> {
    fs::read_dir(extension_dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.join("manifest.json").is_file())
        .filter_map(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .and_then(parse_chrome_version)
        })
        .max()
}

// Pushes the bundled extension package into every browser profile that already
// has it installed by extracting it as a new version directory — the same
// layout Chrome uses for self-hosted updates, picked up on the next browser
// launch. Runs silently at startup; failures are logged, never surfaced.
pub fn sync_installed_browser_extensions(metadata_path: &Path) {
    if let Err(err) = sync_installed_browser_extensions_inner(metadata_path) {
        let _ = log_event("desktop.extension_sync.failed", &[("error", &err)]);
    }
}

pub(super) fn sync_installed_browser_extensions_inner(metadata_path: &Path) -> Result<(), String> {
    let package = bundled_extension_package(metadata_path)?;
    if !package.zip_path.is_file() {
        return Ok(());
    }
    let Some(bundled_version) = parse_chrome_version(&package.manifest_version) else {
        // A non-numeric manifest version cannot be compared or loaded by the
        // browser; leave sideloaded copies untouched.
        let _ = log_event(
            "desktop.extension_sync.skipped",
            &[(
                "reason",
                "bundled manifest version is not Chrome-compatible",
            )],
        );
        return Ok(());
    };

    let extension_ids = extension_ids_for_native_host(&package.id);
    for installed_dir in installed_extension_paths(&extension_ids) {
        let Some(installed_max) = latest_installed_extension_version(&installed_dir) else {
            // Store-installed copy or an unknown layout: never touch it.
            continue;
        };
        // Never downgrade or rewrite an identical version in place.
        if bundled_version <= installed_max {
            continue;
        }
        let version_dir = installed_dir.join(&package.manifest_version);
        match extract_extension_package(&package.zip_path, &version_dir) {
            Ok(()) => {
                let _ = log_event(
                    "desktop.extension_sync.updated",
                    &[
                        ("path", &version_dir.display().to_string()),
                        ("version", &package.manifest_version),
                    ],
                );
            }
            Err(err) => {
                let _ = log_event(
                    "desktop.extension_sync.extract_failed",
                    &[
                        ("path", &version_dir.display().to_string()),
                        ("error", &err),
                    ],
                );
            }
        }
    }

    // Users who loaded the extension unpacked from our own data directory get
    // the same silent refresh; the browser picks the files up on relaunch.
    let unpack_dir = bundled_extension_extract_dir(&package.id)?;
    if unpack_dir.is_dir() {
        let current = fs::read_to_string(unpack_dir.join("manifest.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|value| value.get("version")?.as_str().map(ToString::to_string));
        let stale = current
            .as_deref()
            .and_then(parse_chrome_version)
            .map(|current| bundled_version > current)
            .unwrap_or(true);
        if stale {
            match extract_extension_package(&package.zip_path, &unpack_dir) {
                Ok(()) => {
                    let _ = log_event(
                        "desktop.extension_sync.updated",
                        &[
                            ("path", &unpack_dir.display().to_string()),
                            ("version", &package.manifest_version),
                        ],
                    );
                }
                Err(err) => {
                    let _ = log_event(
                        "desktop.extension_sync.extract_failed",
                        &[("path", &unpack_dir.display().to_string()), ("error", &err)],
                    );
                }
            }
        }
    }
    Ok(())
}
