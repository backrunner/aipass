use super::*;
impl Proxy {
    pub fn preview(&mut self, scenario: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.active = true;
        self.loaded = true;
        self.pending = false;
        self.outbox.clear();
        self.last_refresh = Instant::now() + Duration::from_secs(86400);
        self.config = Some(ProxyConfig::default());
        self.status = Some(ProxyStatus {
            running: false,
            enabled: false,
            bind_addr: "127.0.0.1:8787".into(),
            active_routes: 0,
            requests: 0,
            failures: 0,
            last_error: None,
            degraded: false,
            degraded_target_ids: vec![],
            recent_requests: 0,
            recent_tokens: 0,
            success_rate_bps: 0,
            average_first_token_ms: None,
            in_flight_requests: 0,
            available_channels: 0,
            total_channels: 0,
            channels: vec![],
        });
        self.scanned = true;
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/desktop-parity/fixtures/administration.json"
        ))
        .unwrap();
        self.fixture_now = Some(
            time::OffsetDateTime::parse(
                fixture["now"].as_str().unwrap(),
                &time::format_description::well_known::Rfc3339,
            )
            .unwrap(),
        );
        if !scenario.contains("empty") {
            self.detections = [
                aipass_config_writers::ToolId::Codex,
                aipass_config_writers::ToolId::ClaudeCode,
            ]
            .into_iter()
            .map(|tool| aipass_config_writers::ToolDetection {
                tool,
                binary_found: true,
                local_runtime_found: false,
                extension_found: false,
                config_path: None,
            })
            .collect();
            self.config = Some(serde_json::from_value(fixture["config"].clone()).unwrap());
            self.status = Some(serde_json::from_value(fixture["status"].clone()).unwrap());
            self.usage = Some(serde_json::from_value(fixture["usage"].clone()).unwrap());
            self.series = serde_json::from_value(fixture["series"].clone()).unwrap();
            self.entries = serde_json::from_value(fixture["entries"].clone()).unwrap();
            self.selected = self.config.as_ref().unwrap().routes.first().map(|r| r.id);
        }
        if scenario == "proxy-empty-bind" {
            self.begin_bind(window, cx);
        }
        if scenario.contains("edit") {
            let route = self.config.as_ref().unwrap().routes.first().cloned();
            self.edit_route(route, window, cx);
        }
        if scenario.contains("advanced") {
            self.advanced = true;
        }
        if scenario.contains("models") {
            self.bind_models = true;
        }
        if scenario.contains("picker") {
            self.picker.update(cx, |p, _| p.preview_open());
        }
        if scenario.contains("route") {
            self.edit_route(None, window, cx);
        }
        if scenario.contains("search") {
            self.search_config = self.config.as_ref().unwrap().search.clone();
            self.open_modal(Modal::Search, window, cx);
            if scenario.contains("api") {
                self.search_config.apis.push(aipass_proxy::SearchApiConfig {
                    id: Uuid::from_u128(64),
                    vendor: aipass_proxy::SearchVendor::Tavily,
                    provider_entry_id: None,
                    secret_id: None,
                    base_url: None,
                    enabled: true,
                });
            }
        }
        if scenario.contains("logs") {
            self.open_modal(Modal::Logs, window, cx);
            self.update_logs(serde_json::from_value(fixture["logs"].clone()).unwrap());
        }
        if scenario.starts_with("proxy-preview-") {
            let route = &self.config.as_ref().unwrap().routes[0];
            self.plan = Some(aipass_agent_protocol::ToolConfigProxyRequest {
                tool: aipass_config_writers::ToolId::Codex,
                route_id: route.id,
            });
            self.preview_readonly = scenario.ends_with("readonly");
            self.show_tool_preview(
                aipass_agent_protocol::ToolConfigPreviewResponse {
                    preview_id: None,
                    tool: aipass_agent_protocol::ToolConfigTool::Codex,
                    mode: aipass_agent_protocol::ToolConfigMode::Plaintext,
                    entry_id: route.id,
                    entry_title: route.name.clone(),
                    target_path: "/synthetic/config.toml".into(),
                    summary: String::new(),
                    preview: String::new(),
                    files: serde_json::from_str(include_str!(
                        "../../../../tests/desktop-parity/reference/business-preview-files.json"
                    ))
                    .unwrap(),
                },
                window,
                cx,
            );
            self.preview.as_ref().unwrap().update(cx, |v, cx| {
                if scenario.ends_with("error") {
                    v.error = std::iter::repeat_n(self.locale.text("settings.saveFailed"), 40)
                        .collect::<Vec<_>>()
                        .join("\n");
                }
                v.busy = scenario.ends_with("busy");
                cx.notify();
            });
        }
        if scenario.ends_with("error") {
            self.error = self.locale.text("settings.saveFailed");
        }
        if scenario.ends_with("loading") {
            self.loaded = false;
            self.pending = true;
        }
    }
}
