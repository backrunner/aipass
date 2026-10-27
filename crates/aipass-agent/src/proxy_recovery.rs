//! Installation-owned proxy receipts, usable without unlocking the vault.
//! The device key authorizes only reversal of explicit external config writes.
mod device_key;
mod installations;
use aipass_config_writers::{proxy_recovery::Recovery, transaction::Change, ConfigPlan, ToolId};
use anyhow::Result;
use std::path::{Path, PathBuf};

pub(crate) struct WriteGuard {
    recovery: Recovery,
    owner: PathBuf,
}
impl WriteGuard {
    pub(crate) fn begin(home: &Path, proxy: bool) -> Result<Option<Self>> {
        Self::open_for(home, proxy, false)
    }
    fn open_for(home: &Path, proxy: bool, audit: bool) -> Result<Option<Self>> {
        // Unit tests never consult a real user profile or system keychain.
        if cfg!(test) || !cfg!(any(target_os = "macos", windows)) {
            return Ok(None);
        }
        let root = home.join(".aipass/proxy-recovery");
        if !proxy && !root.exists() {
            return Ok(None);
        }
        let owner = crate::canonical_vault_dir(std::env::current_exe()?)?;
        let recovery = open(&root)?;
        recovery.ensure_writable()?;
        if !audit {
            recovery.ensure_attached(&owner)?;
        }
        Ok(Some(Self { recovery, owner }))
    }
    pub(crate) fn record_plan(
        &mut self,
        vault: &Path,
        plan: &ConfigPlan,
        content: &str,
    ) -> Result<()> {
        self.require_audit(vault)?;
        self.recovery.record(
            &self.owner,
            vault,
            plan.tool.clone(),
            Recovery::plan_changes(plan, content)?,
        )
    }
    pub(crate) fn record(
        &mut self,
        vault: &Path,
        tool: ToolId,
        changes: Vec<Change>,
        sources: Vec<aipass_config_writers::proxy_recovery::NativeSource>,
    ) -> Result<()> {
        self.require_audit(vault)?;
        self.recovery
            .record_native(&self.owner, vault, tool, changes, sources)
    }
    fn require_audit(&self, vault: &Path) -> Result<()> {
        anyhow::ensure!(self.recovery.vaults(&self.owner).iter().any(|v| v == vault), "proxy restoration history is not verified; lock and unlock AIPass before configuring Agents");
        Ok(())
    }
    pub(crate) fn commit(&mut self) {
        if self.recovery.commit().is_err() {
            crate::logging::write_component_log(
                crate::logging::AGENT_LOG,
                "WARN",
                "event=proxy.uninstall.compaction_pending",
            );
        }
    }
    pub(crate) fn retire(
        &mut self,
        plan: &ConfigPlan,
        extra: &[aipass_config_writers::native_auth::Resource],
    ) -> Result<()> {
        use aipass_config_writers::native_auth::Resource;
        let resources: Vec<_> = std::iter::once(Resource::File(plan.target_path.clone()))
            .chain(
                plan.extra_writes
                    .iter()
                    .map(|w| Resource::File(w.target_path.clone())),
            )
            .chain(extra.iter().cloned())
            .collect();
        self.recovery.retire(&self.owner, &plan.tool, &resources)
    }
}
fn open(root: &Path) -> Result<Recovery> {
    Recovery::open(root, |required| device_key::load(root, required))
}
pub(crate) fn audit_session(state: &std::sync::Arc<crate::session::AgentState>) {
    if audit(state).is_err() {
        crate::logging::write_component_log(
            crate::logging::AGENT_LOG,
            "WARN",
            "event=proxy.uninstall.audit_pending",
        );
    }
}

