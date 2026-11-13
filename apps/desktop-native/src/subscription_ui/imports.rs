use super::*;
use crate::parity::Observe;
use aipass_agent_protocol::{SubscriptionImportResult, SubscriptionImportStatus as Status};
use gpui::{div, px, AnyElement, FontWeight};

pub(super) const IMPORT_PROVIDERS: [(&str, &str); 15] = [
    ("anthropic", "Claude"),
    ("codex", "Codex"),
    ("grok", "Grok Build"),
    ("copilot", "GitHub Copilot"),
    ("gemini-cli", "Gemini CLI"),
    ("zcode", "ZCode"),
    ("devin", "Devin"),
    ("commandcode-plan", "Command Code"),
    ("cursor", "Cursor"),
    ("kiro", "Kiro"),
    ("workbuddy", "WorkBuddy"),
    ("workbuddy-ai", "WorkBuddy AI"),
    ("kimi-work", "Kimi Work"),
    ("opencode-go", "OpenCode Go"),
    ("opencode-zen", "OpenCode Zen"),
];

impl Subscriptions {
    pub(super) fn import_busy(&self) -> bool {
        self.pending == Some(Kind::Import)
            || self
                .task
                .as_ref()
                .is_some_and(|t| matches!(t.phase.as_str(), "discovering" | "importing"))
    }
    pub(super) fn import_count(&self) -> AnyElement {
        let results = self
            .task
            .as_ref()
            .map(|t| t.results.as_slice())
            .unwrap_or_default();
        let connected = results
            .iter()
            .filter(|r| matches!(r.status, Status::Imported | Status::Updated))
            .count();
        let found = connected
            + results
                .iter()
                .filter(|r| r.status == Status::Existing)
                .count();
        div()
            .px(px(8.))
            .py(px(3.))
            .rounded_full()
            .border_1()
            .border_color(self.theme.border)
            .text_size(px(11.))
            .line_height(px(11.))
            .text_color(self.theme.secondary)
            .child(
                self.text(if connected > 0 {
                    "subscriptionImport.summary"
                } else {
                    "subscriptionImport.found"
                })
                .replace("{imported}", &connected.to_string())
                .replace("{found}", &found.to_string()),
            )
            .into_any_element()
    }
    fn import_result(&self, result: &SubscriptionImportResult, cx: &Context<Self>) -> AnyElement {
        let provider = result.source.provider.clone();
        let login_provider = provider.clone();
        let name = IMPORT_PROVIDERS
            .iter()
            .find(|(id, _)| *id == provider)
            .map(|(_, n)| *n)
            .unwrap_or(&provider)
            .to_string();
        let source = match provider.as_str() {
            "cursor" => self.text(if result.source.selector == "keychain" {
                "subscriptionImport.source.secureStore"
            } else {
                "subscriptionImport.source.file"
            }),
            "kiro" => {
                if result.source.selector == "ide" {
                    "Kiro IDE".into()
                } else if let Some(kind) = result.source.selector.strip_prefix("cli:") {
                    format!("Kiro CLI ({})", kind.replace("odic", "OIDC"))
                } else {
                    "Kiro CLI".into()
                }
            }
            "zcode" => self.text(if result.source.selector.starts_with("team:") {
                "subscriptionImport.source.team"
            } else {
                "subscriptionImport.source.file"
            }),
            _ => String::new(),
        };
        let status = match result.status {
            Status::Imported => "imported",
            Status::Existing => "existing",
            Status::Updated => "updated",
            Status::NeedsLogin => "needs_login",
            Status::NotFound => "not_found",
            Status::Failed => "failed",
            Status::Cancelled => "cancelled",
        };
        let ticket = self.task.as_ref().unwrap().ticket;
        let source_id = result.source_id;
        div()
            .flex_shrink_0()
            .py(px(14.))
            .border_b_1()
            .border_color(self.theme.divider)
            .flex()
            .items_start()
            .gap(px(12.))
            .text_size(px(12.))
            .line_height(px(15.))
            .child(crate::provider_icon::icon(
                &provider,
                &name,
                aipass_provider_registry::CredentialKind::Api,
                &aipass_provider_registry::ProviderKind::Unknown,
                24.,
                self.dark,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .child(
                        div()
                            .min_h(px(26.))
                            .mt(px(-4.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(div().font_weight(FontWeight::BOLD).child(name))
                            .child(
                                self.button(
                                    format!("subscription-location-{source_id}"),
                                    String::new(),
                                    "folder-open",
                                    false,
                                )
                                .size(px(24.))
                                .border_color(gpui::rgba(0))
                                .bg(gpui::rgba(0))
                                .px(px(0.))
                                .accessibility_label(self.text("subscriptionImport.directory"))
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.queue(
                                            Operation::OpenLocation { ticket, source_id },
                                            cx,
                                        )
                                    },
                                )),
                            ),
                    )
                    .when_some(result.account_identity.as_ref(), |el, identity| {
                        el.child(
                            div()
                                .line_height(px(15.))
                                .child(model::presentation::readable(identity)),
                        )
                    })
                    .when(!source.is_empty(), |el| {
                        el.child(self.paragraph(source, 11.).line_height(px(14.3)))
                    }),
            )
            .child(
                div()
                    .max_w(px(190.))
                    .flex()
                    .flex_col()
                    .items_end()
                    .gap(px(8.))
                    .text_right()
                    .child(
                        div()
                            .text_size(px(11.))
                            .line_height(px(13.))
                            .text_color(if result.status == Status::Failed {
                                self.theme.danger
                            } else {
                                self.theme.secondary
                            })
                            .child(self.text(&format!("subscriptionImport.status.{status}"))),
                    )
                    .when(result.action.as_deref() == Some("login"), |el| {
                        el.child(
                            self.button(
                                format!("subscription-login-{source_id}"),
                                self.text("subscriptionImport.login"),
                                "log-in",
                                false,
                            )
                            .h(px(26.))
                            .px(px(9.))
                            .text_size(px(12.))
                            .line_height(px(15.))
                            .disabled(self.import_busy())
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.show(Some(login_provider.clone()), None, window, cx)
                                },
                            )),
                        )
                    })
                    .when(result.action.as_deref() == Some("install_cli"), |el| {
                        el.child(
                            self.button(
                                format!("subscription-install-{source_id}"),
                                self.text("subscriptionCli.install"),
                                "",
                                false,
                            )
                            .h(px(26.))
                            .px(px(9.))
                            .text_size(px(12.))
                            .line_height(px(15.))
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.queue(
                                        Operation::Install(if provider == "anthropic" {
                                            "claude".into()
                                        } else {
                                            provider.clone()
                                        }),
                                        cx,
                                    )
                                },
                            )),
                        )
                    })
                    .when(result.status == Status::Failed, |el| {
                        el.children(result.error_code.as_ref().map(|code| {
                            self.paragraph(
                                self.text(&format!("subscriptionImport.error.{code}")),
                                10.,
                            )
                            .line_height(px(12.))
                        }))
                    }),
            )
            .observe(format!("subscription-import-row-{source_id}"))
            .into_any_element()
    }
    pub(super) fn import_body(&self, cx: &Context<Self>) -> AnyElement {
        let busy = self.import_busy();
        let results = self
            .task
            .as_ref()
            .map(|t| t.results.as_slice())
            .unwrap_or_default();
        div()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(20.))
                    .py(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .text_size(px(12.))
                    .line_height(px(15.))
                    .child(self.text("subscriptionImport.description"))
                    .when(busy, |el| {
                        el.child(
                            div().font_weight(FontWeight::BOLD).child(
                                self.text("subscriptionImport.progress")
                                    .replace(
                                        "{completed}",
                                        &self.task.as_ref().map_or(0, |t| t.completed).to_string(),
                                    )
                                    .replace(
                                        "{total}",
                                        &self.task.as_ref().map_or(0, |t| t.total).to_string(),
                                    ),
                            ),
                        )
                    })
                    .when(
                        self.task.as_ref().is_some_and(|t| t.phase == "cancelled"),
                        |el| el.child(self.text("subscriptionImport.status.cancelled")),
                    )
                    .when(!self.error.is_empty(), |el| {
                        el.child(self.banner(self.error.clone(), false))
                    })
                    .observe("subscription-import-summary"),
            )
            .child(
                div()
                    .relative()
                    .min_h(px(100.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("subscription-import-results")
                            .track_scroll(&self.body_scroll)
                            .min_h_0()
                            .overflow_y_scroll()
                            .px(px(20.))
                            .flex()
                            .flex_col()
                            .children(results.iter().map(|r| self.import_result(r, cx)))
                            .when(results.is_empty(), |el| {
                                el.child(
                                    div()
                                        .px(px(20.))
                                        .py(px(32.))
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .gap(px(8.))
                                        .text_center()
                                        .child(
                                            div()
                                                .text_size(px(13.))
                                                .line_height(px(16.))
                                                .font_weight(FontWeight::BOLD)
                                                .child(self.text(if busy {
                                                    "subscriptionImport.discovering"
                                                } else {
                                                    "subscriptionImport.empty"
                                                })),
                                        )
                                        .when(!busy, |el| {
                                            el.child(
                                                self.paragraph(
                                                    self.text("subscriptionImport.directoryHint"),
                                                    14.,
                                                )
                                                .line_height(px(17.)),
                                            )
                                        }),
                                )
                            })
                            .observe("subscription-import-results"),
                    )
                    .child(scroll::fade(self.theme.surface, self.body_scroll.clone())),
            )
            .into_any_element()
    }
    pub(super) fn import_footer(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let busy = self.import_busy();
        let results = self
            .task
            .as_ref()
            .map(|t| t.results.as_slice())
            .unwrap_or_default();
        let retry = results.iter().any(|r| {
            matches!(
                r.status,
                Status::Failed | Status::NeedsLogin | Status::Cancelled
            )
        });
        div()
            .flex_shrink_0()
            .px(px(20.))
            .py(px(14.))
            .border_t_1()
            .border_color(self.theme.divider)
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                self.paragraph(
                    self.text(if self.import_provider == "gemini-cli" {
                        "subscriptionImport.geminiDirectoryHint"
                    } else {
                        "subscriptionImport.directoryHint"
                    }),
                    11.,
                )
                .line_height(px(13.)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div().w(px(170.)).child(
                                    self.picker(
                                        "import-provider".into(),
                                        String::new(),
                                        self.import_provider.clone(),
                                        IMPORT_PROVIDERS
                                            .iter()
                                            .map(|(v, n)| (v.to_string(), n.to_string()))
                                            .collect(),
                                        (window, cx),
                                    ),
                                ),
                            )
                            .child(
                                self.button(
                                    "subscription-directory",
                                    self.text("subscriptionImport.directory"),
                                    "folder-open",
                                    false,
                                )
                                .disabled(busy || self.pending.is_some())
                                .on_click(cx.listener(
                                    |this, _, window, cx| this.choose_directory(window, cx),
                                )),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .when(busy, |el| {
                                el.child(
                                    self.button(
                                        "subscription-import-cancel",
                                        self.text("common.cancel"),
                                        "",
                                        false,
                                    )
                                    .disabled(
                                        self.task.is_none()
                                            || self.cancel_requested
                                            || self.pending.is_some(),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                                )
                            })
                            .when(!busy, |el| {
                                el.when(results.is_empty(), |el| {
                                    el.child(
                                        self.button(
                                            "subscription-import-login",
                                            self.text("subscriptionImport.login"),
                                            "log-in",
                                            false,
                                        )
                                        .on_click(
                                            cx.listener(|this, _, window, cx| {
                                                this.show(
                                                    Some(this.import_provider.clone()),
                                                    None,
                                                    window,
                                                    cx,
                                                )
                                            }),
                                        ),
                                    )
                                })
                                .child(
                                    self.button(
                                        "subscription-import-retry",
                                        self.text("subscriptionImport.retry"),
                                        "refresh-cw",
                                        false,
                                    )
                                    .disabled(!retry || self.pending.is_some())
                                    .on_click(cx.listener(|this, _, _, cx| this.retry_import(cx))),
                                )
                                .child(
                                    self.button(
                                        "subscription-import-done",
                                        self.text("common.close"),
                                        "",
                                        true,
                                    )
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.close(window, cx)),
                                    ),
                                )
                            }),
                    ),
            )
            .observe("subscription-footer")
            .into_any_element()
    }
}
