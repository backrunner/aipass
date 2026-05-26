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
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use uuid::Uuid;

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
        Ok(ListenerOptions::new()
            .name(name)
            .try_overwrite(true)
            .reclaim_name(true)
            .create_sync()?)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let path = agent_runtime_dir()?.join(format!("{namespace}.sock"));
        let name = path.clone().to_fs_name::<GenericFilePath>()?;
        #[allow(unused_mut)]
        let mut options = ListenerOptions::new()
            .name(name)
            .try_overwrite(true)
            .reclaim_name(true);
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "freebsd"))]
        {
            options = options.mode(0o600);
        }
        let listener = options.create_sync()?;
        #[cfg(unix)]
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        Ok(listener)
    }
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
    match OpenOptions::new().write(true).create_new(true).open(&path) {
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
