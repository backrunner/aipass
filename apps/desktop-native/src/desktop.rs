use crate::{
    auth::{AuthEvent, AuthView, Mode},
    curtain::{Curtain, CurtainEvent},
    locale::Locale,
    theme::Theme,
    toast::ErrorToast,
    workspace::{ProviderFeedback, Workspace, WorkspaceEvent},
};
use aipass_desktop_native::{
    motion::Direction,
    session::{Command, Session},
};
use gpui::{div, prelude::*, px, rgb, Context, Entity, FocusHandle, Subscription, Task, Window};
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
    auth: Entity<AuthView>,
    workspace: Entity<Workspace>,
    toast: Entity<ErrorToast>,
    last_toast_error: Option<String>,
    toast_fixture: Option<bool>,
    session: Option<Session>,
    show_workspace: bool,
    chinese: bool,
    close_requested: bool,
    last_refresh: Instant,
    recovery_confirmed: bool,
    theme: Theme,
    locale: Locale,
    focus: FocusHandle,
    curtain: Option<Entity<Curtain>>,
    curtain_subscription: Option<Subscription>,
    motion_generation: u64,
    motion_only: bool,
    lock_covered: bool,
    last_unlocked: Option<bool>,
    _subscription: Subscription,
    _workspace_subscription: Subscription,
    pending_command: Option<Command>,
    _focus_subscription: Subscription,
    _poll: Task<()>,
}

