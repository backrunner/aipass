use crate::*;

pub(crate) fn native_host_binary_path(explicit: Option<PathBuf>) -> Result<PathBuf> {
    let path = native_host_binary_candidate(explicit)?;
    ensure_native_host_binary_usable(&path)?;
    Ok(path)
}

pub(crate) fn native_host_binary_candidate(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return absolute_path(path);
    }
    let exe = std::env::current_exe().context("cannot determine current executable")?;
    let host_name = if cfg!(target_os = "windows") {
        "aipass-native-host.exe"
    } else {
        "aipass-native-host"
    };
    let sibling = exe.with_file_name(host_name);
    if sibling.exists() {
        return absolute_path(sibling);
    }
    absolute_path(PathBuf::from(host_name))
}

#[derive(Clone, Debug)]
pub(crate) struct NativeHostBinaryStatus {
    pub exists: bool,
    pub usable: bool,
    pub error: Option<String>,
}

pub(crate) fn native_host_binary_status(path: &Path) -> NativeHostBinaryStatus {
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

pub(crate) fn ensure_native_host_binary_usable(path: &Path) -> Result<()> {
    let status = native_host_binary_status(path);
    if status.usable {
        Ok(())
    } else {
        anyhow::bail!(
            "native host binary is not usable at {}: {}",
            path.display(),
            status
                .error
                .unwrap_or_else(|| "unknown validation error".to_string())
        )
    }
}

pub(crate) fn allowed_origins(extension_ids: &[String]) -> Result<Vec<String>> {
    extension_ids
        .iter()
        .map(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                anyhow::bail!("empty extension id");
            }
            if trimmed.starts_with("chrome-extension://") {
                return Ok(if trimmed.ends_with('/') {
                    trimmed.to_string()
                } else {
                    format!("{trimmed}/")
                });
            }
            Ok(format!("chrome-extension://{trimmed}/"))
        })
        .collect()
}

pub(crate) fn default_native_manifest_path(browser: &BrowserArg) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        let vendor_dir = match browser {
            BrowserArg::Chrome => "Google/Chrome",
            BrowserArg::Chromium => "Chromium",
            BrowserArg::Edge => "Microsoft Edge",
            BrowserArg::Brave => "BraveSoftware/Brave-Browser",
        };
        Some(
            home.join("Library")
                .join("Application Support")
                .join(vendor_dir)
                .join("NativeMessagingHosts")
                .join("dev.aipass.native.json"),
        )
    }

    #[cfg(target_os = "linux")]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        let vendor_dir = match browser {
            BrowserArg::Chrome => "google-chrome",
            BrowserArg::Chromium => "chromium",
            BrowserArg::Edge => "microsoft-edge",
            BrowserArg::Brave => "BraveSoftware/Brave-Browser",
        };
        Some(
            home.join(".config")
                .join(vendor_dir)
                .join("NativeMessagingHosts")
                .join("dev.aipass.native.json"),
        )
    }

    #[cfg(target_os = "windows")]
    {
        let app_data = std::env::var_os("APPDATA").map(PathBuf::from)?;
        Some(
            app_data
                .join("AIPass")
                .join("NativeMessagingHosts")
                .join("dev.aipass.native.json"),
        )
    }
}

pub(crate) fn install_native_manifest_reference(
    browser: &BrowserArg,
    manifest_path: &PathBuf,
) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        let key = match browser {
            BrowserArg::Chrome => {
                r"HKCU\Software\Google\Chrome\NativeMessagingHosts\dev.aipass.native"
            }
            BrowserArg::Chromium => {
                r"HKCU\Software\Chromium\NativeMessagingHosts\dev.aipass.native"
            }
            BrowserArg::Edge => {
                r"HKCU\Software\Microsoft\Edge\NativeMessagingHosts\dev.aipass.native"
            }
            BrowserArg::Brave => {
                r"HKCU\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\dev.aipass.native"
            }
        };
        let status = ProcessCommand::new("reg")
            .args([
                "add",
                key,
                "/ve",
                "/t",
                "REG_SZ",
                "/d",
                &manifest_path.display().to_string(),
                "/f",
            ])
            .status()
            .context("failed to register native host")?;
        if !status.success() {
            anyhow::bail!("native host registry update failed");
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (browser, manifest_path);
    }

    Ok(())
}

pub(crate) fn browser_name(browser: &BrowserArg) -> &'static str {
    match browser {
        BrowserArg::Chrome => "chrome",
        BrowserArg::Chromium => "chromium",
        BrowserArg::Edge => "edge",
        BrowserArg::Brave => "brave",
    }
}

pub(crate) fn native_host_browser_reports() -> serde_json::Value {
    let browsers = [
        BrowserArg::Chrome,
        BrowserArg::Chromium,
        BrowserArg::Edge,
        BrowserArg::Brave,
    ];
    serde_json::Value::Array(
        browsers
            .into_iter()
            .filter_map(|browser| {
                let manifest_path = default_native_manifest_path(&browser)?;
                let manifest_exists = manifest_path.exists();
                let manifest = fs::read_to_string(&manifest_path)
                    .ok()
                    .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
                let allowed_origins = manifest
                    .as_ref()
                    .and_then(|value| value.get("allowed_origins"))
                    .and_then(|value| value.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(ToString::to_string))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                Some(serde_json::json!({
                    "browser": browser_name(&browser),
                    "manifestPath": manifest_path,
                    "manifestExists": manifest_exists,
                    "allowedOrigins": allowed_origins,
                }))
            })
            .collect(),
    )
}
