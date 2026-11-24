//! Retrieve two bounded release assets by numeric ID. No repository code runs at inspection.
use super::{label, Manifest, Result};
use reqwest::{blocking::Client, redirect::Policy, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    time::{Duration, Instant},
};
use uuid::Uuid;

pub(super) const MODULE_LIMIT: u64 = 16 * 1024 * 1024;
const MANIFEST_ASSET: &str = "aipass-plugin.json";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Origin {
    pub repository: String,
    pub tag: String,
    pub release_id: u64,
    pub asset_id: u64,
    pub sha256: String,
}

pub(crate) struct Preview {
    alternate: Option<super::Runtime>,
    pub(super) token: Uuid,
    pub(super) created: Instant,
    pub(super) manifest: Manifest,
    pub(super) raw: Vec<u8>,
    pub(super) module: Vec<u8>,
    pub(super) origin: Origin,
    pub(super) digest: String,
}
impl Preview {
    pub(super) fn summary(&self) -> serde_json::Value {
        serde_json::json!({"id": self.manifest.id, "name": self.manifest.name, "version": self.manifest.version,
            "entry": self.manifest.entry, "digest": self.digest, "review": self.token,"alternate": self.alternate, "origin": self.origin,
            "runtime": self.manifest.runtime, "target": self.manifest.target, "origins": self.manifest.origins})
    }
}
#[derive(Deserialize)]
struct Release {
    id: u64,
    tag_name: String,
    assets: Vec<Asset>,
    draft: bool,
    prerelease: bool,
}
#[derive(Deserialize)]
struct Asset {
    id: u64,
    name: String,
    size: u64,
    digest: Option<String>,
    state: String,
}

fn repository(input: &str) -> Result<String> {
    let input = input
        .trim()
        .strip_prefix("https://github.com/")
        .unwrap_or(input.trim())
        .trim_end_matches('/');
    let input = input.strip_suffix(".git").unwrap_or(input);
    let (owner, repo) = input.split_once('/').ok_or("Invalid GitHub repository")?;
    let valid = |s: &str, max: usize| {
        !s.is_empty()
            && s.len() <= max
            && !matches!(s, "." | "..")
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    if !valid(owner, 39) || !valid(repo, 100) {
        return Err("Invalid GitHub repository".into());
    }
    Ok(format!("{owner}/{repo}"))
}

fn download_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && matches!(
            url.host_str(),
            Some(
                "api.github.com"
                    | "github.com"
                    | "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
            )
        )
}
fn fetch(client: &Client, url: &str, accept: &str, limit: u64) -> Result<Vec<u8>> {
    let response = client
        .get(url)
        .header("Accept", accept)
        .send()
        .map_err(|_| "GitHub download failed")?;
    if !response.status().is_success() {
        return Err(match response.status().as_u16() {
            404 => "GitHub release missing",
            403 | 429 => "GitHub rate limited",
            _ => "GitHub download failed",
        }
        .into());
    }
    if response.content_length().is_some_and(|n| n > limit) {
        return Err("GitHub asset exceeds size limit".into());
    }
    let mut bytes = Vec::new();
    response
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "GitHub download failed")?;
    if bytes.len() as u64 > limit {
        return Err("GitHub asset exceeds size limit".into());
    }
    Ok(bytes)
}
fn asset<'a>(release: &'a Release, name: &str, limit: u64) -> Result<&'a Asset> {
    let mut matches = release.assets.iter().filter(|a| a.name == name);
    let a = matches.next().ok_or("GitHub plugin assets missing")?;
    if matches.next().is_some()
        || a.id == 0
        || a.size == 0
        || a.size > limit
        || a.state != "uploaded"
    {
        return Err("Invalid GitHub plugin asset".into());
    }
    Ok(a)
}
fn verified(a: &Asset, bytes: &[u8]) -> Result<String> {
    let sha = format!("{:x}", Sha256::digest(bytes));
    if bytes.len() as u64 != a.size
        || a.digest.as_deref().and_then(|v| v.strip_prefix("sha256:")) != Some(&sha)
    {
        return Err("GitHub asset digest mismatch".into());
    }
    Ok(sha)
}
pub(crate) fn inspect(input: &str, native: bool) -> Result<Preview> {
    let repo = repository(input)?;
    let client = Client::builder()
        .user_agent("AIPass-subscription-plugins")
        .https_only(true)
        .timeout(Duration::from_secs(25))
        .connect_timeout(Duration::from_secs(6))
        .redirect(Policy::custom(|hop| {
            if hop.previous().len() < 4 && download_url(hop.url()) {
                hop.follow()
            } else {
                hop.error("Untrusted release redirect")
            }
        }))
        .build()
        .map_err(|_| "GitHub download failed")?;
    let base = format!("https://api.github.com/repos/{repo}/releases");
    let release = fetch(
        &client,
        &format!("{base}/latest"),
        "application/vnd.github+json",
        512 * 1024,
    )?;
    prepare(repo, &release, native, |id, limit| {
        fetch(
            &client,
            &format!("{base}/assets/{id}"),
            "application/octet-stream",
            limit,
        )
    })
}
fn prepare(
    repo: String,
    release: &[u8],
    native: bool,
    mut get: impl FnMut(u64, u64) -> Result<Vec<u8>>,
) -> Result<Preview> {
    let release: Release = serde_json::from_slice(release).map_err(|_| "Invalid GitHub release")?;
    if release.id == 0 || release.draft || release.prerelease || !label(&release.tag_name) {
        return Err("Invalid GitHub release".into());
    }
    let platform = format!(
        "aipass-plugin-{}-{}.json",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let portable_available = release.assets.iter().any(|a| a.name == MANIFEST_ASSET);
    let platform_available = release.assets.iter().any(|a| a.name == platform);
    let select_native = native || !portable_available;
    let name = if select_native {
        platform.as_str()
    } else {
        MANIFEST_ASSET
    };
    let alternate = (portable_available && platform_available).then_some(if select_native {
        super::Runtime::Wasi
    } else {
        super::Runtime::NativeSandbox
    });
    let a = asset(&release, name, 64 * 1024)?;
    let raw = get(a.id, a.size)?;
    verified(a, &raw)?;
    let manifest: Manifest =
        serde_json::from_slice(&raw).map_err(|_| "Invalid GitHub plugin manifest")?;
    if (select_native && manifest.runtime != Some(super::Runtime::NativeSandbox))
        || (!select_native && manifest.runtime != Some(super::Runtime::Wasi))
        || manifest.runtime.is_none()
        || manifest.interpreter.is_some()
        || !super::sandbox::safe_entry(&manifest.entry)
        || !manifest.valid()
    {
        return Err("GitHub requires a sandboxed plugin".into());
    }
    let a = asset(
        &release,
        &manifest.entry,
        if manifest.runtime == Some(super::Runtime::Wasi) {
            MODULE_LIMIT
        } else {
            super::FILE_LIMIT
        },
    )?;
    let module = get(a.id, a.size)?;
    let sha256 = verified(a, &module)?;
    super::sandbox::validate(&manifest, &module)?;
    let origin = Origin {
        repository: repo,
        tag: release.tag_name.clone(),
        release_id: release.id,
        asset_id: a.id,
        sha256,
    };
    Ok(Preview {
        alternate,
        token: Uuid::new_v4(),
        created: Instant::now(),
        digest: super::fingerprint(&raw, &module),
        manifest,
        raw,
        module,
        origin,
    })
}

#[cfg(test)]
#[path = "github_tests.rs"]
mod tests;
