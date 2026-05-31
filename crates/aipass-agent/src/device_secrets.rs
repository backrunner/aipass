use crate::paths::namespace_for_vault_dir;
use anyhow::{bail, Result};
use std::path::Path;
use zeroize::Zeroize;

const SERVICE: &str = "dev.aipass.agent";

pub fn set_webdav_password(vault_dir: &Path, password: &str) -> Result<bool> {
    set_secret(vault_dir, "sync.webdav.password", password.as_bytes())
}

pub fn get_webdav_password(vault_dir: &Path) -> Result<Option<String>> {
    Ok(match get_secret(vault_dir, "sync.webdav.password")? {
        Some(bytes) => Some(utf8_string(bytes)?),
        None => None,
    })
}

pub fn delete_webdav_password(vault_dir: &Path) -> Result<()> {
    delete_secret(vault_dir, "sync.webdav.password")
}

pub fn set_session_unlock(vault_dir: &Path, password: &str) -> Result<bool> {
    set_secret(vault_dir, "session.unlock", password.as_bytes())
}

pub fn get_session_unlock(vault_dir: &Path) -> Result<Option<String>> {
    Ok(match get_secret(vault_dir, "session.unlock")? {
        Some(bytes) => Some(utf8_string(bytes)?),
        None => None,
    })
}

pub fn delete_session_unlock(vault_dir: &Path) -> Result<()> {
    delete_secret(vault_dir, "session.unlock")
}

#[cfg(target_os = "macos")]
fn set_secret(vault_dir: &Path, purpose: &str, value: &[u8]) -> Result<bool> {
    let account = account_for(vault_dir, purpose)?;
    security_framework::passwords::set_generic_password(SERVICE, &account, value)?;
    Ok(true)
}

#[cfg(not(target_os = "macos"))]
fn set_secret(_vault_dir: &Path, _purpose: &str, _value: &[u8]) -> Result<bool> {
    Ok(false)
}

#[cfg(target_os = "macos")]
fn get_secret(vault_dir: &Path, purpose: &str) -> Result<Option<Vec<u8>>> {
    let account = account_for(vault_dir, purpose)?;
    match security_framework::passwords::get_generic_password(SERVICE, &account) {
        Ok(value) => Ok(Some(value)),
        Err(err) if err.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
        Err(err) => Err(err.into()),
    }
}

#[cfg(not(target_os = "macos"))]
fn get_secret(_vault_dir: &Path, _purpose: &str) -> Result<Option<Vec<u8>>> {
    Ok(None)
}

#[cfg(target_os = "macos")]
fn delete_secret(vault_dir: &Path, purpose: &str) -> Result<()> {
    let account = account_for(vault_dir, purpose)?;
    match security_framework::passwords::delete_generic_password(SERVICE, &account) {
        Ok(()) => Ok(()),
        Err(err) if err.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
        Err(err) => Err(err.into()),
    }
}

#[cfg(not(target_os = "macos"))]
fn delete_secret(_vault_dir: &Path, _purpose: &str) -> Result<()> {
    Ok(())
}

fn account_for(vault_dir: &Path, purpose: &str) -> Result<String> {
    Ok(format!("{}.{purpose}", namespace_for_vault_dir(vault_dir)?))
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
