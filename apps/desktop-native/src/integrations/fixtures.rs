use super::*;
use aipass_agent_protocol::ToolConfigPreviewResponse;

impl Integrations {
    pub fn presentation(&self, cx: &gpui::App) -> serde_json::Value {
        serde_json::json!({"expanded":self.expanded,"pending":self.pending.is_some(),"blocked":self.blocked,
            "selectedKey":self.secret_id,"preview":self.preview.as_ref().map(|p|p.read(cx).presentation())})
    }
    pub fn fixture(&mut self, scenario: &str, window: &mut Window, cx: &mut Context<Self>) {
        // Discard the queued scan before the fixture's deferred event is delivered.
        self.pending = None;
        self.requests.clear();
        self.in_flight = None;
        self.blocked = scenario.starts_with("providers-edit");
        self.scanned = true;
        self.detections = vec![aipass_config_writers::ToolDetection {
            tool: aipass_config_writers::ToolId::Codex,
            binary_found: true,
            local_runtime_found: false,
            extension_found: false,
            config_path: Some("/synthetic/.codex/config.toml".into()),
        }];
        self.expanded = true;
        if !scenario.starts_with("providers-integration-") {
            let entry = aipass_desktop_native::catalog::Catalog::fixture()
                .active
                .remove(0);
            self.statuses = vec![ToolConfigStatus {
                tool: ToolConfigTool::Codex,
                state: "ready".into(),
                entry_id: Some(entry.id),
                entry_title: Some(entry.title),
                secret_id: Some("key-a".into()),
                mode: None,
                account_identity: None,
                operation_id: None,
                message: None,
                overrides: vec![],
            }];
            return;
        }
        if scenario == "providers-integration-picker" {
            return;
        }
        self.select_secret("key-a".into(), cx);
        self.show_preview(ToolConfigTool::Codex, scenario.ends_with("-readonly"), cx);
        let request = self.take_request().unwrap();
        let entry = self.entry.as_ref().unwrap();
        let files: Vec<aipass_agent_protocol::ToolConfigPreviewFile> = serde_json::from_str(
            include_str!("../../../../tests/desktop-parity/reference/business-preview-files.json"),
        )
        .unwrap();
        let response = ToolConfigPreviewResponse {
            preview_id: Some("synthetic-preview".into()),
            tool: ToolConfigTool::Codex,
            mode: aipass_agent_protocol::ToolConfigMode::Plaintext,
            entry_id: entry.id,
            entry_title: entry.title.clone(),
            target_path: "/synthetic/codex/config".into(),
            summary: "Synthetic account switch".into(),
            preview: files[0].diff.clone(),
            files,
        };
        self.complete(
            Completion {
                scope: request.scope,
                result: Ok(Outcome::Preview(response)),
            },
            window,
            cx,
        );
        if let Some(view) = &self.preview {
            view.update(cx, |view, cx| view.fixture_state(scenario, cx));
        }
    }
}
