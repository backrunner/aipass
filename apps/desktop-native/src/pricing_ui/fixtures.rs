use super::*;
impl Pricing {
    pub fn fixture(&mut self, scenario: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !scenario.starts_with("providers-pricing") {
            return;
        }
        self.pending = None;
        self.request = None;
        let config: PricingConfig = serde_json::from_str(include_str!(
            "../../../../tests/desktop-parity/fixtures/pricing.json"
        ))
        .unwrap();
        self.config = Some(config.clone());
        let entry = self.entry.as_mut().unwrap();
        let secret = &mut entry.secret_refs[0];
        secret.billing = Some(aipass_provider_registry::BillingRule {
            rate: Some("0.75".into()),
            currency: Some("USD".into()),
            unit_price: Some("0.01".into()),
            note: Some("Synthetic reference billing".into()),
        });
        let id = secret.id.clone();
        self.show(&id, window, cx);
        let request = self.take_request().unwrap();
        self.complete(
            Completion {
                scope: request.scope,
                result: Ok(config),
            },
            window,
            cx,
        );
        if scenario == "providers-pricing-reference" {
            self.reference_open = true;
        }
        if scenario == "providers-pricing-source" {
            self.picker_open = true;
        }
        if matches!(
            scenario,
            "providers-pricing-group"
                | "providers-pricing-confirm"
                | "providers-pricing-delete"
                | "providers-pricing-history"
                | "providers-pricing-offpeak"
        ) {
            self.edit_group(window, cx);
            self.panel = Some(match scenario {
                "providers-pricing-confirm" => Panel::Confirm,
                "providers-pricing-delete" => Panel::DeleteGroup,
                _ => Panel::Group,
            });
            if scenario == "providers-pricing-history" {
                self.history_open = true;
            }
            if scenario == "providers-pricing-offpeak" {
                self.rules[0].off_peak = true;
            }
        }
        cx.notify();
    }
    pub fn presentation(&self) -> serde_json::Value {
        serde_json::json!({"panel": self.panel.map(|p| match p { Panel::Assignment => "assignment", Panel::Group => "group", Panel::Confirm => "confirm", Panel::DeleteGroup => "delete", Panel::DeleteVersion(_) => "deleteVersion" }),
            "pending": self.pending.is_some(), "closing": self.closing, "sourceOpen": self.picker_open,
            "historyOpen": self.history_open, "referenceOpen": self.reference_open})
    }
}
