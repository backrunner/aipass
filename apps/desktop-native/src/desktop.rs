mod activity;
mod appearance;
mod session_sync;
#[cfg(all(test, feature = "parity-render"))]
mod tests;
mod transition;
mod view;

use crate::{
    auth::{AuthEvent, AuthView, Mode},
    curtain::Curtain,
    locale::Locale,
    theme::Theme,
    toast::ErrorToast,
    workspace::{ProviderFeedback, Workspace, WorkspaceEvent},
};
use aipass_desktop_native::{
    motion::Direction,
    session::{Command, Session},
};
use gpui::{prelude::*, Context, Entity, FocusHandle, Subscription, Task, Window};
use std::time::{Duration, Instant};

gpui::actions!(desktop, [CloseWindow, LockVault]);
#[cfg(feature = "parity-render")]
gpui::actions!(parity, [ReplayToast]);

pub fn init(cx: &mut gpui::App) {
    #[cfg(feature = "parity-render")]
    cx.bind_keys([gpui::KeyBinding::new("cmd-shift-t", ReplayToast, None)]);
    cx.bind_keys([
        gpui::KeyBinding::new("cmd-l", LockVault, None),
        gpui::KeyBinding::new("cmd-w", CloseWindow, None),
        gpui::KeyBinding::new("cmd-q", CloseWindow, None),
    ]);
}

pub struct Desktop {
    appearance: crate::appearance::Appearance,
    _appearance_subscription: Subscription,
    _preferences_subscription: Option<Subscription>,
    pub(crate) updater: Option<aipass_desktop_native::updates::Worker>,
    pub(crate) update_focus: gpui::FocusHandle,
    pub(crate) runtime_gate: Option<std::sync::Arc<aipass_desktop_native::runtime::Gate>>,
    auth: Entity<AuthView>,
    workspace: Entity<Workspace>,
    toast: Entity<ErrorToast>,
    last_toast_error: Option<String>,
    toast_fixture: Option<bool>,
    session: Option<Session>,
    pub(crate) show_workspace: bool,
    chinese: bool,
    close_requested: bool,
    last_refresh: Instant,
    recovery: Entity<crate::recovery_kit::RecoveryKit>,
    pub(crate) theme: Theme,
    pub(crate) locale: Locale,
    pub(crate) focus: FocusHandle,
    pub(crate) curtain: Option<Entity<Curtain>>,
    curtain_subscription: Option<Subscription>,
    motion_generation: u64,
    motion_only: bool,
    loading_preview: bool,
    lock_covered: bool,
    last_unlocked: Option<bool>,
    _subscription: Subscription,
    _workspace_subscription: Subscription,
    pending_command: Option<Command>,
    _focus_subscription: Subscription,
    _activation_subscription: Subscription,
    _poll: Task<()>,
}

