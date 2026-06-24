use anyhow::{anyhow, Result};
use std::env;
use std::path::{Path, PathBuf};
#[cfg(not(target_os = "windows"))]
use std::process::Command;

const AGENT_BINARY_ENV: &str = "AIPASS_AGENT_BINARY";
const AGENT_PATH_ENV: &str = "AIPASS_AGENT_PATH";

#[derive(Clone, Debug)]
pub struct AgentLaunch {
    pub binary: PathBuf,
    pub candidates: Vec<PathBuf>,
}

#[derive(Clone, Debug, Default)]
struct AgentBinarySearch {
    candidates: Vec<PathBuf>,
    selected: Option<PathBuf>,
    env_override: Option<(String, PathBuf)>,
}

pub fn agent_binary_path() -> Result<PathBuf> {
    let search = find_agent_binary();
    search
        .selected
        .clone()
        .ok_or_else(|| anyhow!(agent_binary_not_found_message(&search)))
}

pub fn agent_binary_candidates() -> Vec<PathBuf> {
    find_agent_binary().candidates
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn launch_agent(
    vault_dir: &Path,
    namespace: &str,
    initial_connection_error: &str,
) -> Result<AgentLaunch> {
    let search = find_agent_binary();
    let Some(binary) = search.selected.clone() else {
        return Err(anyhow!(agent_launch_failure_message(
            vault_dir,
            namespace,
            &search,
            None,
            initial_connection_error,
        )));
    };

    match Command::new(&binary).arg("--vault").arg(vault_dir).spawn() {
        Ok(_) => Ok(AgentLaunch {
            binary,
            candidates: search.candidates,
        }),
        Err(err) => Err(anyhow!(agent_launch_failure_message(
            vault_dir,
            namespace,
            &search,
            Some((&binary, &err.to_string())),
            initial_connection_error,
        ))),
    }
}

pub(crate) fn agent_ready_timeout_message(
    vault_dir: &Path,
    namespace: &str,
    launched_binary: Option<&Path>,
    candidates: &[PathBuf],
    initial_connection_error: &str,
    last_connection_error: Option<&str>,
) -> String {
    let mut lines = vec![
        "AIPass agent was launched but did not become ready in time.".to_string(),
        format!("Vault: {}", vault_dir.display()),
        format!("Namespace: {namespace}"),
    ];
    if let Some(binary) = launched_binary {
        lines.push(format!("Launched binary: {}", binary.display()));
    }
    lines.push(format!("Tried binaries: {}", format_path_list(candidates)));
    lines.push(format!(
        "Initial connection error: {initial_connection_error}"
    ));
    if let Some(error) = last_connection_error {
        lines.push(format!("Last connection error: {error}"));
    }
    lines.push(agent_recovery_hint().to_string());
    lines.join("\n")
}

#[cfg(target_os = "windows")]
pub(crate) fn windows_service_start_failure_message(
    vault_dir: &Path,
    namespace: &str,
    initial_connection_error: &str,
    service_error: &str,
) -> String {
    [
        "AIPass agent service is not running and Windows could not start it.".to_string(),
        format!("Vault: {}", vault_dir.display()),
        format!("Namespace: {namespace}"),
        format!("Service start error: {service_error}"),
        format!("Initial connection error: {initial_connection_error}"),
        "Run `aipass agent install` with permission to register/start the Windows service, then retry.".to_string(),
    ]
    .join("\n")
}

fn find_agent_binary() -> AgentBinarySearch {
    let mut search = AgentBinarySearch::default();

    for env_name in [AGENT_BINARY_ENV, AGENT_PATH_ENV] {
        let Some(value) = env::var_os(env_name).filter(|value| !value.is_empty()) else {
            continue;
        };
        let path = absolute_path(PathBuf::from(value));
        search.env_override = Some((env_name.to_string(), path.clone()));
        add_candidate(&mut search, path.clone());
        if path.is_file() {
            search.selected = Some(path);
        }
        return search;
    }

    let name = agent_binary_name();

    if let Ok(current) = env::current_exe() {
        add_candidate(&mut search, current.with_file_name(name));
        if let Some(exe_dir) = current.parent() {
            add_candidate(&mut search, exe_dir.join("resources").join(name));
            add_candidate(&mut search, exe_dir.join("Resources").join(name));
            if let Some(contents_dir) = exe_dir.parent() {
                add_candidate(&mut search, contents_dir.join("Resources").join(name));
                add_candidate(&mut search, contents_dir.join("resources").join(name));
            }
        }
    }

    if cfg!(debug_assertions) {
        add_debug_target_candidates(&mut search, name);
    }

    if let Some(path) = find_on_path(name) {
        add_candidate(&mut search, path);
    } else {
        add_candidate(&mut search, PathBuf::from(name));
    }

    search.selected = search
        .candidates
        .iter()
        .find(|candidate| candidate.is_file())
        .cloned();
    search
}

fn add_debug_target_candidates(search: &mut AgentBinarySearch, name: &str) {
    if let Some(target_dir) =
        env::var_os("CARGO_TARGET_DIR").or(option_env!("CARGO_TARGET_DIR").map(Into::into))
    {
        add_target_profile_candidates(search, PathBuf::from(target_dir), name);
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(workspace_root) = manifest_dir.parent().and_then(Path::parent) {
        add_target_profile_candidates(search, workspace_root.join("target"), name);
    }

    if let Ok(current_dir) = env::current_dir() {
        for ancestor in current_dir.ancestors().take(8) {
            add_target_profile_candidates(search, ancestor.join("target"), name);
        }
    }
}

fn add_target_profile_candidates(search: &mut AgentBinarySearch, target_dir: PathBuf, name: &str) {
    add_candidate(search, target_dir.join("debug").join(name));
    add_candidate(search, target_dir.join("release").join(name));
}

fn add_candidate(search: &mut AgentBinarySearch, path: PathBuf) {
    if !search.candidates.iter().any(|candidate| candidate == &path) {
        search.candidates.push(path);
    }
}

fn agent_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "aipass-agent.exe"
    } else {
        "aipass-agent"
    }
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    })
}

