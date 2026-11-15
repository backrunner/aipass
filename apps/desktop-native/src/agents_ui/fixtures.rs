use super::*;
pub const SCENARIOS: &[&str] = &[
    "agents-ready",
    "agents-picker",
    "agents-empty",
    "agents-scanning",
    "agents-unavailable",
    "agents-conflict",
    "agents-cursor",
    "agents-setup",
    "agents-preview",
    "agents-switching",
    "agents-usage-error",
    "agents-oauth",
];
pub fn entries(scenario: &str) -> Vec<EntrySummary> {
    let mut entries = aipass_desktop_native::catalog::Catalog::fixture().active;
    if scenario == "agents-oauth" {
        let mut entry = crate::subscription_ui::fixtures::entry("subscription-usage");
        entry.provider_kind = aipass_provider_registry::ProviderKind::Official;
        entry.secret_refs = entries[0].secret_refs.clone();
        entries[0] = entry;
    }
    entries
}
impl Agents {
    pub fn fixture(&mut self, scenario: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.retire(cx);
        self.active = true;
        self.loaded = true;
        self.usage_needed = false;
        self.usage_failed = scenario == "agents-usage-error";
        self.sync(&entries(scenario), cx);
        let data: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/desktop-parity/fixtures/agents.json"
        ))
        .unwrap();
        self.detections = data["detections"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| {
                let tool: Tool = serde_json::from_value(d["tool"].clone()).unwrap();
                aipass_config_writers::ToolDetection {
                    tool: aipass_desktop_native::integration::catalog::tool_id(&tool),
                    binary_found: d["binaryFound"].as_bool().unwrap(),
                    extension_found: d["extensionFound"].as_bool().unwrap(),
                    local_runtime_found: d["localRuntimeFound"].as_bool().unwrap(),
                    config_path: d["configPath"].as_str().map(Into::into),
                }
            })
            .collect();
        self.statuses = serde_json::from_value(data["statuses"].clone()).unwrap();
        self.summary = Some(serde_json::from_value(data["usage"].clone()).unwrap());
        if matches!(scenario, "agents-empty" | "agents-scanning") {
            self.detections.clear();
            self.statuses.clear();
            self.loaded = scenario != "agents-scanning";
        }
        if scenario == "agents-unavailable" {
            self.statuses[0].state = "unavailable".into();
        }
        if scenario == "agents-conflict" {
            self.statuses[0].state = "conflict".into();
            self.statuses[0].operation_id = Some(uuid::Uuid::nil());
            self.statuses[0].overrides = vec!["OPENAI_API_KEY".into()];
        }
        if scenario == "agents-cursor" {
            self.detections
                .iter_mut()
                .find(|d| d.tool == aipass_config_writers::ToolId::Cursor)
                .unwrap()
                .local_runtime_found = true;
        }
        self.selected = Some(if scenario == "agents-cursor" {
            Tool::Cursor
        } else if scenario == "agents-setup" {
            Tool::Cline
        } else {
            Tool::Codex
        });
        self.reconcile(cx);
        if scenario == "agents-picker" {
            self.picker.update(cx, |v, _| v.preview_open());
        }
        if scenario == "agents-setup" {
            let choice = choices::choices(&self.entries, &Tool::Cline).remove(0);
            self.selections.insert(key(&Tool::Cline), choice.value);
            self.reconcile(cx);
            self.show_setup(choice.request, window, cx);
        }
        if scenario == "agents-switching" {
            self.prepare(window, cx);
            self.outbox.clear();
        }
        if scenario == "agents-preview" {
            self.prepare(window, cx);
            let request = self.take_request().unwrap();
            let files: Vec<aipass_agent_protocol::ToolConfigPreviewFile> =
                serde_json::from_str(include_str!(
                    "../../../../tests/desktop-parity/reference/business-preview-files.json"
                ))
                .unwrap();
            self.complete(
                Completion {
                    generation: request.generation,
                    kind: Kind::Preview,
                    result: Ok(Outcome::Preview(
                        aipass_agent_protocol::ToolConfigPreviewResponse {
                            preview_id: Some("fixture-preview".into()),
                            tool: Tool::Codex,
                            mode: aipass_agent_protocol::ToolConfigMode::Plaintext,
                            entry_id: self.entries[0].id,
                            entry_title: self.entries[0].title.clone(),
                            target_path: "/synthetic/codex/config".into(),
                            summary: "Synthetic account switch".into(),
                            preview: files[0].diff.clone(),
                            files,
                        },
                    )),
                },
                window,
                cx,
            );
        }
    }
}
