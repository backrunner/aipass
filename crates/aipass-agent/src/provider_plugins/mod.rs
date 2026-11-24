//! Device-local, per-vault trust grants for explicitly selected single-file providers.
use aipass_agent_protocol::SubscriptionPluginAction;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, Weak},
};

use crate::subscriptions::Operation;

type Result<T> = std::result::Result<T, String>;
const FILE_LIMIT: u64 = 64 * 1024 * 1024;

pub(crate) fn is_plugin(id: &str) -> bool {
    id.strip_prefix("plugin.").is_some_and(|suffix| {
        !suffix.is_empty()
            && suffix.len() <= 80
            && suffix.split('.').all(|part| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
    })
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Manifest {
    protocol: u32,
    pub id: String,
    pub name: String,
    version: String,
    entry: String,
    interpreter: Option<PathBuf>,
    pub runtime: Option<Runtime>,
    target: Option<String>,
    #[serde(default)]
    pub origins: Vec<String>,
    pub methods: Vec<Method>,
}

mod binary;
mod form;
pub(crate) mod github;
mod install;
#[cfg(all(test, unix))]
mod install_tests;
pub(crate) mod network;
pub(crate) mod sandbox;

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Runtime {
    Wasi,
    NativeSandbox,
}

fn fingerprint(raw: &[u8], program: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(Sha256::digest(raw));
    hash.update(Sha256::digest(program));
    format!("{:x}", hash.finalize())
}
use form::Method;

fn label(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 256 && !s.chars().any(|c| c.is_control() || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
}
impl Manifest {
    fn valid(&self) -> bool {
        self.protocol == 1
            && is_plugin(&self.id)
            && label(&self.name)
            && label(&self.version)
            && label(&self.entry)
            && matches!(
                Path::new(&self.entry)
                    .components()
                    .collect::<Vec<_>>()
                    .as_slice(),
                [Component::Normal(_)]
            )
            && !self.entry.contains(['/', '\\', ':'])
            && self
                .interpreter
                .as_ref()
                .is_none_or(|p| p.is_absolute() && p.is_file())
            && (self.runtime.is_none() || self.interpreter.is_none())
            && (self.runtime == Some(Runtime::NativeSandbox) || self.target.is_none())
            && self.origins.len() <= 16
            && !self
                .origins
                .iter()
                .enumerate()
                .any(|(i, origin)| self.origins[..i].contains(origin))
            && (self.runtime.is_some() || self.origins.is_empty())
            && self.origins.iter().all(|v| network::valid_origin(v))
            && (1..=8).contains(&self.methods.len())
            && self.methods.iter().all(Method::valid)
    }
    fn catalog(&self, digest: &str) -> Value {
        let methods: Vec<_> = self
            .methods
            .iter()
            .enumerate()
            .map(|(i, m)| m.catalog(i))
            .collect();
        json!({"id":self.id,"name":self.name,"methods":methods,"localAuthorization":true,"plugin":true,"pluginDigest":digest})
    }
}

#[derive(Serialize, Deserialize)]
struct Grant {
    path: PathBuf,
    origin: Option<github::Origin>,
    digest: String,
    manifest: Manifest,
}
impl Grant {
    fn summary(&self) -> Value {
        let migrated = self.manifest.runtime.is_none() && self.manifest.interpreter.is_none();
        let runtime = self
            .manifest
            .runtime
            .or(migrated.then_some(Runtime::NativeSandbox));
        let target = self.manifest.target.clone().or_else(|| {
            migrated.then(|| format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH))
        });
        json!({"id":self.manifest.id,"name":self.manifest.name,"version":self.manifest.version,"digest":self.digest,
            "path":self.path,"runtime":runtime,"origins":self.manifest.origins,"origin":self.origin,"target":target,"entry":self.manifest.entry,"interpreter":self.manifest.interpreter})
    }
}

fn read(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
    }
    let file = options
        .open(path)
        .map_err(|_| "Plugin file missing or exceeds size limit")?;
    if !file
        .metadata()
        .is_ok_and(|m| m.is_file() && m.len() <= limit)
    {
        return Err("Plugin file missing or exceeds size limit".into());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read plugin file")?;
    if bytes.len() as u64 > limit {
        return Err("Plugin file exceeds size limit".into());
    }
    Ok(bytes)
}
fn read_package(path: &Path) -> Result<(Grant, Vec<u8>)> {
    if !path.is_absolute() {
        return Err("Select an absolute plugin manifest path".into());
    }
    let path = path
        .canonicalize()
        .map_err(|_| "Plugin manifest unavailable")?;
    let raw = read(&path, 64 * 1024)?;
    let mut manifest: Manifest =
        serde_json::from_slice(&raw).map_err(|_| "Invalid plugin manifest")?;
    if !manifest.valid() {
        return Err("Unsupported plugin manifest".into());
    }
    let entry = path
        .with_file_name(&manifest.entry)
        .canonicalize()
        .map_err(|_| "Plugin file missing or exceeds size limit")?;
    if entry == path || entry.parent() != path.parent() {
        return Err("Invalid plugin entry".into());
    }
    let program = read(&entry, FILE_LIMIT)?;
    // Older local single-binary manifests also enter the sandbox.
    if manifest.runtime.is_none() && manifest.interpreter.is_none() {
        if program.starts_with(b"\0asm") {
            manifest.runtime = Some(Runtime::Wasi);
        } else {
            manifest.runtime = Some(Runtime::NativeSandbox);
            manifest.target = Some(format!(
                "{}-{}",
                std::env::consts::OS,
                std::env::consts::ARCH
            ));
        }
    }
    Ok((
        Grant {
            path,
            origin: None,
            digest: fingerprint(&raw, &program),
            manifest,
        },
        program,
    ))
}

fn inspect(path: &Path) -> Result<(Grant, Vec<u8>)> {
    let (grant, program) = read_package(path)?;
    sandbox::validate(&grant.manifest, &program)?;
    Ok((grant, program))
}

/// Each invocation executes the exact bytes inspected, in a new private directory.
pub(crate) struct Launch {
    pub manifest: Manifest,
    pub digest: String,
    pub dir: tempfile::TempDir,
    pub command: PathBuf,
    pub arguments: Vec<std::ffi::OsString>,
}

#[derive(Default)]
pub(crate) struct Registry {
    pending: Option<github::Preview>,
    checking: Option<Weak<install::Check>>,
    active: Vec<(String, Weak<Operation>)>,
}
fn root(vault: &Path) -> Result<PathBuf> {
    let dirs = crate::paths::project_dirs().map_err(|_| "Local plugin directory unavailable")?;
    let namespace =
        crate::paths::namespace_for_vault_dir(vault).map_err(|_| "Invalid vault path")?;
    Ok(dirs
        .data_local_dir()
        .join("subscription-providers")
        .join(namespace))
}
fn grants(root: &Path) -> Result<Vec<Grant>> {
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("Plugin registry unavailable".into()),
    };
    let mut records = Vec::new();
    for item in entries.flatten() {
        let path = item.path();
        if path.extension().and_then(|v| v.to_str()) != Some("json") {
            continue;
        }
        if let Ok(raw) = read(&path, 128 * 1024) {
            if let Ok(g) = serde_json::from_slice::<Grant>(&raw) {
                if is_plugin(&g.manifest.id)
                    && path.file_stem().and_then(|s| s.to_str()) == Some(g.manifest.id.as_str())
                {
                    records.push(g);
                }
            }
        }
    }
    records.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
    Ok(records)
}
// Restrict new directories from the moment they are created, even with a permissive umask.
fn create_registry_dir(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|_| "Cannot open local plugin directory".into())
}

