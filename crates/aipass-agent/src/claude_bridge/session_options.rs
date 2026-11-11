//! Native execution is explicit: inference-only route requests keep caller tools.
use super::*;
use serde::Deserialize;

#[derive(Clone, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Options {
    pub(super) cwd: PathBuf,
    permission_mode: String,
    #[serde(default)]
    setting_sources: Vec<String>,
    #[serde(default)]
    allowed_tools: Vec<String>,
    #[serde(default)]
    disallowed_tools: Vec<String>,
    #[serde(default)]
    additional_directories: Vec<PathBuf>,
}
impl Options {
    pub(super) fn read(payload: &Value, session: Option<&str>) -> Result<Option<Self>, String> {
        let Some(value) = payload.get("_aipass_claude_session") else {
            return Ok(None);
        };
        let mut options: Self = serde_json::from_value(value.clone())
            .map_err(|_| "invalid Claude ACP session options")?;
        if session.is_none_or(str::is_empty) {
            return Err("native Claude tools require x-aipass-session-id".into());
        }
        options.cwd = directory(&options.cwd)?;
        for path in &mut options.additional_directories {
            *path = directory(path)?;
        }
        if !matches!(
            options.permission_mode.as_str(),
            "default" | "acceptEdits" | "plan" | "bypassPermissions" | "auto" | "dontAsk"
        ) {
            return Err("unsupported Claude permissionMode".into());
        }
        if options
            .setting_sources
            .iter()
            .any(|s| !matches!(s.as_str(), "user" | "project" | "local"))
        {
            return Err("invalid Claude setting source".into());
        }
        if options
            .allowed_tools
            .iter()
            .chain(&options.disallowed_tools)
            .any(|s| s.is_empty() || s.len() > 4096 || s.chars().any(char::is_control))
        {
            return Err("invalid Claude tool permission rule".into());
        }
        Ok(Some(options))
    }
    pub(super) fn configure(&self, command: &mut Command) {
        command.args([
            "--tools",
            "default",
            "--permission-mode",
            &self.permission_mode,
        ]);
        if self.permission_mode == "bypassPermissions" {
            command.arg("--allow-dangerously-skip-permissions");
        }
        command
            .arg("--setting-sources")
            .arg(self.setting_sources.join(","));
        // The caller's MCP tools never execute in the agent; preserve their
        // existing round trip independently of native permissions.
        command.arg("--allowedTools").arg(
            std::iter::once("mcp__aipass__*".to_owned())
                .chain(self.allowed_tools.clone())
                .collect::<Vec<_>>()
                .join(","),
        );
        if !self.disallowed_tools.is_empty() {
            command
                .arg("--disallowedTools")
                .arg(self.disallowed_tools.join(","));
        }
        for path in &self.additional_directories {
            command.arg("--add-dir").arg(path);
        }
        if self.setting_sources.is_empty() {
            command.arg("--strict-mcp-config");
        }
    }
}
fn directory(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() || !path.is_dir() {
        return Err("Claude cwd and additionalDirectories must be existing absolute directories on the agent host".into());
    }
    path.canonicalize()
        .map_err(|_| "Claude workspace unavailable".into())
}
