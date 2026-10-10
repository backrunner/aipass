//! Vendor credential stores. Secret bytes never belong in previews or process arguments.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};
use zeroize::{Zeroize, Zeroizing};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Resource {
    File(PathBuf),
    Keychain { service: String, account: String },
}

impl Resource {
    pub fn read(&self) -> Result<Option<Zeroizing<Vec<u8>>>> {
        match self {
            Self::File(path) => match fs::symlink_metadata(path) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    bail!("credential symlinks are unsupported")
                }
                Ok(meta) if !meta.is_file() || meta.len() > 4 * 1024 * 1024 => {
                    bail!("invalid credential file")
                }
                Ok(_) => fs::read(path)
                    .map(|v| Some(Zeroizing::new(v)))
                    .context("credential file unavailable"),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(_) => bail!("credential file unavailable"),
            },
            Self::Keychain { service, account } => keychain_read(service, account),
        }
    }
    pub fn write(&self, bytes: Option<&[u8]>) -> Result<()> {
        match self {
            Self::File(path) => {
                // Recheck at the write boundary, including absence restoration.
                self.read()?;
                if let Some(bytes) = bytes {
                    ensure_private_parent(path)?;
                    aipass_storage::atomic_write_bytes(path, bytes)
                        .context("credential write failed")?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
                    }
                } else if path.exists() {
                    fs::remove_file(path)?;
                }
                Ok(())
            }
            Self::Keychain { service, account } => keychain_write(service, account, bytes),
        }
    }
    pub fn label(&self) -> String {
        match self {
            Self::File(path) => path.display().to_string(),
            Self::Keychain { .. } => "System credential store".into(),
        }
    }
}

#[cfg(target_os = "macos")]
fn keychain_read(service: &str, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
    match security_framework::passwords::get_generic_password(service, account) {
        Ok(v) => Ok(Some(Zeroizing::new(v))),
        Err(e) if e.code() == -25300 => Ok(None),
        Err(_) => bail!("system credential store unavailable"),
    }
}
#[cfg(target_os = "macos")]
fn keychain_write(service: &str, account: &str, bytes: Option<&[u8]>) -> Result<()> {
    use security_framework::passwords::{delete_generic_password, set_generic_password};
    match bytes {
        Some(v) => set_generic_password(service, account, v)
            .map_err(|_| anyhow::anyhow!("system credential write failed")),
        None => match delete_generic_password(service, account) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == -25300 => Ok(()),
            Err(_) => bail!("system credential removal failed"),
        },
    }
}
#[cfg(not(target_os = "macos"))]
fn keychain_read(_: &str, _: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
    bail!("system credential store unsupported on this platform")
}
#[cfg(not(target_os = "macos"))]
fn keychain_write(_: &str, _: &str, _: Option<&[u8]>) -> Result<()> {
    bail!("system credential store unsupported on this platform")
}

pub struct PrivateJson(pub Value);
impl Drop for PrivateJson {
    fn drop(&mut self) {
        fn erase(v: &mut Value) {
            match v {
                Value::String(s) => s.zeroize(),
                Value::Array(a) => a.iter_mut().for_each(erase),
                Value::Object(o) => o.values_mut().for_each(erase),
                _ => {}
            }
        }
        erase(&mut self.0);
    }
}
pub fn parse(bytes: &[u8]) -> Result<PrivateJson> {
    serde_json::from_slice(bytes)
        .map(PrivateJson)
        .map_err(|_| anyhow::anyhow!("invalid native credential data"))
}

