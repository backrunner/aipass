use super::Workspace;
use aipass_desktop_native::provider_tools::{Completion, Request};
use gpui::{Context, Window};
impl Workspace {
    pub fn take_tools_request(&mut self, cx: &mut Context<Self>) -> Option<Request> {
        self.tools.update(cx, |tools, _| tools.take_request())
    }
    pub fn complete_tools(
        &mut self,
        completion: Completion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.provider_busy = false;
        let entry = self.tools.update(cx, |tools, cx| {
            tools.complete(completion, window, cx);
            tools.take_reset()
        });
        if let Some(entry) = entry {
            // The reset returns fresh metadata; never reopen an editor with
            // the removed keys from the previous catalog snapshot.
            for current in self
                .catalog
                .active
                .iter_mut()
                .chain(self.catalog.archived.iter_mut())
            {
                if current.id == entry.id {
                    *current = entry.clone();
                }
            }
            if entry.credential_kind == aipass_provider_registry::CredentialKind::OAuth {
                self.subscriptions.update(cx, |view, cx| {
                    view.show(entry.provider_id, Some(entry.id), window, cx)
                });
            } else {
                self.open_provider_form(Some(entry), window, cx);
            }
        }
        cx.notify();
    }
    pub(super) fn reset_provider(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.provider_busy || matches!(self.selected, 9 | 10) {
            return;
        }
        self.menu_open = false;
        self.tools
            .update(cx, |view, cx| view.show_reset(window, cx));
        cx.notify();
    }
}
