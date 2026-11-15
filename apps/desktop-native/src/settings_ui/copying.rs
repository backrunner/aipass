use super::*;
use aipass_desktop_platform::clipboard::CopyPermit;
impl Settings {
    pub(super) fn copy_access_code(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.copying {
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
                    if generation != s.generation || !s.open || !accepted || !permit.is_current() {
                        s.hide_access_code();
                        cx.notify();
                        return None;
                    }
                    s.access_code
                        .as_ref()
                        .filter(|(_, at)| at.elapsed().as_secs() < 60)
                        .map(|(code, _)| aipass_agent_protocol::SensitiveString::new(code.expose()))
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
