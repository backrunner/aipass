use crate::paths::{agent_runtime_dir, namespace_for_vault_dir};
use aipass_agent_protocol::SensitiveString;
use anyhow::Result;
#[cfg(target_os = "windows")]
use interprocess::local_socket::GenericNamespaced;
use interprocess::local_socket::{prelude::*, GenericFilePath, Listener, ListenerOptions, Stream};
#[cfg(any(target_os = "linux", target_os = "android", target_os = "freebsd"))]
use interprocess::os::unix::local_socket::ListenerOptionsExt;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use uuid::Uuid;

const MAX_AUTH_TOKEN_BYTES: u64 = 4096;

pub fn connect(vault_dir: impl AsRef<Path>) -> Result<Stream> {
    let namespace = namespace_for_vault_dir(vault_dir)?;
    #[cfg(target_os = "windows")]
    {
        let name = format!("dev.aipass.agent.{namespace}").to_ns_name::<GenericNamespaced>()?;
        Ok(Stream::connect(name)?)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let path = agent_runtime_dir()?.join(format!("{namespace}.sock"));
        let name = path.to_fs_name::<GenericFilePath>()?;
        Ok(Stream::connect(name)?)
    }
}

pub fn listen(vault_dir: impl AsRef<Path>) -> Result<Listener> {
    let namespace = namespace_for_vault_dir(vault_dir)?;
    #[cfg(target_os = "windows")]
    {
        let name = format!("dev.aipass.agent.{namespace}").to_ns_name::<GenericNamespaced>()?;
        Ok(ListenerOptions::new().name(name).create_sync()?)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let path = agent_runtime_dir()?.join(format!("{namespace}.sock"));
        listen_at_path(&path)
    }
}

#[cfg(not(target_os = "windows"))]
fn listen_at_path(path: &Path) -> Result<Listener> {
    match create_listener_at_path(path) {
        Ok(listener) => Ok(listener),
        Err(err) if err.kind() == ErrorKind::AddrInUse && stale_socket_path(path) => {
            let _ = fs::remove_file(path);
            Ok(create_listener_at_path(path)?)
        }
        Err(err) => Err(err.into()),
    }
}

#[cfg(not(target_os = "windows"))]
fn create_listener_at_path(path: &Path) -> std::io::Result<Listener> {
    let name = path.to_path_buf().to_fs_name::<GenericFilePath>()?;
    #[allow(unused_mut)]
    let mut options = ListenerOptions::new().name(name);
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "freebsd"))]
    {
        options = options.mode(0o600);
    }
    let listener = options.create_sync()?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

#[cfg(not(target_os = "windows"))]
fn stale_socket_path(path: &Path) -> bool {
    let Ok(name) = path.to_path_buf().to_fs_name::<GenericFilePath>() else {
        return false;
    };
    Stream::connect(name).is_err()
}

pub fn load_or_create_auth_token(vault_dir: impl AsRef<Path>) -> Result<SensitiveString> {
    let path = auth_token_path(vault_dir)?;
    if let Ok(token) = read_auth_token_at_path(&path) {
        return Ok(token);
    }

    let token = SensitiveString::new(format!(
        "{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    match options.open(&path) {
        Ok(mut file) => {
            #[cfg(unix)]
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
            file.write_all(token.expose().as_bytes())?;
            file.sync_all()?;
            Ok(token)
        }
        Err(err) if err.kind() == ErrorKind::AlreadyExists => read_auth_token_at_path(&path),
        Err(err) => Err(err.into()),
    }
}

pub fn read_auth_token(vault_dir: impl AsRef<Path>) -> Result<SensitiveString> {
    read_auth_token_at_path(&auth_token_path(vault_dir)?)
}

pub fn clear_auth_token(vault_dir: impl AsRef<Path>) -> Result<()> {
    let path = auth_token_path(vault_dir)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn auth_token_path(vault_dir: impl AsRef<Path>) -> Result<std::path::PathBuf> {
    let namespace = namespace_for_vault_dir(vault_dir)?;
    Ok(agent_runtime_dir()?.join(format!("{namespace}.token")))
}

fn read_auth_token_at_path(path: &Path) -> Result<SensitiveString> {
    secure_existing_auth_token(path)?;
    if fs::metadata(path)?.len() > MAX_AUTH_TOKEN_BYTES {
        anyhow::bail!("agent auth token is too large");
    }
    let bytes = fs::read(path)?;
    let mut value = String::from_utf8(bytes)?;
    while value.ends_with(['\n', '\r']) {
        value.pop();
    }
    if value.is_empty() {
        anyhow::bail!("agent auth token is empty");
    }
    Ok(SensitiveString::new(value))
}

#[cfg(unix)]
fn secure_existing_auth_token(path: &Path) -> Result<()> {
    let metadata = fs::metadata(path)?;
    if metadata.permissions().mode() & 0o177 != 0 {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn secure_existing_auth_token(_: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn oversized_auth_tokens_are_rejected() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("token");
        fs::write(&path, "x".repeat((MAX_AUTH_TOKEN_BYTES + 1) as usize)).expect("write token");

        let err = read_auth_token_at_path(&path).unwrap_err();
        assert_eq!(err.to_string(), "agent auth token is too large");
    }

    #[cfg(unix)]
    #[test]
    fn existing_auth_token_permissions_are_repaired() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("token");
        fs::write(&path, "secret").expect("write token");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("chmod token");

        let token = read_auth_token_at_path(&path).expect("read token");

        assert_eq!(token.expose(), "secret");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn listener_does_not_overwrite_live_agent_socket() {
        let runtime = tempdir().expect("runtime");
        let path = runtime.path().join("agent.sock");

        let first = listen_at_path(&path).expect("first listener");
        let err = listen_at_path(&path).unwrap_err();

        drop(first);
        assert!(err
            .downcast_ref::<std::io::Error>()
            .is_some_and(|err| err.kind() == ErrorKind::AddrInUse));
    }
}
