use super::*;
impl Tools {
    pub fn fixture(&mut self, scenario: &str, window: &mut Window, cx: &mut Context<Self>) {
        if scenario == "providers-reset" || scenario == "providers-reset-error" {
            self.show_reset(window, cx);
            if scenario.ends_with("-error") {
                self.error = self.locale.text("tray.agentUnavailable");
            }
        }
        if scenario.starts_with("providers-usage") {
            self.show_usage(window, cx);
            if scenario.contains("advanced") {
                self.mode = UsageProbeMode::NewApiAdvanced;
            }
            if scenario.ends_with("-result") {
                self.usage=Some(serde_json::from_value(serde_json::json!({"ok":true,"source":"new_api_token_usage","endpoint":"https://gateway.example.invalid/api/usage/token","status":200,"quota":{"label":"Fixture plan","limit":"100","used":"25","remaining":"75","unit":"USD"},"gateway":{"group":"default","rate":"1"}})).unwrap());
            }
            if scenario.ends_with("-failed") {
                self.usage=Some(serde_json::from_value(serde_json::json!({"ok":false,"source":"unknown","endpoint":"https://gateway.example.invalid/api/usage/token","status":401,"error":"Fixture access denied"})).unwrap());
            }
            if scenario.ends_with("-busy") {
                self.pending = Some(Pending::Usage);
                for input in &self.inputs {
                    input.update(cx, |s, cx| s.set_disabled(true, cx));
                }
            }
        }
        if scenario == "providers-probe-result" {
            self.probe=Some(serde_json::from_value(serde_json::json!({"ok":true,"interfaceType":"openai_compatible","status":200,"modelCount":12})).unwrap());
        }
    }
    pub fn presentation(&self) -> serde_json::Value {
        serde_json::json!({"panel":self.panel.map(|p|if p==Panel::Reset{"reset"}else{"usage"}),"busy":self.busy(),"canApply":self.usage.as_ref().is_some_and(aipass_desktop_native::provider_tools::can_apply),"hasProbe":self.probe.is_some(),"keyOpen":self.key_open})
    }
}