fn absolute_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        return path;
    }
    env::current_dir()
        .map(|dir| dir.join(&path))
        .unwrap_or(path)
}

fn agent_binary_not_found_message(search: &AgentBinarySearch) -> String {
    let mut lines = vec!["AIPass agent binary was not found.".to_string()];
    if let Some((env_name, path)) = &search.env_override {
        lines.push(format!(
            "{env_name} is set to {}, but that file does not exist.",
            path.display()
        ));
    }
    lines.push(format!(
        "Tried binaries: {}",
        format_path_list(&search.candidates)
    ));
    lines.push(agent_recovery_hint().to_string());
    lines.join("\n")
}

fn agent_launch_failure_message(
    vault_dir: &Path,
    namespace: &str,
    search: &AgentBinarySearch,
    launch_error: Option<(&Path, &str)>,
    initial_connection_error: &str,
) -> String {
    let mut lines = vec!["AIPass agent is not running and could not be launched.".to_string()];
    lines.push(format!("Vault: {}", vault_dir.display()));
    lines.push(format!("Namespace: {namespace}"));
    if let Some((binary, error)) = launch_error {
        lines.push(format!(
            "Launch error: failed to start {}: {error}",
            binary.display()
        ));
    } else if let Some((env_name, path)) = &search.env_override {
        lines.push(format!(
            "Launch error: {env_name} points to {}, but that file does not exist.",
            path.display()
        ));
    } else {
        lines.push("Launch error: no aipass-agent binary was found.".to_string());
    }
    lines.push(format!(
        "Tried binaries: {}",
        format_path_list(&search.candidates)
    ));
    lines.push(format!(
        "Initial connection error: {initial_connection_error}"
    ));
    lines.push(agent_recovery_hint().to_string());
    lines.join("\n")
}

fn format_path_list(paths: &[PathBuf]) -> String {
    if paths.is_empty() {
        return "(none)".to_string();
    }
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn agent_recovery_hint() -> &'static str {
    if cfg!(debug_assertions) {
        "Development hint: run `cargo build -p aipass-agent` from the repository root, or set `AIPASS_AGENT_BINARY=/absolute/path/to/aipass-agent`."
    } else {
        "Install or repair AIPass so aipass-agent is available, or set `AIPASS_AGENT_BINARY=/absolute/path/to/aipass-agent`."
    }
}
