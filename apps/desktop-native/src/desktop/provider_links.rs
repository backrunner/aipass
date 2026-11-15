//! OS links wait behind unlock and existing edits; only Save can issue a write.
use super::Desktop;
use crate::parity::Observe;
use aipass_desktop_native::{catalog::draft::Draft, provider_links};
use aipass_desktop_platform::deeplink::PendingDeepLink;
use gpui::{div, prelude::*, px, Context, FocusHandle, Global, Window};
use gpui_base::{Dialog, DialogPopup};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

#[derive(Clone, Default)]
pub struct Inbox(Rc<RefCell<(VecDeque<PendingDeepLink>, bool)>>);
impl Global for Inbox {}
impl Inbox {
    pub fn receive(&self, link: PendingDeepLink) {
        let mut inbox = self.0.borrow_mut();
        if inbox.0.len() < provider_links::QUEUE_LIMIT {
            inbox.0.push_back(link);
            inbox.1 = true;
        }
    }
    fn clear(&self) {
        self.0.borrow_mut().0.clear();
    }
}

pub(super) struct Links {
    duplicate: Option<(Draft, String)>,
    focus: FocusHandle,
    cancel_focus: FocusHandle,
    return_focus: Option<FocusHandle>,
}
impl Links {
    pub fn new(cx: &mut Context<Desktop>) -> Self {
        Self {
            duplicate: None,
            focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            return_focus: None,
        }
    }
    pub(super) fn clear(&mut self, cx: &gpui::App) {
        self.duplicate = None;
        self.return_focus = None;
        if let Some(inbox) = cx.try_global::<Inbox>() {
            inbox.clear();
        }
    }
}

