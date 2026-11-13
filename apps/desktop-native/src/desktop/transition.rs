//! Window close, lock and curtain state transitions.
use super::Desktop;
use crate::{
    auth::Mode,
    curtain::{Curtain, CurtainEvent},
};
use aipass_desktop_native::{
    motion::Direction,
    session::{Command, Session},
};
use gpui::{AppContext, Context, Window};

impl Desktop {
    pub(super) fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.session.is_none() {
            return true;
        }
        self.close_requested = true;
        self.lock(window, cx);
        false
    }

    pub(super) fn lock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(worker) = &self.updater {
            worker.cancel_confirmation();
        }
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

    pub(super) fn cancel_transition(&mut self) {
        self.motion_generation += 1;
        self.curtain_subscription = None;
        self.curtain = None;
        self.lock_covered = false;
    }

    pub(super) fn start_transition(
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
}
