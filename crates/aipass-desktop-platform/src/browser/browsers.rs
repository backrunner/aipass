use super::*;
#[derive(Clone, Debug)]
pub(super) struct BrowserTarget {
    pub(super) id: &'static str,
    pub(super) label: &'static str,
    pub(super) manifest_path: PathBuf,
    pub(super) profile_roots: Vec<PathBuf>,
    pub(super) executable_candidates: Vec<PathBuf>,
    #[cfg(target_os = "windows")]
    pub(super) native_host_registry_key: &'static str,
}

pub(super) fn installed_extension_paths(extension_ids: &[String]) -> Vec<PathBuf> {
    known_browser_targets()
        .into_iter()
        .flat_map(|target| {
            target
                .profile_roots
                .into_iter()
                .flat_map(|profile_root| {
                    fs::read_dir(profile_root)
                        .ok()
                        .into_iter()
                        .flat_map(|items| items.filter_map(|item| item.ok()))
                        .map(|entry| entry.path())
                        .filter(|path| path.is_dir())
                        .flat_map(|path| {
                            extension_ids
                                .iter()
                                .map(move |id| path.join("Extensions").join(id))
                        })
                        .filter(|path| path.exists())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

pub(super) fn detected_browser_targets() -> Vec<BrowserTarget> {
    let mut detected = known_browser_targets()
        .into_iter()
        .filter(browser_target_detected)
        .collect::<Vec<_>>();
    detected.sort_by_key(|target| target_sort_rank(target.id));
    detected
}

pub(super) fn browser_target_detected(target: &BrowserTarget) -> bool {
    find_browser_path(target).is_some()
        || target
            .profile_roots
            .iter()
            .any(|profile_root| profile_root_has_browser_data(profile_root))
}

pub(super) fn profile_root_has_browser_data(profile_root: &Path) -> bool {
    if profile_root.join("Local State").is_file() {
        return true;
    }
    for entry in fs::read_dir(profile_root)
        .ok()
        .into_iter()
        .flat_map(|items| items.filter_map(|item| item.ok()))
    {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.file_name().and_then(|value| value.to_str()) == Some("NativeMessagingHosts") {
            continue;
        }
        if path.join("Preferences").is_file()
            || path.join("Secure Preferences").is_file()
            || path.join("Extensions").is_dir()
        {
            return true;
        }
    }
    false
}

pub(super) fn repair_browser_targets() -> Result<Vec<BrowserTarget>, String> {
    let detected = detected_browser_targets();
    if !detected.is_empty() {
        return Ok(detected);
    }
    known_browser_targets()
        .into_iter()
        .next()
        .map(|target| vec![target])
        .ok_or_else(|| "native host repair is not supported on this platform".to_string())
}

pub(super) fn preferred_browser_target(targets: &[BrowserTarget]) -> Option<&BrowserTarget> {
    targets
        .iter()
        .find(|target| find_browser_path(target).is_some())
        .or_else(|| targets.first())
}

pub(super) fn target_sort_rank(id: &str) -> usize {
    match id {
        "chrome" => 0,
        "edge" => 1,
        "brave" => 2,
        "arc" => 3,
        "chromium" => 4,
        _ => 10,
    }
}

pub(super) fn known_browser_targets() -> Vec<BrowserTarget> {
    #[cfg(target_os = "macos")]
    {
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            return Vec::new();
        };
        let support = home.join("Library").join("Application Support");
        vec![
            mac_browser_target(
                &support,
                "chrome",
                "Google Chrome",
                "Google/Chrome",
                &["Google Chrome"],
            ),
            mac_browser_target(
                &support,
                "edge",
                "Microsoft Edge",
                "Microsoft Edge",
                &["Microsoft Edge"],
            ),
            mac_browser_target(
                &support,
                "brave",
                "Brave",
                "BraveSoftware/Brave-Browser",
                &["Brave Browser"],
            ),
            mac_browser_target(&support, "arc", "Arc", "Arc/User Data", &["Arc"]),
            mac_browser_target(&support, "chromium", "Chromium", "Chromium", &["Chromium"]),
            mac_browser_target(
                &support,
                "chrome-beta",
                "Google Chrome Beta",
                "Google/Chrome Beta",
                &["Google Chrome Beta"],
            ),
            mac_browser_target(
                &support,
                "chrome-dev",
                "Google Chrome Dev",
                "Google/Chrome Dev",
                &["Google Chrome Dev"],
            ),
            mac_browser_target(
                &support,
                "chrome-canary",
                "Google Chrome Canary",
                "Google/Chrome Canary",
                &["Google Chrome Canary"],
            ),
            mac_browser_target(
                &support,
                "edge-beta",
                "Microsoft Edge Beta",
                "Microsoft Edge Beta",
                &["Microsoft Edge Beta"],
            ),
            mac_browser_target(
                &support,
                "edge-dev",
                "Microsoft Edge Dev",
                "Microsoft Edge Dev",
                &["Microsoft Edge Dev"],
            ),
            mac_browser_target(
                &support,
                "edge-canary",
                "Microsoft Edge Canary",
                "Microsoft Edge Canary",
                &["Microsoft Edge Canary"],
            ),
            mac_browser_target(&support, "vivaldi", "Vivaldi", "Vivaldi", &["Vivaldi"]),
            mac_browser_target(
                &support,
                "vivaldi-snapshot",
                "Vivaldi Snapshot",
                "Vivaldi Snapshot",
                &["Vivaldi Snapshot"],
            ),
        ]
    }

    #[cfg(target_os = "linux")]
    {
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            return Vec::new();
        };
        let config = aipass_agent::paths::linux_config_home(&home);
        vec![
            linux_browser_target(
                "chrome",
                "Google Chrome",
                config.join("google-chrome"),
                &["google-chrome", "google-chrome-stable"],
            ),
            linux_browser_target(
                "edge",
                "Microsoft Edge",
                config.join("microsoft-edge"),
                &["microsoft-edge", "microsoft-edge-stable"],
            ),
            linux_browser_target(
                "brave",
                "Brave",
                config.join("BraveSoftware").join("Brave-Browser"),
                &["brave-browser", "brave"],
            ),
            linux_browser_target(
                "chromium",
                "Chromium",
                config.join("chromium"),
                &["chromium", "chromium-browser"],
            ),
            linux_browser_target(
                "vivaldi",
                "Vivaldi",
                config.join("vivaldi"),
                &["vivaldi", "vivaldi-stable"],
            ),
        ]
    }

    #[cfg(target_os = "windows")]
    {
        let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) else {
            return Vec::new();
        };
        let mut executable_roots = vec![local_app_data.clone()];
        executable_roots.extend(
            ["PROGRAMFILES", "PROGRAMFILES(X86)"]
                .into_iter()
                .filter_map(std::env::var_os)
                .map(PathBuf::from),
        );
        let application_candidates = |vendor: &str, browser: &str, executable: &str| {
            executable_roots
                .iter()
                .map(|root| {
                    root.join(vendor)
                        .join(browser)
                        .join("Application")
                        .join(executable)
                })
                .collect::<Vec<_>>()
        };
        let app_data = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| local_app_data.join("AIPass"));
        let shared_manifest = app_data
            .join("AIPass")
            .join("NativeMessagingHosts")
            .join("dev.aipass.native.json");
        vec![
            windows_browser_target(
                "chrome",
                "Google Chrome",
                local_app_data
                    .join("Google")
                    .join("Chrome")
                    .join("User Data"),
                shared_manifest.clone(),
                &application_candidates("Google", "Chrome", "chrome.exe"),
                r"HKCU\Software\Google\Chrome\NativeMessagingHosts\dev.aipass.native",
            ),
            windows_browser_target(
                "edge",
                "Microsoft Edge",
                local_app_data
                    .join("Microsoft")
                    .join("Edge")
                    .join("User Data"),
                shared_manifest.clone(),
                &application_candidates("Microsoft", "Edge", "msedge.exe"),
                r"HKCU\Software\Microsoft\Edge\NativeMessagingHosts\dev.aipass.native",
            ),
            windows_browser_target(
                "brave",
                "Brave",
                local_app_data
                    .join("BraveSoftware")
                    .join("Brave-Browser")
                    .join("User Data"),
                shared_manifest.clone(),
                &application_candidates("BraveSoftware", "Brave-Browser", "brave.exe"),
                r"HKCU\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\dev.aipass.native",
            ),
        ]
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
pub(super) fn mac_browser_target(
    support_root: &Path,
    id: &'static str,
    label: &'static str,
    support_subdir: &str,
    app_names: &[&str],
) -> BrowserTarget {
    let profile_root = support_root.join(support_subdir);
    let executable_candidates = app_names
        .iter()
        .flat_map(|name| {
            [
                PathBuf::from("/Applications")
                    .join(format!("{name}.app"))
                    .join("Contents")
                    .join("MacOS")
                    .join(name),
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_default()
                    .join("Applications")
                    .join(format!("{name}.app"))
                    .join("Contents")
                    .join("MacOS")
                    .join(name),
            ]
        })
        .collect();
    BrowserTarget {
        id,
        label,
        manifest_path: profile_root
            .join("NativeMessagingHosts")
            .join("dev.aipass.native.json"),
        profile_roots: vec![profile_root],
        executable_candidates,
    }
}

#[cfg(target_os = "linux")]
pub(super) fn linux_browser_target(
    id: &'static str,
    label: &'static str,
    profile_root: PathBuf,
    executable_names: &[&str],
) -> BrowserTarget {
    BrowserTarget {
        id,
        label,
        manifest_path: profile_root
            .join("NativeMessagingHosts")
            .join("dev.aipass.native.json"),
        profile_roots: vec![profile_root],
        executable_candidates: executable_names
            .iter()
            .filter_map(|name| find_executable_in_path(name))
            .collect(),
    }
}

#[cfg(target_os = "windows")]
pub(super) fn windows_browser_target(
    id: &'static str,
    label: &'static str,
    profile_root: PathBuf,
    manifest_path: PathBuf,
    executable_candidates: &[PathBuf],
    native_host_registry_key: &'static str,
) -> BrowserTarget {
    BrowserTarget {
        id,
        label,
        manifest_path,
        profile_roots: vec![profile_root],
        executable_candidates: executable_candidates.to_vec(),
        native_host_registry_key,
    }
}

pub(super) fn find_browser_path(target: &BrowserTarget) -> Option<PathBuf> {
    let env_name = format!("AIPASS_{}_PATH", target.id.replace('-', "_").to_uppercase());
    if let Some(path) = std::env::var_os(env_name).map(PathBuf::from) {
        if path.exists() {
            return Some(path);
        }
    }

    target
        .executable_candidates
        .iter()
        .find(|path| path.exists())
        .cloned()
}

#[cfg(target_os = "linux")]
pub(super) fn find_executable_in_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

pub(super) fn open_browser_extensions_page(target: &BrowserTarget) -> Result<(), String> {
    let browser_path =
        find_browser_path(target).ok_or_else(|| format!("{} is not installed", target.label))?;
    ProcessCommand::new(browser_path)
        .arg("chrome://extensions")
        .spawn()
        .map(|_| ())
        .map_err(|err| err.to_string())
}

pub(super) fn extension_ids_for_native_host(primary_extension_id: &str) -> Vec<String> {
    merged_extension_ids_for_native_host([primary_extension_id.to_string()])
}

pub(super) fn merged_extension_ids_for_native_host(
    extension_ids: impl IntoIterator<Item = String>,
) -> Vec<String> {
    merge_extension_ids(extension_ids, discover_aipass_extension_ids(), Vec::new())
}

pub(super) fn merge_extension_ids(
    extension_ids: impl IntoIterator<Item = String>,
    discovered_ids: impl IntoIterator<Item = String>,
    existing_origins: impl IntoIterator<Item = String>,
) -> Vec<String> {
    let mut ids = BTreeSet::new();
    for value in extension_ids
        .into_iter()
        .chain(discovered_ids)
        .chain(existing_origins)
    {
        let id = normalized_extension_id(&value);
        if !id.is_empty() {
            ids.insert(id);
        }
    }
    ids.into_iter().collect()
}

pub(super) fn discover_aipass_extension_ids() -> Vec<String> {
    let mut ids = BTreeSet::new();
    for target in known_browser_targets() {
        for manifest in installed_extension_manifest_paths(&target) {
            if manifest_is_aipass(&manifest) {
                if let Some(id) = manifest
                    .parent()
                    .and_then(Path::parent)
                    .and_then(|path| path.file_name())
                    .and_then(|name| name.to_str())
                    .filter(|id| looks_like_extension_id(id))
                {
                    ids.insert(id.to_string());
                }
            }
        }

        for preferences_path in browser_preferences_paths(&target) {
            let Ok(bytes) = fs::read(&preferences_path) else {
                continue;
            };
            let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                continue;
            };
            collect_aipass_extension_ids_from_value(&value, &mut ids);
        }
    }
    ids.into_iter().collect()
}

pub(super) fn installed_extension_manifest_paths(target: &BrowserTarget) -> Vec<PathBuf> {
    target
        .profile_roots
        .iter()
        .flat_map(|profile_root| {
            fs::read_dir(profile_root)
                .ok()
                .into_iter()
                .flat_map(|items| items.filter_map(|item| item.ok()))
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .flat_map(|profile_path| {
                    fs::read_dir(profile_path.join("Extensions"))
                        .ok()
                        .into_iter()
                        .flat_map(|items| items.filter_map(|item| item.ok()))
                        .map(|entry| entry.path())
                        .filter(|path| path.is_dir())
                        .flat_map(|extension_path| {
                            fs::read_dir(&extension_path)
                                .ok()
                                .into_iter()
                                .flat_map(|items| items.filter_map(|item| item.ok()))
                                .map(|entry| entry.path().join("manifest.json"))
                                .filter(|path| path.exists())
                                .collect::<Vec<_>>()
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

pub(super) fn browser_preferences_paths(target: &BrowserTarget) -> Vec<PathBuf> {
    target
        .profile_roots
        .iter()
        .flat_map(|profile_root| {
            let mut paths = vec![
                profile_root.join("Preferences"),
                profile_root.join("Secure Preferences"),
            ];
            if let Ok(entries) = fs::read_dir(profile_root) {
                for entry in entries.filter_map(|entry| entry.ok()) {
                    let path = entry.path();
                    if path.is_dir() {
                        paths.push(path.join("Preferences"));
                        paths.push(path.join("Secure Preferences"));
                    }
                }
            }
            paths
        })
        .filter(|path| path.exists())
        .collect()
}

pub(super) fn manifest_is_aipass(path: &Path) -> bool {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .is_some_and(|value| value_contains_aipass_manifest_signal(&value))
}

pub(super) fn collect_aipass_extension_ids_from_value(
    value: &serde_json::Value,
    ids: &mut BTreeSet<String>,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                if looks_like_extension_id(key) && value_contains_aipass_manifest_signal(child) {
                    ids.insert(key.to_string());
                }
                collect_aipass_extension_ids_from_value(child, ids);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_aipass_extension_ids_from_value(item, ids);
            }
        }
        _ => {}
    }
}

pub(super) fn value_contains_aipass_manifest_signal(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            if map
                .get("name")
                .and_then(|value| value.as_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("AIPass"))
            {
                return true;
            }
            if map
                .get("short_name")
                .and_then(|value| value.as_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("AIPass"))
            {
                return true;
            }
            if map
                .get("manifest")
                .is_some_and(value_contains_aipass_manifest_signal)
            {
                return true;
            }
            if let Some(manifest_text) = map.get("manifest.json").and_then(|value| value.as_str()) {
                if serde_json::from_str::<serde_json::Value>(manifest_text)
                    .ok()
                    .is_some_and(|manifest| value_contains_aipass_manifest_signal(&manifest))
                {
                    return true;
                }
            }
            map.values().any(value_contains_aipass_manifest_signal)
        }
        serde_json::Value::Array(items) => items.iter().any(value_contains_aipass_manifest_signal),
        serde_json::Value::String(value) => {
            value.contains("\"name\": \"AIPass\"") || value.contains("\"short_name\": \"AIPass\"")
        }
        _ => false,
    }
}

pub fn looks_like_extension_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| (b'a'..=b'p').contains(&byte))
}

pub(super) fn reveal_path(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        ProcessCommand::new("open")
            .args(["-R"])
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|err| err.to_string())
    }

    #[cfg(target_os = "windows")]
    {
        let mut arg = OsString::from("/select,");
        arg.push(path.as_os_str());
        ProcessCommand::new("explorer")
            .arg(arg)
            .spawn()
            .map(|_| ())
            .map_err(|err| err.to_string())
    }

    #[cfg(target_os = "linux")]
    {
        let target = path.parent().unwrap_or(path);
        ProcessCommand::new("xdg-open")
            .arg(target)
            .spawn()
            .map(|_| ())
            .map_err(|err| err.to_string())
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = path;
        Err("opening the extension package is not supported on this platform".to_string())
    }
}
