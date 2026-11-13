//! Apply Agent completions before scheduling the next refresh.
use super::Desktop;
use crate::workspace::ProviderFeedback;
use aipass_desktop_native::{motion::Direction, session::Command};
use gpui::{Context, Window};
use std::time::{Duration, Instant};

impl Desktop {
    pub(super) fn update_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .runtime_gate
            .as_ref()
            .is_some_and(|gate| gate.suspended())
        {
            return;
        }
        let subscriptions = self.workspace.read(cx).subscriptions.clone();
        subscriptions.update(cx, |view, cx| view.tick(cx));
        let Some(session) = &mut self.session else {
            #[cfg(feature = "parity-render")]
            subscriptions.update(cx, |view, cx| view.run_preview(window, cx));
            return;
        };
        let changed = session.poll();
        if let Some(completion) = session.subscription_completion.take() {
            subscriptions.update(cx, |view, cx| view.complete(completion, window, cx));
        }
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
            } else if let Some(request) = subscriptions.update(cx, |view, _| view.take_request()) {
                session.submit(Command::Subscription(Box::new(request)));
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
                if let Some(completion) = session.tools_completion.take() {
                    view.complete_tools(completion, window, cx);
                }
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
        if was_unlocked == Some(true) && !unlocked {
            if let Some(worker) = &self.updater {
                worker.cancel_confirmation();
            }
        }
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
