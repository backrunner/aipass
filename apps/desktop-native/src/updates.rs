//! Native desktop updater. Network, cache and installation run off the GPUI thread.
//! This does not depend on Tauri and does not accept unsigned packages.
mod bundle;
mod worker;
pub use worker::{Command, Host, Snapshot, Worker};

use aipass_storage::atomic_write_bytes;
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use minisign_verify::{PublicKey, Signature};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io::Read, path::PathBuf, time::Duration};
use url::Url;

const MAX_PACKAGE: u64 = 512 * 1024 * 1024;
const MAX_MANIFEST: u64 = 1024 * 1024;
pub const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub current_version: Version,
    pub endpoint: Url,
    pub target: String,
    pub public_key: String,
    pub cache_dir: PathBuf,
    pub app_path: PathBuf,
    pub identifier: String,
}

pub fn channel(version: &Version) -> &'static str {
    if version.pre.as_str().contains("nightly") {
        "nightly"
    } else if !version.pre.is_empty() {
        "beta"
    } else {
        "official"
    }
}

pub fn endpoint(version: &Version) -> &'static str {
    match channel(version) {
        "nightly" => "https://aipass.alkinum.io/api/updates/nightly/latest.json",
        "beta" => "https://aipass.alkinum.io/api/updates/beta/latest.json",
        _ => "https://github.com/backrunner/aipass/releases/latest/download/latest.json",
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Release {
    pub version: Version,
    pub notes: Option<String>,
    pub url: Url,
    pub signature: String,
}

#[derive(Deserialize)]
struct Manifest {
    version: Version,
    notes: Option<String>,
    platforms: BTreeMap<String, Platform>,
}

#[derive(Deserialize)]
struct Platform {
    url: Url,
    signature: String,
}

#[derive(Deserialize, Serialize)]
struct Cached {
    endpoint: Url,
    target: String,
    release: Release,
}

#[derive(Default, Deserialize, Serialize)]
struct Schedule {
    last_check: u64,
    dismissed: Option<Version>,
}

pub struct Updater {
    config: Config,
    client: reqwest::blocking::Client,
    key: PublicKey,
    allow_loopback: bool,
}

impl Updater {
    pub fn new(config: Config) -> Result<Self> {
        Self::create(config, false)
    }

    // Validation builds alone can use disposable keys and loopback fixtures.
    #[cfg(any(test, feature = "parity-render"))]
    pub fn validation(config: Config) -> Result<Self> {
        ensure!(
            config.identifier == "com.alkinum.aipass.gpui-validation",
            "invalid validation identity"
        );
        Self::create(config, true)
    }

    fn create(config: Config, allow_loopback: bool) -> Result<Self> {
        validate_url(&config.endpoint, allow_loopback)?;
        ensure!(
            config.target.starts_with("darwin-native-"),
            "update_release_unavailable"
        );
        let key = PublicKey::decode(std::str::from_utf8(&STANDARD.decode(&config.public_key)?)?)?;
        let client = reqwest::blocking::Client::builder()
            .user_agent(concat!("AIPass-native/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(300))
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= 10
                    || validate_url(attempt.url(), allow_loopback).is_err()
                {
                    attempt.stop()
                } else {
                    attempt.follow()
                }
            }))
            .build()?;
        Ok(Self {
            config,
            client,
            key,
            allow_loopback,
        })
    }

    pub fn check(&self) -> Result<Option<Release>> {
        let response = self
            .client
            .get(self.config.endpoint.clone())
            .send()?
            .error_for_status()?;
        if response.status() == reqwest::StatusCode::NO_CONTENT {
            return Ok(None);
        }
        ensure!(response.status().is_success(), "update_network_error");
        let bytes = bounded_read(response, MAX_MANIFEST, |_| {})?;
        let manifest: Manifest = serde_json::from_slice(&bytes)?;
        if manifest.version <= self.config.current_version {
            return Ok(None);
        }
        let platform = manifest
            .platforms
            .get(&self.config.target)
            .context("update_release_unavailable")?;
        validate_url(&platform.url, self.allow_loopback)?;
        Ok(Some(Release {
            version: manifest.version,
            notes: manifest.notes,
            url: platform.url.clone(),
            signature: platform.signature.clone(),
        }))
    }

    fn verify(&self, release: &Release, bytes: &[u8]) -> Result<()> {
        let verify = || -> Result<()> {
            let signature = STANDARD.decode(&release.signature)?;
            let signature = Signature::decode(std::str::from_utf8(&signature)?)?;
            self.key.verify(bytes, &signature, true)?;
            Ok(())
        };
        verify().context("update_signature_invalid")
    }

    fn cached(&self, release: &Release) -> Result<Option<Vec<u8>>> {
        let metadata = self.config.cache_dir.join("metadata.json");
        let package = self.config.cache_dir.join("package");
        if !metadata.try_exists()? && !package.try_exists()? {
            return Ok(None);
        }
        let read = || -> Result<Vec<u8>> {
            let cached: Cached = serde_json::from_slice(&bounded_read(
                fs::File::open(metadata)?,
                MAX_MANIFEST,
                |_| {},
            )?)?;
            ensure!(
                cached.endpoint == self.config.endpoint
                    && cached.target == self.config.target
                    && cached.release == *release,
                "stale update cache"
            );
            let bytes = bounded_read(fs::File::open(package)?, MAX_PACKAGE, |_| {})?;
            self.verify(release, &bytes)?;
            Ok(bytes)
        };
        match read() {
            Ok(bytes) => Ok(Some(bytes)),
            Err(_) => {
                self.clear()?;
                Ok(None)
            }
        }
    }

    pub fn download(&self, release: &Release, progress: impl Fn(u64)) -> Result<()> {
        if self.cached(release)?.is_some() {
            return Ok(());
        }
        validate_url(&release.url, self.allow_loopback)?;
        let response = self
            .client
            .get(release.url.clone())
            .send()?
            .error_for_status()?;
        ensure!(response.status().is_success(), "update_network_error");
        ensure!(
            response
                .content_length()
                .is_none_or(|size| size <= MAX_PACKAGE),
            "update_package_invalid"
        );
        let bytes = bounded_read(response, MAX_PACKAGE, progress)?;
        self.verify(release, &bytes)?;
        atomic_write_bytes(self.config.cache_dir.join("package"), &bytes)?;
        atomic_write_bytes(
            self.config.cache_dir.join("metadata.json"),
            &serde_json::to_vec(&Cached {
                endpoint: self.config.endpoint.clone(),
                target: self.config.target.clone(),
                release: release.clone(),
            })?,
        )?;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        // Removing the commit record first prevents a partial cleanup being pending.
        for name in ["metadata.json", "package"] {
            match fs::remove_file(self.config.cache_dir.join(name)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    pub fn has_pending(&self) -> bool {
        self.config.cache_dir.join("metadata.json").is_file()
    }

    pub fn prepare_install(&self, expected: &Release) -> Result<bundle::Prepared> {
        let release = self.check()?.context("update_release_unavailable")?;
        ensure!(release == *expected, "update release changed; check again");
        self.download(&release, |_| {})?;
        let bytes = self.cached(&release)?.context("update_cache_unavailable")?;
        // Verify the exact bytes handed to the archive reader immediately before staging.
        self.verify(&release, &bytes)?;
        bundle::Prepared::new(&self.config, &release, &bytes)
    }

    fn schedule(&self) -> Schedule {
        fs::read(self.config.cache_dir.join("schedule.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn initial_delay(&self, now: u64) -> Duration {
        let last = self.schedule().last_check;
        if last == 0 || last > now {
            return Duration::from_secs(3);
        }
        Duration::from_secs(INTERVAL.as_secs().saturating_sub(now - last))
    }

    fn checked(&self, now: u64) -> Result<()> {
        let mut schedule = self.schedule();
        schedule.last_check = now;
        atomic_write_bytes(
            self.config.cache_dir.join("schedule.json"),
            &serde_json::to_vec(&schedule)?,
        )?;
        Ok(())
    }

    fn dismiss(&self, version: Version) -> Result<()> {
        let mut schedule = self.schedule();
        schedule.dismissed = Some(version);
        atomic_write_bytes(
            self.config.cache_dir.join("schedule.json"),
            &serde_json::to_vec(&schedule)?,
        )?;
        Ok(())
    }
}

fn validate_url(url: &Url, allow_loopback: bool) -> Result<()> {
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "invalid update URL"
    );
    if url.scheme() == "https" {
        return Ok(());
    }
    if allow_loopback && url.scheme() == "http" && url.host_str() == Some("127.0.0.1") {
        return Ok(());
    }
    bail!("updates require HTTPS")
}

fn bounded_read(mut reader: impl Read, limit: u64, progress: impl Fn(u64)) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 64 * 1024];
    loop {
        let count = reader.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        ensure!(
            bytes.len() as u64 + count as u64 <= limit,
            "update_package_invalid"
        );
        bytes.extend_from_slice(&chunk[..count]);
        progress(bytes.len() as u64);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;
