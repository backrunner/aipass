use super::*;
impl Settings {
    pub fn preview(&mut self, scenario: &str, window: &mut Window, cx: &mut Context<Self>) {
        let tab = ["general", "security", "sync", "server", "backup", "about"]
            .iter()
            .position(|name| scenario.starts_with(&format!("settings-{name}")))
            .unwrap_or(0);
        self.show(tab, false, window, cx);
        // Same fixed build stamp as tests/desktop-parity/vite.config.mjs,
        // displayed in the comparison's Asia/Singapore timezone.
        self.build_time = "2026-11-11 08:00".into();
        self.detection = Some(CcSwitchDetection {
            config_exists: false,
            app_installed: false,
            config_path: None,
        });
        self.config = Some(aipass_proxy::ProxyConfig::default());
        self.panel = Some(ControlPanelStatus {
            settings: ControlPanelSettings::default(),
            running: false,
            url: None,
            addresses: vec!["127.0.0.1".into()],
            certificate_pem: None,
            fingerprint: None,
            imported_certificate: false,
            error: None,
            has_access_code: false,
            remote_unlock_enabled: false,
        });
        use aipass_desktop_platform::browser::{BrowserExtensionStatus, NativeHostStatus};
        self.extension = Some(BrowserExtensionStatus {
            browser: "chromium".into(),
            detected_browsers: vec!["Chromium".into()],
            chrome_installed: true,
            chrome_path: None,
            extension_id: String::new(),
            discovered_extension_ids: vec![],
            extension_version: String::new(),
            zip_path: Default::default(),
            zip_exists: false,
            extension_installed: false,
            installed_paths: vec![],
            native_host_configured: false,
            native_host: NativeHostStatus {
                browser: "chromium".into(),
                browser_label: "Chromium".into(),
                host_path: Default::default(),
                host_exists: false,
                host_usable: false,
                host_error: None,
                manifest_path: Default::default(),
                manifest_exists: false,
                settings_path: Default::default(),
                allowed_extension_ids: vec![],
                allowed_origins: vec![],
            },
            native_hosts: vec![],
        });
        if scenario.contains("webdav") {
            self.sync_mode = SyncMode::WebDav;
            self.sync_settings.mode = SyncMode::WebDav;
        }
        if scenario.contains("records") || scenario.contains("snapshots") {
            let fixture: serde_json::Value = serde_json::from_str(include_str!(
                "../../../../tests/desktop-parity/fixtures/administration.json"
            ))
            .unwrap();
            let records = &fixture["settingsRecords"];
            self.conflicts = serde_json::from_value(
                records[if scenario.contains("snapshots") {
                    "snapshots"
                } else {
                    "conflicts"
                }]
                .clone(),
            )
            .unwrap();
            self.devices = serde_json::from_value(records["devices"].clone()).unwrap();
        }
        if scenario.starts_with("settings-server-") {
            let fixture: serde_json::Value = serde_json::from_str(include_str!(
                "../../../../tests/desktop-parity/fixtures/administration.json"
            ))
            .unwrap();
            self.config =
                Some(serde_json::from_value(fixture["settingsServer"]["config"].clone()).unwrap());
            if scenario.contains("certificate") || scenario.contains("access-code") {
                self.panel = Some(
                    serde_json::from_value(fixture["settingsServer"]["panel"].clone()).unwrap(),
                );
                self.certificate_open = scenario.ends_with("open");
                if scenario.contains("access-code") {
                    self.show_access_code(SensitiveString::new("fixture-access-code-only"), cx);
                    self.allow_remote_unlock = true;
                }
            }
        }
        if scenario.ends_with("error") {
            self.error = self.locale.text("settings.saveFailed");
        }
        if scenario == "settings-sync-feedback-success" {
            self.sync_settings.sync_folder = Some("/tmp/aipass-parity-sync".into());
            self.notice = self.locale.text("settings.syncSettingsSaved");
        }
        if scenario.ends_with("loading") {
            self.loading = true;
            self.pending = true;
        }
    }
}
