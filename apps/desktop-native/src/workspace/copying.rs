//! Public provider metadata uses the platform clipboard, without a secret read.
use super::Workspace;
use gpui::{ClipboardItem, Context, Window};
use std::time::{Duration, Instant};
use uuid::Uuid;

impl Workspace {
    pub(super) fn copy_endpoint(
        &mut self,
        entry: Uuid,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_provider != Some(entry) {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(value));
        let copied = (entry, Instant::now());
        self.copied_endpoint = Some(copied);
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1800))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.copied_endpoint == Some(copied) {
                    this.copied_endpoint = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }
}
