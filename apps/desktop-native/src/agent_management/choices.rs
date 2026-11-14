//! Keep managed-switch choices distinct from provider quick-integration modes.
use crate::{integration::catalog, subscriptions::presentation};
use aipass_agent_protocol::{
    CodexApiKeyMode, ToolConfigMode, ToolConfigRequest, ToolConfigStatus, ToolConfigTool as Tool,
};
use aipass_config_writers::ToolDetection;
use aipass_provider_registry::CredentialKind;
use aipass_vault::EntrySummary;

pub const MANUAL: [(Tool, &str, &str); 6] = [
    (Tool::VscodeChat, "VS Code Chat", "vscode-chat"),
    (Tool::Cline, "Cline", "cline"),
    (Tool::Trae, "TRAE", "trae"),
    (Tool::Qoder, "Qoder", "qoder"),
    (Tool::Zcode, "ZCode", "zcode"),
    (
        Tool::JetbrainsCopilot,
        "JetBrains Copilot",
        "jetbrains-copilot",
    ),
];
pub fn name(tool: &Tool) -> &'static str {
    MANUAL
        .iter()
        .find(|(id, _, _)| id == tool)
        .map(|(_, name, _)| *name)
        .unwrap_or_else(|| catalog::name(tool))
}
pub fn guide(tool: &Tool) -> Option<&'static str> {
    MANUAL
        .iter()
        .find(|(id, _, _)| id == tool)
        .map(|(_, _, guide)| *guide)
}
pub fn tools(detections: &[ToolDetection]) -> Vec<Tool> {
    catalog::AUTOMATIC
        .iter()
        .map(|(id, _)| id)
        .chain(MANUAL.iter().map(|(id, _, _)| id))
        .filter(|tool| catalog::installed(tool, detections))
        .cloned()
        .collect()
}
pub fn empty_status(tool: Tool, state: &str) -> ToolConfigStatus {
    ToolConfigStatus {
        tool,
        state: state.into(),
        entry_title: None,
        entry_id: None,
        secret_id: None,
        mode: None,
        account_identity: None,
        operation_id: None,
        message: None,
        overrides: vec![],
    }
}
#[derive(Clone)]
pub struct Choice {
    pub value: String,
    pub label: String,
    pub request: ToolConfigRequest,
}
pub fn choices(entries: &[EntrySummary], tool: &Tool) -> Vec<Choice> {
    let mut choices = vec![];
    for entry in entries
        .iter()
        .filter(|e| e.archived_at.is_none() && e.deleted_at.is_none())
    {
        let native = catalog::native_tool(entry).as_ref() == Some(tool);
        for secret in &entry.secret_refs {
            if !native && !catalog::ready(tool, entry, secret) {
                continue;
            }
            let detail = if secret.label.trim().is_empty() {
                &secret.id
            } else {
                secret.label.trim()
            };
            let duplicate = entry.secret_refs.iter().filter(|s| if s.label.trim().is_empty() { &s.id } else { s.label.trim() } == detail).count() > 1;
            let title = if native {
                let a = presentation::account(entry, &|key| key.into());
                if a.subtitle.is_empty() {
                    a.title
                } else {
                    format!("{} ({})", a.title, a.subtitle)
                }
            } else {
                entry.title.clone()
            };
            let label = if entry.secret_refs.len() > 1 {
                format!(
                    "{title} ({detail}{})",
                    if duplicate {
                        format!(" / {}", secret.id)
                    } else {
                        String::new()
                    }
                )
            } else {
                title
            };
            let mode = if native {
                ToolConfigMode::Official
            } else if matches!(
                tool,
                Tool::Zed
                    | Tool::AntigravityCli
                    | Tool::MuseCode
                    | Tool::HermesAgent
                    | Tool::CommandCode
            ) {
                ToolConfigMode::Helper
            } else {
                ToolConfigMode::Plaintext
            };
            choices.push(Choice {
                value: format!("{}:{}", entry.id, secret.id),
                label,
                request: ToolConfigRequest {
                    tool: tool.clone(),
                    id: entry.id,
                    secret_id: Some(secret.id.clone()),
                    mode,
                    codex_api_key_mode: (*tool == Tool::Codex
                        && entry.credential_kind == CredentialKind::Api)
                        .then_some(CodexApiKeyMode::AuthJson),
                    preview_id: None,
                },
            });
        }
    }
    let labels: Vec<_> = choices.iter().map(|c| c.label.clone()).collect();
    for choice in &mut choices {
        if labels.iter().filter(|l| **l == choice.label).count() > 1 {
            choice.label = format!(
                "{} ({} / {})",
                choice.label,
                presentation::tail(&choice.request.id.to_string(), 8),
                choice.request.secret_id.as_deref().unwrap_or_default()
            );
        }
    }
    choices
}
pub fn reconcile(choices: &[Choice], previous: &str, status: Option<&ToolConfigStatus>) -> String {
    choices
        .iter()
        .find(|c| c.value == previous)
        .or_else(|| {
            choices.iter().find(|c| {
                status.is_some_and(|s| {
                    Some(c.request.id) == s.entry_id && c.request.secret_id == s.secret_id
                })
            })
        })
        .map(|c| c.value.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
