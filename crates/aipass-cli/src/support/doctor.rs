use crate::*;

pub(crate) fn doctor_report(
    explicit_vault: Option<PathBuf>,
    auth_available: bool,
) -> Result<serde_json::Value> {
    let dir = vault_dir(explicit_vault)?;
    let manifest_path = dir.join("manifest.aipmanifest");
    let vault_exists = manifest_path.exists();
    let agent_status = AgentClientConfig::for_vault(dir.clone())
        .ok()
        .map(AgentClient::new)
        .and_then(|client| {
            client
                .request::<SessionStatus>(&AgentRequest::SessionStatus)
                .ok()
        });
    let native_host_binary = native_host_binary_candidate(None)?;
    let native_host_binary_status = native_host_binary_status(&native_host_binary);
    let native_host_binary_exists = native_host_binary_status.exists;
    let native_host_binary_usable = native_host_binary_status.usable;
    let native_hosts = native_host_browser_reports();
    let allowed_extension_ids = allowed_extension_ids_from_env();
    let configured_extension_ids =
        aipass_native_host::load_allowed_extension_ids().unwrap_or_default();
    let effective_extension_ids = if allowed_extension_ids.is_empty() {
        configured_extension_ids.clone()
    } else {
        allowed_extension_ids.clone()
    };
    let native_host_installed = native_hosts
        .as_array()
        .map(|items| {
            items.iter().any(|item| {
                item.get("manifestExists").and_then(|value| value.as_bool()) == Some(true)
            })
        })
        .unwrap_or(false);
    let checks = serde_json::json!([
        {
            "name": "vault_manifest",
            "ok": vault_exists,
            "message": if vault_exists { "vault manifest found" } else { "vault is not initialized" }
        },
        {
            "name": "agent",
            "ok": agent_status.is_some(),
            "message": if agent_status.is_some() { "agent responded" } else { "agent is not reachable" }
        },
        {
            "name": "native_host_binary",
            "ok": native_host_binary_usable,
            "message": if native_host_binary_usable {
                "native host binary is usable"
            } else {
                native_host_binary_status.error.as_deref().unwrap_or("native host binary is not usable")
            }
        },
        {
            "name": "native_host_manifest",
            "ok": native_host_installed,
            "message": if native_host_installed { "browser manifest installed" } else { "browser native host manifest is not installed" }
        },
        {
            "name": "extension_allowlist",
            "ok": !effective_extension_ids.is_empty(),
            "message": if effective_extension_ids.is_empty() { "extension id allowlist is empty" } else { "extension id allowlist configured" }
        }
    ]);
    Ok(serde_json::json!({
        "ok": checks
            .as_array()
            .map(|items| {
                items.iter().all(|item| {
                    item.get("ok").and_then(|value| value.as_bool()) == Some(true)
                })
            })
            .unwrap_or(false),
        "vaultDir": dir,
        "vaultManifest": manifest_path,
        "authSource": if auth_available { "env_or_flag" } else { "missing" },
        "agent": agent_status.map(|status| serde_json::json!({
            "reachable": true,
            "exists": status.exists,
            "locked": status.locked,
            "lastLockReason": status.last_lock_reason,
            "vaultNamespace": status.vault_namespace,
        })).unwrap_or_else(|| serde_json::json!({ "reachable": false })),
        "nativeHost": {
            "binaryPath": native_host_binary,
            "binaryExists": native_host_binary_exists,
            "binaryUsable": native_host_binary_usable,
            "binaryError": native_host_binary_status.error,
            "settingsPath": aipass_native_host::native_host_settings_path().ok(),
            "browsers": native_hosts,
        },
        "extensionAllowlist": {
            "env": allowed_extension_ids,
            "configured": configured_extension_ids,
            "effective": effective_extension_ids,
        },
        "checks": checks,
    }))
}

pub(crate) fn allowed_extension_ids_from_env() -> Vec<String> {
    std::env::var("AIPASS_ALLOWED_EXTENSION_IDS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

pub(crate) fn doctor_text(report: &serde_json::Value, ok: bool) -> String {
    let vault = report
        .get("vaultDir")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let agent = report
        .get("agent")
        .and_then(|value| value.get("reachable"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let native_host_count = report
        .get("nativeHost")
        .and_then(|value| value.get("browsers"))
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter(|item| {
                    item.get("manifestExists").and_then(|value| value.as_bool()) == Some(true)
                })
                .count()
        })
        .unwrap_or(0);
    let allowlist_count = report
        .get("extensionAllowlist")
        .and_then(|value| value.get("effective"))
        .and_then(|value| value.as_array())
        .map(Vec::len)
        .unwrap_or(0);
    format!(
        "AIPass doctor: {}\nVault: {vault}\nAgent: {}\nNative host manifests: {native_host_count}\nExtension allowlist ids: {allowlist_count}",
        if ok { "ok" } else { "issues found" },
        if agent { "reachable" } else { "not reachable" }
    )
}
