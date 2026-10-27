use aipass_agent_protocol::{UpdateChannel, UpdateOutcome};
use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use semver::Version;
use serde::Deserialize;
use std::{io::Read, time::Duration};

const API: &str = "https://api.github.com/repos/backrunner/aipass/releases";
const MAX_METADATA: u64 = 4 * 1024 * 1024;
pub(super) const MAX_ARCHIVE: u64 = 256 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
    size: u64,
}

#[derive(Debug)]
pub struct CliRelease {
    pub outcome: UpdateOutcome,
    pub(super) url: String,
    pub(super) sha256: String,
    pub(super) size: u64,
    pub(super) timeout: Duration,
}

pub(super) fn client(timeout: Duration) -> Result<Client> {
    Ok(Client::builder()
        .user_agent(concat!("aipass/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .connect_timeout(Duration::from_secs(15).min(timeout))
        .timeout(timeout)
        .build()?)
}

pub(super) fn bounded(mut response: reqwest::blocking::Response, limit: u64) -> Result<Vec<u8>> {
    response.error_for_status_ref()?;
    if response.content_length().is_some_and(|size| size > limit) {
        bail!("update response exceeds the size limit");
    }
    let mut bytes = Vec::new();
    response.by_ref().take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        bail!("update response exceeds the size limit");
    }
    Ok(bytes)
}

pub fn check_cli(channel: UpdateChannel, current: &str, timeout: Duration) -> Result<CliRelease> {
    let client = client(timeout)?;
    let response = if channel == UpdateChannel::Official {
        client.get(format!("{API}/latest"))
    } else {
        client.get(API).query(&[("per_page", "100")])
    }
    .send()
    .context("failed to fetch official release metadata")?;
    let bytes = bounded(response, MAX_METADATA)?;
    let releases: Vec<Release> = if channel == UpdateChannel::Official {
        vec![serde_json::from_slice(&bytes)?]
    } else {
        serde_json::from_slice(&bytes)?
    };
    select_release(releases, channel, current, timeout)
}

fn channel_matches(version: &Version, channel: UpdateChannel) -> bool {
    let prefix = version.pre.as_str().split('.').next().unwrap_or_default();
    match channel {
        UpdateChannel::Official => version.pre.is_empty(),
        UpdateChannel::Beta => prefix == "beta",
        UpdateChannel::Nightly => prefix == "nightly",
    }
}

fn select_release(
    releases: Vec<Release>,
    channel: UpdateChannel,
    current: &str,
    timeout: Duration,
) -> Result<CliRelease> {
    let current_version = Version::parse(current)?;
    let (version, release) = releases
        .into_iter()
        .filter(|release| !release.draft)
        .filter_map(|release| {
            Version::parse(release.tag_name.trim_start_matches('v'))
                .ok()
                .map(|v| (v, release))
        })
        .filter(|(version, _)| channel_matches(version, channel))
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .context("no published release in the selected channel")?;
    let mut outcome = UpdateOutcome::new("cli", current);
    outcome.latest_version = Some(version.to_string());
    outcome.available = version > current_version;
    if !outcome.available {
        return Ok(CliRelease {
            outcome,
            url: String::new(),
            sha256: String::new(),
            size: 0,
            timeout,
        });
    }
    let name = match std::env::consts::OS {
        "macos" => "aipass-cli-macos.tar.gz",
        "linux" => "aipass-cli-linux.tar.gz",
        "windows" => "aipass-cli-windows.zip",
        _ => bail!("self-update is unsupported on this platform"),
    };
    let asset = release
        .assets
        .into_iter()
        .find(|asset| asset.name == name)
        .context("release has no CLI package for this platform")?;
    let expected = format!(
        "https://github.com/backrunner/aipass/releases/download/{}/{name}",
        release.tag_name
    );
    if asset.browser_download_url != expected || asset.size == 0 || asset.size > MAX_ARCHIVE {
        bail!("invalid official CLI release asset");
    }
    let digest = asset
        .digest
        .as_deref()
        .and_then(|s| s.strip_prefix("sha256:"))
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .context("official release has no SHA-256 digest; download and verify it manually")?;
    outcome.status = "available".into();
    Ok(CliRelease {
        outcome,
        url: expected,
        sha256: digest.to_ascii_lowercase(),
        size: asset.size,
        timeout,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn release(tag: &str) -> Release {
        Release {
            tag_name: tag.into(),
            draft: false,
            assets: vec![],
        }
    }
    #[test]
    fn selection_is_semantic_channel_specific_and_never_downgrades() {
        let values = vec![
            release("v1.0.0-beta.9"),
            release("v1.0.0-beta.10"),
            release("v1.0.0-nightly.20"),
            release("v1.0.0"),
        ];
        let result =
            select_release(values, UpdateChannel::Beta, "2.0.0", Duration::from_secs(1)).unwrap();
        assert_eq!(
            result.outcome.latest_version.as_deref(),
            Some("1.0.0-beta.10")
        );
        assert!(!result.outcome.available);
        let mut draft = release("v9.0.0");
        draft.draft = true;
        assert!(select_release(
            vec![draft],
            UpdateChannel::Official,
            "1.0.0",
            Duration::from_secs(1)
        )
        .is_err());
        assert!(select_release(
            vec![release("v9.0.0-rc.1")],
            UpdateChannel::Beta,
            "1.0.0",
            Duration::from_secs(1)
        )
        .is_err());
    }
    #[test]
    fn available_updates_require_the_exact_asset_and_digest() {
        assert!(select_release(
            vec![release("v9.0.0")],
            UpdateChannel::Official,
            "1.0.0",
            Duration::from_secs(1)
        )
        .is_err());
    }
}