impl Desktop {
    #[cfg(feature = "parity-render")]
    pub(crate) fn preview_provider_link(
        &mut self,
        long: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = if long {
            if self.chinese {
                "这是一个很长的服务商名称，用于检查窄窗口中的换行和确认按钮。".repeat(12)
            } else {
                "A deliberately long provider name for a wrapped confirmation dialog. ".repeat(12)
            }
        } else if self.chinese {
            "测试网关".into()
        } else {
            "Fixture gateway".into()
        };
        self.links.duplicate = Some((Draft::new(None), name));
        self.links.cancel_focus.focus(window, cx);
        cx.notify();
    }
    pub(super) fn clear_links(&mut self, cx: &gpui::App) {
        self.links.clear(cx);
    }
    pub(super) fn pump_links(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(inbox) = cx.try_global::<Inbox>().cloned() else {
            return;
        };
        if std::mem::take(&mut inbox.0.borrow_mut().1) {
            window.activate_window();
            cx.activate(true);
        }
        let ready = self
            .session
            .as_ref()
            .is_some_and(|s| s.is_unlocked() && !s.pending && s.error.is_none());
        if !ready
            || !self.show_workspace
            || self.close_requested
            || self.curtain.is_some()
            || self.links.duplicate.is_some()
            || self.settings.read(cx).open
            || self.workspace.read(cx).has_provider_form()
            || window.context_stack().iter().any(|context| {
                [
                    "Dialog",
                    "Popover",
                    "PopoverContent",
                    "AIPassMenu",
                    "AIPassFieldChoice",
                ]
                .iter()
                .any(|name| context.contains(name))
            })
        {
            return;
        }
        let pending = inbox.0.borrow_mut().0.pop_front();
        let Some(pending) = pending else {
            return;
        };
        match provider_links::into_draft(pending) {
            Ok(draft) => {
                let duplicate =
                    provider_links::duplicate(&self.session.as_ref().unwrap().catalog, &draft)
                        .map(|entry| entry.title.clone());
                if let Some(name) = duplicate {
                    self.links.return_focus = window.focused(cx);
                    self.links.duplicate = Some((draft, name));
                    self.links.cancel_focus.focus(window, cx);
                } else {
                    self.workspace
                        .update(cx, |s, cx| s.import_provider_draft(draft, window, cx));
                }
            }
            Err(message) => {
                let message = self
                    .locale
                    .text(message.key)
                    .replace(message.parameter, &message.value);
                self.toast.update(cx, |s, cx| s.show(message, window, cx));
            }
        }
        cx.notify();
    }
    fn dismiss_link(&mut self, apply: bool, window: &mut Window, cx: &mut Context<Self>) {
        let pending = self.links.duplicate.take();
        if let Some(focus) = self.links.return_focus.take() {
            focus.focus(window, cx);
        }
        if apply
            && self
                .session
                .as_ref()
                .is_some_and(|s| s.is_unlocked() && !s.foreground_pending)
        {
            if let Some((draft, _)) = pending {
                self.workspace
                    .update(cx, |s, cx| s.import_provider_draft(draft, window, cx));
            }
        }
        cx.notify();
    }
    pub(super) fn provider_link_dialog(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some((_, name)) = &self.links.duplicate else {
            return div().into_any_element();
        };
        let theme = self.theme;
        let title = self.locale.text("deepLink.duplicateTitle");
        let description = self
            .locale
            .text("deepLink.duplicateBody")
            .replace("{name}", name);
        let owner = cx.entity().downgrade();
        Dialog::new(cx)
            .layer(220, true)
            .focus_handle(self.links.focus.clone())
            .accessibility_label(title.clone())
            .on_ok(|_, _, _| false)
            .on_open_change(move |open, _, w, cx| {
                if !open {
                    let _ = owner.update(cx, |s, cx| s.dismiss_link(false, w, cx));
                }
            })
            .backdrop(
                div()
                    .absolute()
                    .inset_0()
                    .bg(gpui::rgb(0x0f1110).opacity(0.5))
                    .backdrop_blur(px(4.)),
            )
            .popup(
                DialogPopup::new()
                    .w(px(420.))
                    .max_h(window.viewport_size().height - px(64.))
                    .rounded(px(14.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.surface)
                    .text_color(theme.text)
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .shadow(vec![gpui_base::box_shadow(
                        0.,
                        24.,
                        crate::theme::shadow_blur(64.),
                        0.,
                        if theme.bg.r < 0.2 {
                            gpui::rgba(0x00000080).into()
                        } else {
                            gpui::rgba(0x0f111038).into()
                        },
                    )])
                    .child(
                        div()
                            .id("provider-link-message")
                            .min_h_0()
                            .overflow_y_scroll()
                            .px(px(20.))
                            .py(px(18.))
                            .border_b_1()
                            .border_color(theme.divider)
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                div()
                                    .size(px(36.))
                                    .flex_shrink_0()
                                    .rounded_full()
                                    .bg(theme.warning.opacity(if theme.bg.r < 0.2 {
                                        0.14
                                    } else {
                                        0.1
                                    }))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        gpui::svg()
                                            .path("triangle-alert")
                                            .size(px(20.))
                                            .text_color(theme.warning),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_size(px(16.))
                                            .line_height(px(18.))
                                            .font_weight(gpui::FontWeight(650.))
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .mt(px(6.))
                                            .text_size(px(12.))
                                            .line_height(px(18.))
                                            .text_color(theme.secondary)
                                            .child(description),
                                    ),
                            )
                            .observe("provider-link-message"),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .border_t_1()
                            .border_color(theme.divider)
                            .px(px(20.))
                            .py(px(14.))
                            .flex()
                            .justify_end()
                            .gap(px(8.))
                            .child(
                                crate::providers::controls::ghost_button(
                                    "provider-link-cancel",
                                    false,
                                    theme,
                                )
                                .accessibility_label(self.locale.text("common.cancel"))
                                .child(self.locale.text("common.cancel"))
                                .track_focus(&self.links.cancel_focus)
                                .on_click(cx.listener(|s, _, w, cx| s.dismiss_link(false, w, cx))),
                            )
                            .child(
                                crate::providers::controls::button(
                                    "provider-link-confirm",
                                    self.locale.text("deepLink.duplicateConfirm"),
                                    true,
                                    theme,
                                )
                                .on_click(cx.listener(|s, _, w, cx| s.dismiss_link(true, w, cx))),
                            ),
                    )
                    .observe("provider-link-dialog"),
            )
            .into_any_element()
    }
}

#[cfg(all(test, feature = "parity-render"))]
mod tests;
