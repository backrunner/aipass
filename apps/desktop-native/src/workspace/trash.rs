use super::Workspace;
use aipass_desktop_native::trash::{Completion, Request};
use gpui::{Context, Window};
use uuid::Uuid;

impl Workspace {
    pub(super) fn confirm_trash(
        &mut self,
        target: Option<(Uuid, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.provider_busy || self.catalog.trash.is_empty() {
            return;
        }
        if let Some((id, _)) = &target {
            if !self.catalog.trash.iter().any(|entry| entry.id == *id) {
                return;
            }
        }
        self.trash
            .update(cx, |view, cx| view.show(target, window, cx));
    }
    pub fn take_trash_request(&mut self, cx: &mut Context<Self>) -> Option<Request> {
        self.trash.update(cx, |view, _| view.take_request())
    }
    pub fn complete_trash(
        &mut self,
        completion: Completion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.provider_busy = false;
        self.trash
            .update(cx, |view, cx| view.complete(completion, window, cx));
        cx.notify();
    }
}