impl Desktop {
    #[cfg(feature = "parity-render")]
    pub fn presentation_state(&self, cx: &gpui::App) -> serde_json::Value {
        serde_json::json!({"workspace": self.show_workspace, "providers": self.workspace.read(cx).presentation_state(cx)})
    }
    #[cfg(feature = "parity-render")]
    pub fn capture_ready(&self, cx: &gpui::App) -> Result<bool, String> {
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
                    "setup" => Mode::Choose,
                    "create" => Mode::Create,
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
                AuthEvent::Create(password) => Some(Command::Create {
                    password: password.clone(),
                    local_only: false,
                }),
                AuthEvent::Recover { key, password } => Some(Command::Recover {
                    key: key.clone(),
                    password: password.clone(),
                }),
                AuthEvent::Import => None,
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
                        this.last_refresh = Instant::now() - Duration::from_secs(2)
                    }
                    _ => {}
                }
                this.update_session(window, cx);
            });
        let show_workspace = session.is_none()
            && matches!(
                scenario,
                "empty"
                    | "menu"
                    | "filter"
                    | "toast"
                    | "toast-long"
                    | "providers"
                    | "providers-add"
                    | "providers-edit"
                    | "providers-add-advanced"
                    | "providers-edit-advanced"
                    | "providers-key-add"
                    | "providers-key-edit"
                    | "providers-key-billing"
                    | "providers-key-reveal"
                    | "providers-integration-diff"
                    | "providers-integration-full"
                    | "providers-integration-file2"
                    | "providers-integration-unchanged"
                    | "providers-integration-readonly"
            );
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
                .update_in(cx, |this, window, cx| this.update_session(window, cx))
                .is_err()
            {
                break;
            }
        });
        Self {
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
            recovery_confirmed: false,
            theme: Theme::new(dark),
            locale: Locale::new(chinese),
            focus,
            curtain,
            curtain_subscription: None,
            motion_generation: 0,
            motion_only,
            lock_covered: false,
            last_unlocked: None,
            _subscription: subscription,
            _workspace_subscription: workspace_subscription,
            pending_command: None,
            _focus_subscription: focus_subscription,
            _poll: poll,
        }
    }

    fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.session.is_none() {
            return true;
        }
        self.close_requested = true;
        self.lock(window, cx);
        false
    }

    fn lock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .curtain
            .as_ref()
            .is_some_and(|curtain| curtain.read(cx).direction == Direction::Lock)
        {
            return;
        }
        if let Some(session) = &mut self.session {
            session.submit(Command::Lock);
        }
        self.recovery_confirmed = false;
        self.pending_command = None;
        self.workspace
            .update(cx, |view, cx| view.clear_providers(window, cx));
        self.toast.update(cx, |toast, cx| toast.dismiss(cx));
        self.last_toast_error = None;
        self.focus.focus(window, cx);
        self.auth.update(cx, |auth, cx| {
            auth.clear(window, cx);
            auth.mode = Mode::Unlock;
            auth.set_busy(self.session.is_some(), cx);
        });
        if self.close_requested || !self.show_workspace {
            self.cancel_transition();
            self.show_workspace = false;
            self.auth.update(cx, |auth, cx| auth.focus(window, cx));
        } else {
            self.start_transition(Direction::Lock, window, cx);
        }
        cx.notify();
    }

    fn cancel_transition(&mut self) {
        self.motion_generation += 1;
        self.curtain_subscription = None;
        self.curtain = None;
        self.lock_covered = false;
    }

    fn start_transition(
        &mut self,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_transition();
        let generation = self.motion_generation;
        let curtain = cx.new(|_| Curtain::new(direction, self.theme));
        self.curtain_subscription =
            Some(
                cx.subscribe_in(&curtain, window, move |this, _, event, window, cx| {
                    if this.motion_generation != generation {
                        return;
                    }
                    match event {
                        CurtainEvent::Covered => {
                            this.lock_covered = direction == Direction::Lock;
                            this.show_workspace = direction == Direction::Unlock
                                && this.session.as_ref().is_none_or(Session::is_unlocked);
                            this.focus.focus(window, cx);
                            if !this.show_workspace {
                                this.auth.update(cx, |auth, cx| auth.focus(window, cx));
                            }
                        }
                        CurtainEvent::Done => {
                            this.curtain = None;
                            this.curtain_subscription = None;
                            this.lock_covered = false;
                        }
                    }
                    cx.notify();
                }),
            );
        self.curtain = Some(curtain);
        cx.notify();
    }

    fn update_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = &mut self.session else {
            return;
        };
        let changed = session.poll();
        // Read completion feedback before the next background refresh clears
        // Session.error. Repeated failures must not restart the toast forever.
        let completed_error =
            changed.then(|| session.error.as_ref().map(|error| error.message.clone()));
        let auth_error = session.error.as_ref().map(|error| {
            if error.code == Some(aipass_agent_protocol::AgentErrorCode::InvalidPassword) {
                self.locale.text("error.incorrectMasterPassword")
            } else {
                error.message.clone()
            }
        });
        if !session.pending && session.is_unlocked() && !self.close_requested {
            if let Some(action) = self.pending_command.take() {
                session.submit(action);
            }
        }
        if !self.close_requested
            && !session.pending
            && (session.provider_applied || self.last_refresh.elapsed() >= Duration::from_secs(1))
        {
            session.submit(Command::Refresh);
            self.last_refresh = Instant::now();
        }
        if !changed {
            return;
        }
        if self.close_requested
            && !session.pending
            && !session.lock_pending
            && session.error.is_none()
            && session
                .status
                .as_ref()
                .is_some_and(|status| status.locked || !status.exists)
        {
            window.remove_window();
            cx.quit();
            return;
        }
        let unlocked = session.is_unlocked();
        if unlocked {
            self.workspace.update(cx, |view, cx| {
                if let Some(completion) = session.integration_completion.take() {
                    view.complete_integration(completion, window, cx);
                }
                if let Some(completion) = session.credential_completion.take() {
                    view.complete_credential(completion, window, cx);
                }
                view.sync_providers(
                    session.catalog.clone(),
                    ProviderFeedback {
                        selected: session.selected,
                        busy: session.foreground_pending || self.pending_command.is_some(),
                        saved: session.provider_saved,
                        error: completed_error.clone().flatten(),
                    },
                    window,
                    cx,
                )
            });
        } else {
            self.pending_command = None;
            self.workspace
                .update(cx, |view, cx| view.clear_providers(window, cx));
        }
        if unlocked && !self.workspace.read(cx).has_provider_form() {
            if let Some(error) = completed_error {
                if error != self.last_toast_error {
                    if let Some(message) = &error {
                        self.toast
                            .update(cx, |toast, cx| toast.show(message.clone(), window, cx));
                    }
                    self.last_toast_error = error;
                }
            }
        } else {
            self.toast.update(cx, |toast, cx| toast.dismiss(cx));
            self.last_toast_error = None;
        }
        let was_unlocked = self.last_unlocked.replace(unlocked);
        let was_workspace = self.show_workspace;
        let locking = self
            .curtain
            .as_ref()
            .is_some_and(|curtain| curtain.read(cx).direction == Direction::Lock);
        let unlocking = self
            .curtain
            .as_ref()
            .is_some_and(|curtain| curtain.read(cx).direction == Direction::Unlock);
        if locking {
            self.show_workspace = !self.lock_covered;
        } else if !unlocking {
            self.show_workspace = unlocked;
        }
        self.auth.update(cx, |auth, cx| {
            auth.set_busy(session.foreground_pending || session.lock_pending, cx);
            if let Some(status) = &session.status {
                auth.sync_exists(status.exists, window, cx);
            }
            if unlocked {
                auth.clear(window, cx);
            }
            if let Some(error) = auth_error {
                auth.error = error;
            }
        });
        if was_unlocked == Some(false) && unlocked && !self.close_requested {
            self.show_workspace = false;
            self.start_transition(Direction::Unlock, window, cx);
        } else if !unlocked && unlocking {
            // Invalidation cancels the previous unlock callback immediately.
            self.cancel_transition();
            self.show_workspace = false;
        }
        if was_workspace != self.show_workspace {
            self.focus.focus(window, cx);
            if !self.show_workspace {
                self.auth.update(cx, |auth, cx| auth.focus(window, cx));
            }
        }
        cx.notify();
    }
}

