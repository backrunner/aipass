use super::*;
use aipass_desktop_platform::clipboard::CopyPermit;
impl Proxy {
    pub(super) fn cancel_copy(&mut self) {
        if let Some(cancel) = self.copy_cancel.take() {
            cancel.cancel();
        }
        self.copying = false;
    }
    pub(super) fn copy_token(&mut self, route: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy() || self.copying {
            return;
        }
        let permit = match CopyPermit::begin() {
            Ok(Some(p)) => p,
            Ok(None) => return,
            Err(e) => {
                self.error = e;
                cx.notify();
                return;
            }
        };
        self.copy_cancel = Some(permit.cancellation());
        self.copying = true;
        let generation = self.generation;
        let preferences = cx.global::<crate::preferences::Preferences>().0.clone();
        let prompt = if preferences.confirm_secret_copy {
            let (t, d, c, x) =
                aipass_desktop_platform::clipboard::confirmation(self.locale.language());
            Some(window.prompt(gpui::PromptLevel::Warning, t, Some(d), &[c, x], cx))
        } else {
            None
        };
        cx.spawn(async move |this, cx| {
            let accepted = match prompt {
                Some(p) => p.await == Ok(0),
                None => true,
            };
            let token = this
                .update(cx, |s, cx| {
                    if generation != s.generation || !s.active || !accepted || !permit.is_current()
                    {
                        s.cancel_copy();
                        cx.notify();
                        return None;
                    }
                    s.config
                        .as_ref()
                        .and_then(|c| c.routes.iter().find(|r| r.id == route))
                        .map(|r| aipass_agent_protocol::SensitiveString::new(r.token.clone()))
                })
                .ok()
                .flatten();
            let result = if let Some(token) = token {
                cx.background_executor()
                    .spawn(async move { permit.write(token, preferences.clipboard_clear_seconds) })
                    .await
            } else {
                Ok(false)
            };
            let _ = this.update(cx, |s, cx| {
                if s.generation != generation {
                    return;
                }
                s.copying = false;
                s.copy_cancel = None;
                match result {
                    Err(e) => s.error = e,
                    Ok(true) => s.notice = s.locale.text("providerDetail.copied"),
                    _ => {}
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