fn private_stage(parent: Option<&Path>) -> Result<tempfile::TempDir> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("provider-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    match parent {
        Some(path) => builder.tempdir_in(path),
        None => builder.tempdir(),
    }
    .map_err(|_| "Cannot prepare plugin directory".into())
}

fn save_grant(root: &Path, g: &Grant) -> Result<()> {
    create_registry_dir(root)?;
    let mut tmp =
        tempfile::NamedTempFile::new_in(root).map_err(|_| "Cannot create plugin grant")?;
    serde_json::to_writer(tmp.as_file_mut(), &g).map_err(|_| "Cannot write plugin grant")?;
    tmp.as_file()
        .sync_all()
        .map_err(|_| "Cannot save plugin grant")?;
    tmp.persist_noclobber(root.join(format!("{}.json", g.manifest.id)))
        .map_err(|_| "Disable the existing plugin before enabling a replacement")?;
    Ok(())
}

impl Registry {
    pub fn preview(&mut self, preview: github::Preview) -> Value {
        let summary = preview.summary();
        self.pending = Some(preview);
        summary
    }

    pub fn catalog(&self, vault: &Path) -> Result<Vec<Value>> {
        Ok(grants(&root(vault)?)?
            .iter()
            .map(|g| g.manifest.catalog(&g.digest))
            .collect())
    }
    pub fn manage(&mut self, vault: &Path, action: SubscriptionPluginAction) -> Result<Value> {
        use SubscriptionPluginAction::*;
        let root = root(vault)?;
        match action {
            List => Ok(json!(grants(&root)?
                .iter()
                .map(Grant::summary)
                .collect::<Vec<_>>())),
            Inspect { path } => Ok(inspect(&path)?.0.summary()),
            Enable { .. } | EnableGithub { .. } | InspectGithub { .. } => {
                Err("Plugin verification required".into())
            }
            Disable { id } => {
                if !is_plugin(&id) {
                    return Err("Invalid plugin id".into());
                }
                let installed = grants(&root)?
                    .into_iter()
                    .find(|g| g.manifest.id == id && g.origin.is_some());
                match fs::remove_file(root.join(format!("{id}.json"))) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                        return Err("Cannot disable plugin".into())
                    }
                    _ => {}
                }
                self.cancel(Some(&id));
                if let Some(parent) = installed.as_ref().and_then(|g| g.path.parent()) {
                    if parent.parent() == Some(&root.join("packages")) {
                        fs::remove_dir_all(parent)
                            .map_err(|_| "Plugin disabled but package cleanup failed")?;
                    }
                }
                Ok(Value::Null)
            }
        }
    }
    pub fn cancel(&mut self, only: Option<&str>) {
        if only.is_none() {
            self.pending = None;
        }
        if let Some(check) = self.checking.as_ref().and_then(Weak::upgrade) {
            if only.is_none_or(|id| id == check.id) {
                check.cancel.send_replace(true);
            }
        }
        let mut stopping = Vec::new();
        self.active.retain(|(id, weak)| {
            let Some(op) = weak.upgrade() else {
                return false;
            };
            if only.is_some_and(|selected| selected != id) {
                return true;
            }
            op.kill();
            stopping.push(op);
            false
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        for op in stopping {
            op.wait_for_plugin_exit(deadline);
        }
    }
    pub fn track(&mut self, id: &str, op: &Arc<Operation>) {
        self.active.retain(|(_, op)| op.strong_count() > 0);
        self.active.push((id.into(), Arc::downgrade(op)));
    }
    pub fn launch(&self, vault: &Path, id: &str) -> Result<Launch> {
        if !is_plugin(id) {
            return Err("Invalid plugin id".into());
        }
        if self
            .active
            .iter()
            .filter(|(_, op)| op.upgrade().is_some_and(|v| v.running()))
            .count()
            >= 16
        {
            return Err("Too many active plugin operations".into());
        }
        let bytes = read(&root(vault)?.join(format!("{id}.json")), 128 * 1024)
            .map_err(|_| "Subscription plugin is disabled on this device")?;
        let approved: Grant = serde_json::from_slice(&bytes).map_err(|_| "Invalid plugin grant")?;
        let (g, program) = read_package(&approved.path)?;
        if g.manifest.id != id
            || g.digest != approved.digest
            || (approved.origin.is_some() && g.manifest.runtime.is_none())
        {
            return Err("Plugin changed; review and enable the new version".into());
        }
        stage(&g, &program)
    }
}

fn stage(g: &Grant, program: &[u8]) -> Result<Launch> {
    let dir = private_stage(None)?;
    let entry = dir.path().join(&g.manifest.entry);
    let mut file = fs::File::create_new(&entry).map_err(|_| "Cannot prepare plugin executable")?;
    file.write_all(program)
        .map_err(|_| "Cannot prepare plugin executable")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o700))
            .map_err(|_| "Cannot set plugin permissions")?;
    }
    let (command, arguments) = sandbox::command(&g.manifest, &entry)?;
    Ok(Launch {
        manifest: g.manifest.clone(),
        digest: g.digest.clone(),
        dir,
        command,
        arguments,
    })
}
