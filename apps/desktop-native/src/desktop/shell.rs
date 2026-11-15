//! Native tray and singleton events re-enter the existing UI/session owner.
use super::Desktop;
use aipass_desktop_native::{session::Command, shell::Event};
use gpui::{Context, Window};
impl Desktop {
    pub(crate) fn activate_server(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace
            .update(cx, |view, cx| view.open_server(window, cx));
    }
    pub(super) fn pump_shell(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for event in crate::startup::events(cx) {
            match event {
                Event::Probe(send) => {
                    let size = window.viewport_size();
                    let _ = send.send(aipass_desktop_native::runtime_check::probe(
                        f32::from(size.width),
                        f32::from(size.height),
                        crate::startup::tray_ready(cx),
                    ));
                }
                Event::Activate(target) => {
                    if target == "tray" {
                        crate::startup::hide(window, cx);
                    } else {
                        crate::startup::activate(window, cx);
                        if target == "server" {
                            self.activate_server(window, cx);
                        }
                    }
                }
                Event::Links(links) => {
                    if let Some(inbox) = cx.try_global::<super::provider_links::Inbox>() {
                        for link in links {
                            inbox.receive(link);
                        }
                    }
                }
                Event::Refresh => {
                    if let Some(session) = &mut self.session {
                        session.submit(Command::Refresh);
                    }
                }
                Event::Locked => self.lock(window, cx),
                Event::Quit => {
                    cx.quit();
                    return;
                }
                Event::OpenUrl(url) => cx.open_url(&url),
                Event::Copy(value) => cx.write_to_clipboard(gpui::ClipboardItem::new_string(value)),
                Event::Error(message) => self
                    .toast
                    .update(cx, |toast, cx| toast.show(message, window, cx)),
                Event::Tray(_) | Event::UnlockResult(_) => {}
            }
        }
    }
}