impl Render for Desktop {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.motion_only {
            return div()
                .size_full()
                .relative()
                .rounded(px(12.))
                .overflow_hidden()
                .bg(self.theme.bg)
                .children(self.curtain.clone())
                .into_any_element();
        }
        let recovery = self
            .session
            .as_ref()
            .and_then(|s| s.recovery.as_ref())
            .map(|key| key.expose().to_owned());
        let root = div()
            .id("desktop")
            .track_focus(&self.focus)
            .size_full()
            .font_weight(gpui::FontWeight::NORMAL)
            .font_features(gpui::FontFeatures(std::sync::Arc::new(vec![
                ("ss01".into(), 1),
                ("cv11".into(), 1),
            ])))
            .relative()
            .rounded(px(12.))
            .overflow_hidden()
            .on_action(cx.listener(|this, _: &CloseWindow, window, cx| {
                if this.request_close(window, cx) {
                    window.remove_window();
                    cx.quit();
                }
            }))
            .on_action(cx.listener(|this, _: &LockVault, window, cx| this.lock(window, cx)))
            .child(if self.show_workspace {
                self.workspace.clone().into_any_element()
            } else {
                self.auth.clone().into_any_element()
            })
            .when(!self.show_workspace, |el| {
                el.child(
                    div()
                        .absolute()
                        .left(px(19.))
                        .top(px(19.))
                        .flex()
                        .gap(px(8.))
                        .children([0xff5f57, 0xfebc2e, 0x28c840].into_iter().enumerate().map(
                            |(index, color)| {
                                div()
                                    .id(("window-control", index))
                                    .size(px(12.))
                                    .rounded_full()
                                    .bg(rgb(color))
                                    .border_1()
                                    .border_color(gpui::rgba(0x00000026))
                                    .on_click(move |_, window, cx| match index {
                                        0 => window.dispatch_action(Box::new(CloseWindow), cx),
                                        1 => window.minimize_window(),
                                        _ => window.zoom_window(),
                                    })
                            },
                        )),
                )
            })
            .when_some(recovery, |el, key| {
                el.child(
                    div()
                        .absolute()
                        .inset_0()
                        .bg(gpui::rgba(0x080c188c))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .w(px(440.))
                                .p(px(28.))
                                .rounded(px(14.))
                                .bg(self.theme.surface)
                                .text_color(self.theme.text)
                                .flex()
                                .flex_col()
                                .gap(px(14.))
                                .child(
                                    div()
                                        .text_size(px(18.))
                                        .child(self.locale.text("recovery.title")),
                                )
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .child(self.locale.text("recovery.shortDesc")),
                                )
                                .child(
                                    div()
                                        .p(px(14.))
                                        .rounded(px(8.))
                                        .bg(self.theme.surface2)
                                        .font_family("Menlo")
                                        .text_size(px(13.))
                                        .child(key),
                                )
                                .child(
                                    div()
                                        .id("recovery-confirm")
                                        .cursor_pointer()
                                        .text_size(px(12.))
                                        .child(format!(
                                            "{} {}",
                                            if self.recovery_confirmed {
                                                "☑"
                                            } else {
                                                "☐"
                                            },
                                            self.locale.text("recovery.ack")
                                        ))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.recovery_confirmed = !this.recovery_confirmed;
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    div()
                                        .id("recovery-continue")
                                        .h(px(36.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(8.))
                                        .bg(rgb(0x2563eb))
                                        .text_color(rgb(0xffffff))
                                        .when(!self.recovery_confirmed, |el| el.opacity(0.5))
                                        .child(self.locale.text("recovery.continue"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if this.recovery_confirmed {
                                                if let Some(session) = &mut this.session {
                                                    session.recovery = None;
                                                }
                                                this.recovery_confirmed = false;
                                                cx.notify();
                                            }
                                        })),
                                ),
                        ),
                )
            })
            .children(self.curtain.clone())
            .child(self.toast.clone());
        #[cfg(feature = "parity-render")]
        let root = root.on_action(cx.listener(|this, _: &ReplayToast, window, cx| {
            if let Some(long) = this.toast_fixture.filter(|_| this.session.is_none()) {
                this.toast.update(cx, |toast, cx| {
                    toast.show(
                        crate::toast::fixture_message(this.chinese, long),
                        window,
                        cx,
                    )
                });
            }
        }));
        #[cfg(not(feature = "parity-render"))]
        let _ = self.toast_fixture;
        root.into_any_element()
    }
}