#[derive(Clone)]
pub struct NativeStore {
    pub tool: String,
    pub home: PathBuf,
    pub primary: Resource,
    pub fallback: Option<Resource>,
    pub profile: Option<Resource>,
}
impl NativeStore {
    pub fn codex(home: &Path) -> Result<Self> {
        let config = Resource::File(home.join("config.toml")).read()?;
        let doc: toml_edit::DocumentMut = config
            .as_deref()
            .map(|v| std::str::from_utf8(v))
            .transpose()
            .context("invalid Codex configuration")?
            .unwrap_or("")
            .parse()
            .context("invalid Codex configuration")?;
        let mode = doc
            .get("cli_auth_credentials_store")
            .and_then(|v| v.as_str())
            .unwrap_or("file");
        if mode != "file"
            && (cfg!(windows)
                || doc
                    .get("features")
                    .and_then(|v| v.get("secret_auth_storage"))
                    .and_then(|v| v.as_bool())
                    == Some(true))
        {
            bail!("Codex encrypted secrets credential store is unsupported; no credentials were changed");
        }
        let file = Resource::File(home.join("auth.json"));
        let keychain = Resource::Keychain {
            service: "Codex Auth".into(),
            account: format!(
                "cli|{}",
                &format!(
                    "{:x}",
                    Sha256::digest(
                        home.canonicalize()
                            .unwrap_or(home.to_owned())
                            .to_string_lossy()
                            .as_bytes()
                    )
                )[..16]
            ),
        };
        let (primary, fallback) = match mode {
            "file" => (file, None),
            "keyring" => (keychain, Some(file)),
            // Select the actual backing store. An inaccessible keychain must not be treated as absence.
            "auto" => {
                if keychain.read()?.is_some() {
                    (keychain, Some(file))
                } else {
                    (file, Some(keychain))
                }
            }
            _ => bail!("unsupported Codex credential storage mode"),
        };
        Ok(Self {
            tool: "codex".into(),
            home: home.to_owned(),
            primary,
            fallback,
            profile: None,
        })
    }
    pub fn claude(home: &Path, default_home: &Path) -> Result<Self> {
        let file = Resource::File(home.join(".credentials.json"));
        #[cfg(target_os = "macos")]
        let (primary, fallback) = {
            let actual_default = std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|p| p.join(".claude"));
            let service = if home == default_home && actual_default.as_deref() == Some(home) {
                "Claude Code-credentials".into()
            } else {
                format!(
                    "Claude Code-credentials-{}",
                    &format!("{:x}", Sha256::digest(home.to_string_lossy().as_bytes()))[..8]
                )
            };
            let account = std::env::var("USER").context("system account unavailable")?;
            let keychain = Resource::Keychain { service, account };
            if keychain.read()?.is_some() || file.read()?.is_none() {
                (keychain, Some(file))
            } else {
                (file, Some(keychain))
            }
        };
        #[cfg(not(target_os = "macos"))]
        let (primary, fallback) = {
            let _ = default_home;
            (file, None)
        };
        let profile = if home == default_home {
            home.parent().unwrap_or(home).join(".claude.json")
        } else {
            home.join(".claude.json")
        };
        Ok(Self {
            tool: "claude-code".into(),
            home: home.to_owned(),
            primary,
            fallback,
            profile: Some(Resource::File(profile)),
        })
    }
    pub fn credentials(&self) -> Result<PrivateJson> {
        let bytes = self.primary.read()?.context("native sign-in missing")?;
        let value = parse(&bytes)?;
        if !value.0.is_object() {
            bail!("invalid native credential data");
        }
        Ok(value)
    }
    pub fn identity(&self) -> Result<Option<String>> {
        let Some(bytes) = self.primary.read()? else {
            return Ok(None);
        };
        let auth = parse(&bytes)?;
        if self.tool == "codex" {
            if auth.0["OPENAI_API_KEY"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
            {
                return Ok(None);
            }
            let tokens = &auth.0["tokens"];
            let claims = jwt_claims(tokens["id_token"].as_str().unwrap_or(""));
            let user = claims.0["email"]
                .as_str()
                .or_else(|| claims.0["sub"].as_str())
                .unwrap_or("");
            let workspace = tokens["account_id"].as_str().unwrap_or("");
            if user.is_empty() || workspace.is_empty() {
                bail!("native account identity unavailable");
            }
            Ok(Some(format!("{user}:{workspace}")))
        } else {
            if auth.0.get("claudeAiOauth").is_none() {
                return Ok(None);
            }
            let bytes = self
                .profile
                .as_ref()
                .context("native account profile unavailable")?
                .read()?
                .context("native account profile unavailable")?;
            let profile = parse(&bytes)?;
            Ok(Some(
                profile
                    .0
                    .pointer("/oauthAccount/emailAddress")
                    .and_then(Value::as_str)
                    .context("native account identity unavailable")?
                    .into(),
            ))
        }
    }
    pub fn expires_at(&self) -> Result<Option<i64>> {
        let auth = self.credentials()?;
        Ok(credential_expiry(&self.tool, &auth.0))
    }

    pub fn resources(&self) -> Vec<Resource> {
        let mut r = vec![self.primary.clone()];
        r.extend(self.fallback.iter().cloned());
        r.extend(self.profile.iter().cloned());
        r
    }
}

fn jwt_claims(token: &str) -> PrivateJson {
    // Decode identity claims only; the official CLI validates/refreshes the grant.
    let raw = token.split('.').nth(1).unwrap_or("");
    let alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut bits = 0u32;
    let mut count = 0u32;
    let mut bytes = Zeroizing::new(Vec::new());
    for c in raw.chars().take(16384) {
        let Some(v) = alphabet.find(c) else {
            break;
        };
        bits = (bits << 6) | v as u32;
        count += 6;
        if count >= 8 {
            count -= 8;
            bytes.push((bits >> count) as u8);
        }
    }
    parse(&bytes).unwrap_or_else(|_| PrivateJson(Value::Null))
}

pub fn codex_storage_mode(content: &str, mode: &str) -> Result<String> {
    let mut doc: toml_edit::DocumentMut = content.parse().context("invalid Codex configuration")?;
    doc["cli_auth_credentials_store"] = toml_edit::value(mode);
    Ok(doc.to_string())
}

/// Create only missing parents; never chmod an existing user-owned directory.
pub fn ensure_private_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            ensure_private_parent(parent)?;
            match fs::create_dir(parent) {
                Ok(()) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.into()),
            }
        }
        if fs::symlink_metadata(parent)?.file_type().is_symlink() {
            bail!("credential directory symlinks are unsupported");
        }
    }
    Ok(())
}

pub fn credential_expiry(tool: &str, auth: &Value) -> Option<i64> {
    if tool == "codex" {
        jwt_claims(
            auth.pointer("/tokens/access_token")
                .and_then(Value::as_str)
                .unwrap_or(""),
        )
        .0["exp"]
            .as_i64()
    } else {
        auth.pointer("/claudeAiOauth/expiresAt")
            .and_then(Value::as_i64)
            .map(|v| v / 1000)
    }
}
