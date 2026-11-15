//! Installed bundle metadata and compile-time trust, shared by UI and CLI updates.
use super::{Config, Host, Updater};
use crate::runtime::{instance::Instance, Runtime};
#[cfg(target_os = "macos")]
use anyhow::ensure;
use anyhow::{Context as _, Result};
use std::{path::Path, sync::Arc};

#[derive(Clone)]
pub struct Context {
    pub config: Config,
    pub runtime: Arc<Runtime>,
    pub instance: Instance,
}
impl Context {
    pub fn host(&self) -> Host {
        Host {
            runtime: Some(self.runtime.clone()),
            instance: Some(self.instance.clone()),
        }
    }
    pub fn updater(&self) -> Result<Updater> {
        Updater::create(self.config.clone(), crate::runtime_check::active())
    }
}

pub fn config(executable: &Path) -> Result<Option<Config>> {
    #[cfg(not(target_os = "macos"))]
    let metadata: serde_json::Value = serde_json::from_str(include_str!("../../desktop.json"))?;
    let version = env!("CARGO_PKG_VERSION").parse()?;
    #[cfg(target_os = "macos")]
    let (app_path, identifier) = {
        let Some(bundle) = executable
            .ancestors()
            .find(|path| path.extension().is_some_and(|ext| ext == "app"))
        else {
            return Ok(None);
        };
        let info = plist::Value::from_file(bundle.join("Contents/Info.plist"))?;
        let map = info
            .as_dictionary()
            .context("invalid application metadata")?;
        let identifier = map
            .get("CFBundleIdentifier")
            .and_then(plist::Value::as_string)
            .context("missing application identity")?;
        // Preview/capture bundles never inherit production update authority.
        if !matches!(identifier, "com.alkinum.aipass" | "com.alkinum.aipass.dev") {
            return Ok(None);
        }
        ensure!(
            map.get("CFBundleShortVersionString")
                .and_then(plist::Value::as_string)
                == Some(env!("CARGO_PKG_VERSION")),
            "installed version mismatch"
        );
        (bundle.to_path_buf(), identifier.to_owned())
    };
    #[cfg(not(target_os = "macos"))]
    let (app_path, identifier) = (
        executable.to_path_buf(),
        metadata["identifier"]
            .as_str()
            .context("missing identifier")?
            .to_owned(),
    );
    let preferences =
        aipass_desktop_platform::preferences::default_path().map_err(anyhow::Error::msg)?;
    let cache_dir = preferences
        .parent()
        .context("missing configuration directory")?
        .join(if cfg!(debug_assertions) {
            "native-updates-dev"
        } else {
            "native-updates"
        });
    #[cfg(target_os = "linux")]
    let app_path = std::env::var_os("APPIMAGE")
        .map(std::path::PathBuf::from)
        .unwrap_or(app_path);
    #[cfg(target_os = "linux")]
    let target = match super::bundle::target() {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    #[cfg(not(target_os = "linux"))]
    let target = format!(
        "{}-native-{}",
        if cfg!(target_os = "macos") {
            "darwin"
        } else {
            "windows"
        },
        std::env::consts::ARCH
    );
    Ok(Some(Config {
        endpoint: crate::runtime_check::endpoint().unwrap_or(super::endpoint(&version).parse()?),
        current_version: version,
        target,
        public_key: public_key()?,
        cache_dir: crate::runtime_check::path("cache/updates").unwrap_or(cache_dir),
        app_path,
        identifier,
    }))
}

pub fn public_key() -> Result<String> {
    let metadata: serde_json::Value = serde_json::from_str(include_str!("../../desktop.json"))?;
    Ok(option_env!("AIPASS_UPDATER_PUBLIC_KEY")
        .unwrap_or(
            metadata["updaterPublicKey"]
                .as_str()
                .context("missing updater trust root")?,
        )
        .to_owned())
}