#[cfg(all(test, feature = "parity-render"))]
mod tests {
    use super::*;

    struct RejectPassword;
    impl aipass_desktop_native::session::Transport for RejectPassword {
        fn execute(
            &mut self,
            command: Command,
        ) -> Result<aipass_desktop_native::session::Reply, aipass_agent::AgentCommandError>
        {
            if matches!(command, Command::Unlock(_)) {
                return Err(aipass_agent::AgentCommandError {
                    code: Some(aipass_agent_protocol::AgentErrorCode::InvalidPassword),
                    message: "synthetic password rejection".into(),
                });
            }
            Ok(aipass_desktop_native::session::Reply::Snapshot {
                status: aipass_agent_protocol::SessionStatus {
                    exists: true,
                    locked: true,
                    policy: Default::default(),
                    last_lock_reason: None,
                    vault_namespace: None,
                    initial_sync_pending: false,
                    initial_sync_failed: false,
                    sync_revision: 0,
                    sync_status: None,
                },
                entries: vec![],
            })
        }
    }

    #[gpui::test]
    fn password_error_survives_a_background_refresh_due_on_completion(
        cx: &mut gpui::TestAppContext,
    ) {
        let (desktop, cx) = workspace(cx);
        cx.update(|_, cx| {
            desktop.update(cx, |view, _| {
                let mut session = Session::new(RejectPassword).unwrap();
                assert!(session.submit(Command::Unlock("synthetic-password".into())));
                view.session = Some(session);
                view.show_workspace = false;
                view.last_refresh = Instant::now() - Duration::from_secs(2);
            })
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let completed = cx.update(|window, cx| {
                desktop.update(cx, |view, cx| {
                    view.update_session(window, cx);
                    !view.session.as_ref().unwrap().foreground_pending
                })
            });
            if completed {
                break;
            }
            assert!(Instant::now() < deadline, "auth worker failed to complete");
            std::thread::yield_now();
        }
        cx.update(|_, cx| {
            let view = desktop.read(cx);
            assert_eq!(
                view.auth.read(cx).error,
                view.locale.text("error.incorrectMasterPassword")
            );
            assert!(!view.auth.read(cx).busy);
            assert!(!view.show_workspace);
        });
    }

    fn workspace(cx: &mut gpui::TestAppContext) -> (Entity<Desktop>, &mut gpui::VisualTestContext) {
        cx.update(|cx| {
            gpui_base::init(cx);
            init(cx);
        });
        let mut desktop = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| Desktop::new(None, "empty", false, false, window, cx));
            desktop = Some(view.clone());
            gpui_base::Root::new(view, window, cx)
        });
        (desktop.unwrap(), cx)
    }

    #[gpui::test]
    fn lock_keeps_workspace_until_cover_and_repeated_lock_does_not_restart(
        cx: &mut gpui::TestAppContext,
    ) {
        let (desktop, cx) = workspace(cx);
        let curtain = cx.update(|window, cx| {
            desktop.update(cx, |view, cx| {
                view.lock(window, cx);
                let curtain = view.curtain.clone().unwrap();
                curtain.update(cx, |curtain, _| curtain.freeze_at(0.));
                assert!(view.show_workspace);
                view.lock(window, cx);
                assert_eq!(
                    view.curtain.as_ref().unwrap().entity_id(),
                    curtain.entity_id()
                );
                curtain
            })
        });
        cx.update(|_, cx| curtain.update(cx, |_, cx| cx.emit(CurtainEvent::Covered)));
        assert!(!desktop.read_with(cx, |view, _| view.show_workspace));
        cx.update(|_, cx| curtain.update(cx, |_, cx| cx.emit(CurtainEvent::Done)));
        assert!(desktop.read_with(cx, |view, _| view.curtain.is_none()));
    }

    #[gpui::test]
    fn superseded_unlock_events_cannot_reveal_or_finish_the_later_lock(
        cx: &mut gpui::TestAppContext,
    ) {
        let (desktop, cx) = workspace(cx);
        let unlock = cx.update(|window, cx| {
            desktop.update(cx, |view, cx| {
                view.show_workspace = false;
                view.start_transition(Direction::Unlock, window, cx);
                let curtain = view.curtain.clone().unwrap();
                curtain.update(cx, |curtain, _| curtain.freeze_at(0.));
                curtain
            })
        });
        cx.update(|_, cx| unlock.update(cx, |_, cx| cx.emit(CurtainEvent::Covered)));
        assert!(desktop.read_with(cx, |view, _| view.show_workspace));
        let lock = cx.update(|window, cx| {
            desktop.update(cx, |view, cx| {
                view.lock(window, cx);
                let curtain = view.curtain.clone().unwrap();
                curtain.update(cx, |curtain, _| curtain.freeze_at(0.));
                curtain
            })
        });
        cx.update(|_, cx| lock.update(cx, |_, cx| cx.emit(CurtainEvent::Covered)));
        cx.update(|_, cx| {
            unlock.update(cx, |_, cx| {
                cx.emit(CurtainEvent::Covered);
                cx.emit(CurtainEvent::Done);
            })
        });
        assert!(desktop.read_with(cx, |view, _| !view.show_workspace
            && view.curtain.as_ref().unwrap().entity_id() == lock.entity_id()));
    }

    #[gpui::test]
    fn initial_workspace_focus_routes_window_actions(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_base::init(cx);
            init(cx);
            cx.set_reduce_motion(true);
        });
        let mut desktop = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| Desktop::new(None, "empty", false, false, window, cx));
            desktop = Some(view.clone());
            gpui_base::Root::new(view, window, cx)
        });
        let desktop = desktop.unwrap();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(desktop.read_with(cx, |desktop, _| desktop.show_workspace));
        cx.simulate_keystrokes("cmd-l");
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(!desktop.read_with(cx, |desktop, _| desktop.show_workspace));
        cx.update(|window, cx| {
            let view = desktop.read(cx);
            assert!(view.focus.contains_focused(window, cx));
            assert!(!view.focus.is_focused(window));
        });
    }
}
