use crate::paths::namespace_for_vault_dir;
use anyhow::{bail, Result};
#[cfg(target_os = "macos")]
use std::collections::HashMap;
use std::path::Path;
#[cfg(target_os = "macos")]
use std::sync::{Mutex, OnceLock};
use zeroize::Zeroize;

const SERVICE: &str = "dev.aipass.agent";
const WEBDAV_PASSWORD_PURPOSE: &str = "sync.webdav.password";
const SESSION_UNLOCK_PURPOSE: &str = "session.unlock";

#[cfg(target_os = "macos")]
enum CachedSecret {
    Missing,
    Present(Vec<u8>),
}

#[cfg(target_os = "macos")]
impl Drop for CachedSecret {
    fn drop(&mut self) {
        if let Self::Present(value) = self {
            value.zeroize();
        }
    }
}

#[cfg(target_os = "macos")]
static SECRET_CACHE: OnceLock<Mutex<HashMap<String, CachedSecret>>> = OnceLock::new();

pub fn set_webdav_password(vault_dir: &Path, password: &str) -> Result<bool> {
    set_secret(vault_dir, WEBDAV_PASSWORD_PURPOSE, password.as_bytes())
}

pub fn get_webdav_password(vault_dir: &Path) -> Result<Option<String>> {
    Ok(match get_secret(vault_dir, WEBDAV_PASSWORD_PURPOSE)? {
        Some(bytes) => Some(utf8_string(bytes)?),
        None => None,
    })
}

pub fn delete_webdav_password(vault_dir: &Path) -> Result<()> {
    delete_secret(vault_dir, WEBDAV_PASSWORD_PURPOSE)
}

pub fn set_session_unlock(vault_dir: &Path, password: &str) -> Result<bool> {
    set_secret(vault_dir, SESSION_UNLOCK_PURPOSE, password.as_bytes())
}

pub fn get_session_unlock(vault_dir: &Path) -> Result<Option<String>> {
    Ok(match get_secret(vault_dir, SESSION_UNLOCK_PURPOSE)? {
        Some(bytes) => Some(utf8_string(bytes)?),
        None => None,
    })
}

pub fn delete_session_unlock(vault_dir: &Path) -> Result<()> {
    delete_secret(vault_dir, SESSION_UNLOCK_PURPOSE)
}

#[cfg(all(target_os = "macos", not(test)))]
fn set_secret(vault_dir: &Path, purpose: &str, value: &[u8]) -> Result<bool> {
    let account = account_for(vault_dir, purpose)?;
    if should_cache_value(purpose) {
        if let Some(cached) = cache_get(&account) {
            if cached.as_deref() == Some(value) {
                return Ok(true);
            }
        }
    }
    security_framework::passwords::set_generic_password(SERVICE, &account, value)?;
    if should_cache_value(purpose) {
        cache_put(&account, Some(value));
    }
    Ok(true)
}

#[cfg(all(target_os = "macos", test))]
fn set_secret(vault_dir: &Path, purpose: &str, value: &[u8]) -> Result<bool> {
    let account = account_for(vault_dir, purpose)?;
    cache_put(&account, Some(value));
    Ok(true)
}

#[cfg(not(target_os = "macos"))]
fn set_secret(_vault_dir: &Path, _purpose: &str, _value: &[u8]) -> Result<bool> {
    Ok(false)
}

#[cfg(all(target_os = "macos", not(test)))]
fn get_secret(vault_dir: &Path, purpose: &str) -> Result<Option<Vec<u8>>> {
    let account = account_for(vault_dir, purpose)?;
    if should_cache_value(purpose) {
        if let Some(cached) = cache_get(&account) {
            return Ok(cached);
        }
    }
    let value = match security_framework::passwords::get_generic_password(SERVICE, &account) {
        Ok(value) => Some(value),
        Err(err) if err.code() == ERR_SEC_ITEM_NOT_FOUND => None,
        Err(err) => return Err(err.into()),
    };
    if should_cache_value(purpose) {
        cache_put(&account, value.as_deref());
    }
    Ok(value)
}

#[cfg(all(target_os = "macos", test))]
fn get_secret(vault_dir: &Path, purpose: &str) -> Result<Option<Vec<u8>>> {
    let account = account_for(vault_dir, purpose)?;
    Ok(cache_get(&account).unwrap_or(None))
}

#[cfg(not(target_os = "macos"))]
fn get_secret(_vault_dir: &Path, _purpose: &str) -> Result<Option<Vec<u8>>> {
    Ok(None)
}

#[cfg(all(target_os = "macos", not(test)))]
fn delete_secret(vault_dir: &Path, purpose: &str) -> Result<()> {
    let account = account_for(vault_dir, purpose)?;
    if should_cache_value(purpose) && matches!(cache_get(&account), Some(None)) {
        return Ok(());
    }
    match security_framework::passwords::delete_generic_password(SERVICE, &account) {
        Ok(()) => {
            if should_cache_value(purpose) {
                cache_put(&account, None);
            }
            Ok(())
        }
        Err(err) if err.code() == ERR_SEC_ITEM_NOT_FOUND => {
            if should_cache_value(purpose) {
                cache_put(&account, None);
            }
            Ok(())
        }
        Err(err) => Err(err.into()),
    }
}

#[cfg(all(target_os = "macos", test))]
fn delete_secret(vault_dir: &Path, purpose: &str) -> Result<()> {
    let account = account_for(vault_dir, purpose)?;
    cache_put(&account, None);
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn delete_secret(_vault_dir: &Path, _purpose: &str) -> Result<()> {
    Ok(())
}

fn account_for(vault_dir: &Path, purpose: &str) -> Result<String> {
    Ok(format!("{}.{purpose}", namespace_for_vault_dir(vault_dir)?))
}

#[cfg(all(target_os = "macos", not(test)))]
fn should_cache_value(purpose: &str) -> bool {
    purpose == WEBDAV_PASSWORD_PURPOSE
}

#[cfg(target_os = "macos")]
fn cache_get(account: &str) -> Option<Option<Vec<u8>>> {
    let cache = SECRET_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let guard = cache.lock().ok()?;
    guard.get(account).map(|secret| match secret {
        CachedSecret::Missing => None,
        CachedSecret::Present(value) => Some(value.clone()),
    })
}

#[cfg(target_os = "macos")]
fn cache_put(account: &str, value: Option<&[u8]>) {
    let cache = SECRET_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut guard) = cache.lock() else {
        return;
    };
    let replacement = match value {
        Some(value) => CachedSecret::Present(value.to_vec()),
        None => CachedSecret::Missing,
    };
    guard.insert(account.to_string(), replacement);
}

#[cfg(target_os = "macos")]
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

fn utf8_string(bytes: Vec<u8>) -> Result<String> {
    match String::from_utf8(bytes) {
        Ok(value) => Ok(value),
        Err(err) => {
            let mut bytes = err.into_bytes();
            bytes.zeroize();
            bail!("stored webdav password is not valid utf-8")
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_backend_caches_present_and_missing_secrets() {
        let vault = tempdir().expect("temp vault");

        assert!(get_session_unlock(vault.path()).unwrap().is_none());
        assert!(set_session_unlock(vault.path(), "session password").unwrap());
        assert_eq!(
            get_session_unlock(vault.path()).unwrap().as_deref(),
            Some("session password")
        );

        delete_session_unlock(vault.path()).unwrap();
        assert!(get_session_unlock(vault.path()).unwrap().is_none());
    }
}
