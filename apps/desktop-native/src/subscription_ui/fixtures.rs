//! Synthetic comparison states. This module never opens a browser or reads a CLI directory.
use super::*;
use aipass_provider_registry::{CredentialKind, SubscriptionSnapshot};

pub const SCENARIOS: &[&str] = &[
    "subscription-overview",
    "subscription-overview-collapsed",
    "subscription-runtime-quota",
    "subscription-runtime-proxy",
    "subscription-runtime-balance",
    "subscription-runtime-webhooks",
    "subscription-runtime-balance-bottom",
    "subscription-runtime-webhooks-bottom",
    "subscription-directory",
    "subscription-search-empty",
    "subscription-catalog-error",
    "subscription-claude-missing",
    "subscription-claude-pending",
    "subscription-cli",
    "subscription-api",
    "subscription-region",
    "subscription-device",
    "subscription-code",
    "subscription-error",
    "subscription-import",
    "subscription-import-progress",
    "subscription-import-empty",
    "subscription-usage",
    "subscription-usage-stale",
    "subscription-usage-empty",
    "subscription-usage-console",
    "subscription-usage-unknown",
    "subscription-usage-collapsed",
];
pub fn data() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../../../tests/desktop-parity/fixtures/subscriptions.json"
    ))
    .unwrap()
}
pub fn snapshot() -> SubscriptionSnapshot {
    serde_json::from_value(data()["snapshot"].clone()).unwrap()
}
pub fn entry(scenario: &str) -> EntrySummary {
    let mut entry = aipass_desktop_native::catalog::Catalog::fixture()
        .active
        .remove(0);
    entry.provider_id = Some("codex".into());
    entry.credential_kind = CredentialKind::OAuth;
    entry.title = "Codex · alice@example.invalid".into();
    entry.account_identity = Some("alice@example.invalid:fixture-workspace".into());
    entry.secret_refs.clear();
    entry.notes = None;
    let mut subscription = snapshot();
    if scenario.ends_with("stale") {
        subscription.stale = true;
        subscription.error = Some("login_required".into());
    }
    if scenario.ends_with("unknown") {
        subscription.windows[0].used_percent = None;
        subscription.windows[1].used_percent = Some(120.);
    }
    if scenario.ends_with("empty") {
        entry.subscription = None;
        return entry;
    }
    if scenario.ends_with("console") {
        subscription.quota_unavailable = true;
        subscription.windows.clear();
    }
    entry.subscription = Some(subscription);
    entry
}
impl Subscriptions {
    pub fn prepare_capture_scroll(&self) {
        self.body_scroll
            .set_offset(gpui::point(gpui::px(0.), -self.body_scroll.max_offset().y));
    }
    pub fn fixture(&mut self, scenario: &str, window: &mut Window, cx: &mut Context<Self>) {
        let scenario = scenario.strip_suffix("-bottom").unwrap_or(scenario);
        self.preview = true;
        self.providers = model::presentation::catalog().to_vec();
        self.loaded = true;
        if scenario.starts_with("subscription-overview") {
            self.runtime_options.value =
                Some(serde_json::from_value(data()["runtime"].clone()).unwrap());
            self.runtime_options.expanded = !scenario.ends_with("collapsed");
            return;
        }
        if scenario.starts_with("subscription-runtime-") {
            self.entry = Some(entry(scenario));
            self.runtime_options.value =
                Some(serde_json::from_value(data()["runtime"].clone()).unwrap());
            self.show_options(
                match scenario.rsplit('-').next().unwrap_or_default() {
                    "proxy" => 1,
                    "balance" => 2,
                    "webhooks" => 3,
                    _ => 0,
                },
                window,
                cx,
            );
            self.outbox.clear();
            return;
        }
        if scenario.starts_with("subscription-usage") {
            self.usage_open = !scenario.ends_with("collapsed");
            return;
        }
        self.show(None, None, window, cx);
        match scenario {
            "subscription-search-empty" => self.query.update(cx, |input, cx| {
                input.set_value("no-such-provider", window, cx)
            }),
            "subscription-catalog-error" => {
                self.providers.clear();
                self.loaded = false;
                self.error = self.text("communityConnect.loadFailed");
            }
            "subscription-claude-missing" | "subscription-claude-pending" => {
                self.choose("claude".into(), window, cx);
                self.cli = Some(ClaudeCliStatus {
                    available: scenario.ends_with("pending"),
                    reason: Some("missing".into()),
                    version: None,
                });
                if scenario.ends_with("pending") {
                    self.login = Some(fixture_login(Flow::Claude(Uuid::nil()), false));
                }
            }
            "subscription-cli" => {
                self.choose("codex".into(), window, cx);
                self.cli = Some(ClaudeCliStatus {
                    available: false,
                    reason: Some("missing".into()),
                    version: None,
                });
            }
            "subscription-api" | "subscription-region" => {
                self.choose(
                    if scenario.ends_with("region") {
                        "bailian-coding-plan"
                    } else {
                        "minimax-m-plan-cn"
                    }
                    .into(),
                    window,
                    cx,
                );
            }
            "subscription-device" | "subscription-code" | "subscription-error" => {
                self.choose("copilot".into(), window, cx);
                self.cli = Some(ClaudeCliStatus {
                    available: true,
                    reason: None,
                    version: None,
                });
                if scenario.ends_with("error") {
                    self.error = self.text("subscriptionImport.error.task_failed");
                } else {
                    self.login = Some(fixture_login(
                        Flow::Community(Uuid::nil()),
                        scenario.ends_with("code"),
                    ));
                }
            }
            "subscription-import"
            | "subscription-import-progress"
            | "subscription-import-empty" => {
                self.screen = Screen::Import;
                let mut task: SubscriptionImportTask =
                    serde_json::from_value(data()["task"].clone()).unwrap();
                if scenario.ends_with("progress") {
                    task.phase = "importing".into();
                    task.completed = 2;
                    task.results.truncate(2);
                }
                if scenario.ends_with("empty") {
                    task.results.clear();
                    task.total = 0;
                    task.completed = 0;
                }
                self.task = Some(task);
            }
            _ => {}
        }
        self.outbox.clear();
        self.pending = None;
        self.next_poll = None;
        self.set_disabled(cx);
        cx.notify();
    }
    pub fn run_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(request) = self.take_request() else {
            return;
        };
        let kind = request.operation.kind();
        let result = match request.operation {
            Operation::Options(_, action) => {
                use model::options::{Action, ResultData};
                Ok(Outcome::Options(match action {
                    Action::Load => ResultData::Loaded(
                        Box::new(serde_json::from_value(data()["runtime"].clone()).unwrap()),
                        vec![],
                        None,
                    ),
                    Action::Save(mut value) => {
                        if let Some(p) = &mut value.proxy {
                            p.has_credentials = !p.clear_credentials
                                && (p.has_credentials
                                    || p.password.is_some()
                                    || p.username.is_some());
                            p.username = None;
                            p.password = None;
                            p.clear_credentials = false;
                        }
                        if let Some(b) = &mut value.balance {
                            b.body = None;
                            for (_, secret) in &mut b.headers {
                                *secret = None;
                            }
                        }
                        for h in &mut value.webhooks {
                            h.has_secret = !h.clear_secret && (h.has_secret || h.secret.is_some());
                            h.secret = None;
                            h.clear_secret = false;
                        }
                        ResultData::Saved(value)
                    }
                    Action::Probe => ResultData::Balance("42 USD".into()),
                    Action::Test(_) => ResultData::Sent,
                    Action::Reset => ResultData::Reset("not_needed".into()),
                }))
            }
            Operation::Catalog => Ok(Outcome::Catalog(model::presentation::catalog().to_vec())),
            Operation::Cli(_) => Ok(Outcome::Cli(ClaudeCliStatus {
                available: false,
                reason: Some("missing".into()),
                version: None,
            })),
            Operation::Import(_) => Ok(Outcome::Import(
                serde_json::from_value(data()["task"].clone()).unwrap(),
            )),
            Operation::Refresh(_) => Ok(Outcome::Refreshed(snapshot())),
            _ => Err(aipass_agent::AgentCommandError {
                code: None,
                message: "Synthetic UI preview: external actions are disabled".into(),
            }),
        };
        self.complete(
            Completion {
                generation: request.generation,
                kind,
                result,
            },
            window,
            cx,
        );
    }
}
fn fixture_login(flow: Flow, code: bool) -> Login {
    Login {
        flow,
        pending: true,
        entry_id: None,
        url: Some(
            if matches!(flow, Flow::Claude(_)) {
                "https://claude.ai/oauth/authorize"
            } else {
                "https://github.com/login/device"
            }
            .into(),
        ),
        code_required: code,
        user_code: (!code).then(|| "ABCD-EFGH".into()),
        instructions: None,
        error: None,
    }
}