impl Desktop {
    #[cfg(feature = "parity-render")]
    pub fn prepare_capture_scroll(&self, scenario: &str, cx: &gpui::App) {
        if scenario.starts_with("subscription-overview") {
            self.workspace
                .read(cx)
                .prepare_subscription_overview_scroll();
        }
        if scenario.starts_with("subscription-runtime-") && scenario.ends_with("-bottom") {
            self.workspace.read(cx).prepare_subscription_scroll(cx);
        }
        if scenario.starts_with("providers-notes-") {
            self.workspace.read(cx).prepare_notes_scroll();
        }
        if scenario == "reset-bottom" {
            self.auth.read(cx).prepare_capture_scroll();
        }
        if scenario.starts_with("providers-")
            && (scenario.ends_with("-advanced")
                || scenario.ends_with("-fields")
                || scenario.ends_with("-gateway")
                || scenario.ends_with("-menu")
                || scenario.ends_with("-all"))
        {
            self.workspace.read(cx).prepare_form_scroll(cx);
        }
    }
    #[cfg(feature = "parity-render")]
    pub fn presentation_state(&self, cx: &gpui::App) -> serde_json::Value {
        serde_json::json!({"workspace": self.show_workspace, "providers": self.workspace.read(cx).presentation_state(cx)})
    }
    #[cfg(feature = "parity-render")]
    pub fn capture_ready(&self, cx: &gpui::App) -> Result<bool, String> {
        if self.loading_preview {
            return Ok(true);
        }
        if self.motion_only {
            Ok(true)
        } else if self.show_workspace {
            self.workspace.read(cx).capture_ready()
        } else {
            self.auth.read(cx).capture_ready()
        }
    }
    #[cfg(feature = "parity-render")]
    pub fn freeze_motion_at(&mut self, elapsed_ms: f64, cx: &mut Context<Self>) {
        if let Some(curtain) = &self.curtain {
            curtain.update(cx, |curtain, _| curtain.freeze_at(elapsed_ms));
        }
    }
    pub fn new(
        mut session: Option<Session>,
        scenario: &str,
        dark: bool,
        chinese: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let auth = cx.new(|cx| {
            AuthView::new(
                match scenario {
                    s if s.starts_with("setup-import-") => Mode::Import,
                    s if s.starts_with("setup") => Mode::Choose,
                    "create" | "create-local" => Mode::Create,
                    "recover" | "reset" | "reset-bottom" => Mode::Recover,
                    _ => Mode::Unlock,
                },
                dark,
                chinese,
                window,
                cx,
            )
        });
        auth.update(cx, |auth, cx| auth.preview_state(scenario, window, cx));
        let workspace = cx.new(|cx| Workspace::new(dark, chinese, window, cx));
        workspace.update(cx, |workspace, _| workspace.preview_state(scenario));
        #[cfg(feature = "parity-render")]
        workspace.update(cx, |workspace, cx| {
            workspace.preview_providers(scenario, window, cx)
        });
        let recovery = cx.new(|cx| crate::recovery_kit::RecoveryKit::new(dark, chinese, cx));
        #[cfg(feature = "parity-render")]
        if scenario.starts_with("recovery-kit") {
            recovery.update(cx, |v, cx| v.preview(scenario, window, cx));
        }
        let toast = cx.new(|cx| ErrorToast::new(dark, chinese, window, cx));
        if matches!(scenario, "toast" | "toast-long") {
            toast.update(cx, |toast, cx| {
                toast.show(
                    crate::toast::fixture_message(chinese, scenario == "toast-long"),
                    window,
                    cx,
                )
            });
        }
        let subscription = cx.subscribe_in(&auth, window, |this, _, event, window, cx| {
            let command = match event {
                AuthEvent::Unlock(password) => Some(Command::Unlock(password.clone())),
                AuthEvent::Create {
                    password,
                    local_only,
                } => Some(Command::Create {
                    password: password.clone(),
                    local_only: *local_only,
                }),
                AuthEvent::Recover { key, password } => Some(Command::Recover {
                    key: key.clone(),
                    password: password.clone(),
                }),
                AuthEvent::Import(request) => Some(Command::Import(request.clone())),
                AuthEvent::CheckCloud => Some(Command::CheckCloud),
                AuthEvent::Reset => Some(Command::Reset),
            };
            if let (Some(session), Some(command)) = (&mut this.session, command) {
                if !session.submit(command) {
                    this.auth.update(cx, |auth, cx| {
                        auth.set_busy(session.foreground_pending || session.lock_pending, cx)
                    });
                }
            } else {
                this.auth.update(cx, |auth, cx| {
                    auth.set_busy(false, cx);
                    auth.error = if this.chinese {
                        "此迁移预览尚未接通该操作。"
                    } else {
                        "This operation is not connected in the migration preview."
                    }
                    .into();
                });
            }
            this.update_session(window, cx);
        });
        let workspace_subscription =
            cx.subscribe_in(&workspace, window, |this, view, event, window, cx| {
                match event {
                    WorkspaceEvent::Provider => {
                        let action = view.update(cx, |view, _| view.take_provider_action());
                        if this.session.as_ref().is_some_and(Session::is_unlocked) {
                            this.pending_command = action.map(Command::Provider);
                        } else {
                            view.update(cx, |view, cx| {
                                view.sync_providers(
                                    Default::default(),
                                    ProviderFeedback { error: Some(if this.session.is_some() {
                                        this.locale.text("ext.locked.title")
                                    } else if this.chinese {
                                        "此迁移预览尚未接通该操作。".into()
                                    } else {
                                        "This operation is not connected in the migration preview."
                                            .into()
                                    }), ..Default::default() },
                                    window,
                                    cx,
                                )
                            });
                        }
                    }
                    WorkspaceEvent::ProviderTools => {
                        if let Some(request) = view.update(cx, |view,cx|view.take_tools_request(cx)) {
                            if this.session.as_ref().is_some_and(|s|s.is_unlocked()) {
                                this.pending_command = Some(Command::ProviderTools(Box::new(request)));
                            } else {
                                let completion = aipass_desktop_native::provider_tools::Completion { scope: request.scope, mutated: false, result: Err(aipass_agent::AgentCommandError { code: None, message: this.locale.text("tray.agentUnavailable") }) };
                                view.update(cx, |view,cx|view.complete_tools(completion,window,cx));
                            }
                        }
                    }
                    WorkspaceEvent::Trash => {
                        if let Some(request) = view.update(cx, |view, cx| view.take_trash_request(cx)) {
                            if this.session.as_ref().is_some_and(Session::is_unlocked) {
                                this.pending_command = Some(Command::Trash(request));
                            } else {
                                let completion = aipass_desktop_native::trash::Completion {
                                    generation: request.generation,
                                    result: Err(aipass_agent::AgentCommandError { code: None, message: this.locale.text("tray.agentUnavailable") }),
                                };
                                view.update(cx, |view, cx| view.complete_trash(completion, window, cx));
                            }
                        }
                    }
                    WorkspaceEvent::Credential => {
                        let request = view.update(cx, |view, cx| view.take_credential_request(cx));
                        if let Some(request) = request {
                            if this.session.as_ref().is_some_and(Session::is_unlocked) {
                                this.pending_command = Some(Command::Credential(Box::new(request)));
                            } else {
                                let completion = aipass_desktop_native::credentials::Completion {
                                    scope: request.scope,
                                    result: Err(aipass_agent::AgentCommandError {
                                        code: None,
                                        message: if this.chinese { "此迁移预览尚未接通该操作。" } else {
                                            "This operation is not connected in the migration preview."
                                        }.into(),
                                    }),
                                };
                                view.update(cx, |view, cx| {
                                    view.complete_credential(completion, window, cx);
                                });
                            }
                        }
                    }
                    WorkspaceEvent::Integration => {
                        let request = view.update(cx, |view, cx| view.take_integration_request(cx));
                        if let Some(request) = request {
                            if this.session.as_ref().is_some_and(Session::is_unlocked) {
                                this.pending_command = Some(Command::Integration(Box::new(request)));
                            } else {
                                let completion = aipass_desktop_native::integration::Completion {
                                    scope: request.scope,
                                    result: Err(aipass_agent::AgentCommandError {
                                        code: None,
                                        message: if this.chinese { "此迁移预览尚未接通该操作。" } else {
                                            "This operation is not connected in the migration preview."
                                        }.into(),
                                    }),
                                };
                                view.update(cx, |view, cx| {
                                    view.complete_integration(completion, window, cx);
                                });
                            }
                        }
                    }
                    WorkspaceEvent::Refresh => {
                        view.read(cx).subscriptions.clone().update(cx,|view,cx|view.show_import(window,cx));
                    }
                    WorkspaceEvent::Connect => {view.read(cx).subscriptions.clone().update(cx,|view,cx|view.show(None,None,window,cx));}
                    _ => {}
                }
                this.update_session(window, cx);
            });
        let show_workspace = session.is_none()
            && (scenario.starts_with("agents-")
                || scenario.starts_with("recovery-kit")
                || scenario.starts_with("subscription-")
                || scenario.starts_with("providers-trash")
                || matches!(
                    scenario,
                    "empty"
                        | "menu"
                        | "filter"
                        | "toast"
                        | "toast-long"
                        | "providers"
                        | "providers-reset"
                        | "providers-reset-error"
                        | "providers-usage"
                        | "providers-usage-advanced"
                        | "providers-usage-result"
                        | "providers-usage-failed"
                        | "providers-usage-busy"
                        | "providers-probe-result"
                        | "providers-notes-normal"
                        | "providers-notes-empty"
                        | "providers-notes-long"
                        | "providers-list"
                        | "providers-list-hover"
                        | "providers-list-selected-hover"
                        | "providers-add"
                        | "providers-add-menu"
                        | "providers-add-all"
                        | "providers-edit"
                        | "providers-add-advanced"
                        | "providers-edit-advanced"
                        | "providers-edit-fields"
                        | "providers-edit-gateway"
                        | "providers-key-add"
                        | "providers-key-edit"
                        | "providers-key-billing"
                        | "providers-key-reveal"
                        | "providers-pricing"
                        | "providers-pricing-reference"
                        | "providers-pricing-source"
                        | "providers-pricing-group"
                        | "providers-pricing-confirm"
                        | "providers-pricing-delete"
                        | "providers-pricing-history"
                        | "providers-pricing-offpeak"
                        | "providers-integration-picker"
                        | "providers-integration-diff"
                        | "providers-integration-full"
                        | "providers-integration-file2"
                        | "providers-integration-unchanged"
                        | "providers-integration-readonly"
                ));
        let motion_only = matches!(scenario, "unlock-motion" | "lock-motion");
        let curtain = motion_only.then(|| {
            cx.new(|_| {
                Curtain::new(
                    if scenario == "lock-motion" {
                        Direction::Lock
                    } else {
                        Direction::Unlock
                    },
                    Theme::new(dark),
                )
            })
        });
        let focus = cx.focus_handle();
        let focus_subscription = cx.on_focus_lost(window, |this, window, cx| {
            if this.show_workspace {
                this.focus.focus(window, cx);
            } else {
                this.auth.update(cx, |auth, cx| auth.focus(window, cx));
            }
        });
        let activation_subscription = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() && this.session.as_ref().is_some_and(Session::is_unlocked)
            {
                let agents = this.workspace.read(cx).agents.clone();
                agents.update(cx, |view, cx| view.focused(cx));
            }
        });
        if show_workspace || window.focused(cx).is_none() {
            // AuthView initializes its input focus, but it is absent from the
            // workspace tree. Keep window actions on a mounted dispatch path.
            focus.focus(window, cx);
        }
        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |this, cx| this.request_close(window, cx))
                .unwrap_or(true)
        });
        if let Some(session) = &mut session {
            session.submit(Command::Refresh);
        }
        let poll = cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
            if this
                .update_in(cx, |this, window, cx| {
                    if this
                        .updater
                        .as_ref()
                        .is_some_and(|worker| worker.snapshot().restarting)
                    {
                        cx.quit();
                        return;
                    }
                    if this.updater.is_some() {
                        cx.notify();
                    }
                    this.update_session(window, cx);
                })
                .is_err()
            {
                break;
            }
        });
        let appearance_subscription = cx.observe_window_appearance(window, |this, window, cx| {
            this.appearance.requested =
                crate::appearance::resolve(this.appearance.preference, window.appearance());
            cx.notify();
        });
        Self {
            appearance: crate::appearance::Appearance::new(dark),
            _appearance_subscription: appearance_subscription,
            _preferences_subscription: None,
            updater: None,
            update_focus: cx.focus_handle(),
            runtime_gate: None,
            auth,
            workspace,
            toast,
            last_toast_error: None,
            toast_fixture: matches!(scenario, "toast" | "toast-long")
                .then_some(scenario == "toast-long"),
            session,
            show_workspace,
            chinese,
            close_requested: false,
            last_refresh: Instant::now(),
            recovery,
            theme: Theme::new(dark),
            locale: Locale::new(chinese),
            focus,
            curtain,
            curtain_subscription: None,
            motion_generation: 0,
            motion_only,
            loading_preview: scenario == "boot",
            lock_covered: false,
            last_unlocked: None,
            _subscription: subscription,
            _workspace_subscription: workspace_subscription,
            pending_command: None,
            _focus_subscription: focus_subscription,
            _activation_subscription: activation_subscription,
            _poll: poll,
        }
    }
}