pub(crate) fn audit(
    state: &std::sync::Arc<crate::session::AgentState>,
) -> crate::session::ServiceResult<()> {
    use crate::session::{with_vault, ServiceError};
    use aipass_config_writers::native_auth::Resource;
    if cfg!(test) || !cfg!(any(target_os = "macos", windows)) {
        return Ok(());
    }
    with_vault(state, false, |vault| {
        let home = crate::server::home_dir(vault)?;
        let Some(mut guard) =
            WriteGuard::open_for(&home, true, true).map_err(ServiceError::internal)?
        else {
            return Ok(());
        };
        let config = state
            .proxy
            .lock()
            .map_err(|_| ServiceError::internal(anyhow::anyhow!("proxy unavailable")))?
            .config(vault)?;
        let mut legacy = Vec::new();
        for detection in aipass_config_writers::detect_tools_in(Some(&home), &[]) {
            let tool = crate::server::tool_config_tool_for(&detection.tool);
            let mut paths = crate::tool_credentials::config_paths(&home, &tool);
            if let Some(path) = detection.config_path {
                if path.is_file() && !paths.contains(&path) {
                    paths.push(path);
                }
            }
            let native_proxy = if crate::tool_switch::supported(&tool) {
                crate::tool_switch::has_proxy_binding(state, vault, &tool, &paths)
                    .map_err(ServiceError::internal)?
            } else {
                false
            };
            let mut resources = Vec::new();
            for (index, path) in paths.into_iter().enumerate() {
                let resource = Resource::File(path);
                if guard.recovery.tracked(
                    &guard.owner,
                    &detection.tool,
                    std::slice::from_ref(&resource),
                ) {
                    continue;
                }
                if let Some(raw) = resource.read().map_err(ServiceError::internal)? {
                    let found = (native_proxy && index == 0)
                        || config.routes.iter().any(|r| {
                            !r.token.is_empty()
                                && raw.windows(r.token.len()).any(|w| w == r.token.as_bytes())
                        });
                    if found {
                        resources.push(resource);
                    }
                }
            }
            if !resources.is_empty() {
                legacy.push((detection.tool, resources));
            }
        }
        guard
            .recovery
            .audit(&guard.owner, &state.vault_dir, legacy)
            .map_err(ServiceError::internal)
    })
}

/// Invoked exclusively by an explicit uninstall command, never quit, autostart
/// removal, file disappearance, or updater shutdown. No vault is opened here.
pub fn prepare_uninstall(agent: &Path) -> Result<()> {
    anyhow::ensure!(
        cfg!(any(target_os = "macos", windows)),
        "proxy uninstall recovery is supported on macOS and Windows"
    );
    let home = aipass_config_writers::user_home_dir()
        .ok_or_else(|| anyhow::anyhow!("user home unavailable"))?;
    let root = home.join(".aipass/proxy-recovery");
    let required_vaults = installations::vaults(agent, &home)?;
    if !root.exists() {
        anyhow::ensure!(
            required_vaults.is_empty(),
            "no verified proxy restoration history; open and unlock AIPass before uninstalling"
        );
        return Ok(());
    }
    let owner = crate::canonical_vault_dir(agent)?;
    let mut recovery = open(&root)?;
    let vaults = recovery.vaults(&owner);
    anyhow::ensure!(required_vaults.iter().all(|v| vaults.contains(v)), "a registered vault has no verified restoration history; open and unlock it before uninstalling");
    let restored = recovery.restore(&owner)?;
    // Keep the writer lock until services have stopped. A racing writer fails
    // instead of reinstalling the proxy after successful restoration.
    for vault in vaults {
        let _ = &vault;
        #[cfg(target_os = "macos")]
        crate::autostart::uninstall_owned_macos_agent(agent, &vault)?;
    }
    #[cfg(windows)]
    crate::autostart::uninstall_windows_tray_entries(&agent.with_file_name("aipass-desktop.exe"))?;
    recovery.finish(&owner)?;
    crate::logging::write_component_log(
        crate::logging::AGENT_LOG,
        "INFO",
        &format!("event=proxy.uninstall.restored resources={restored}"),
    );
    Ok(())
}
