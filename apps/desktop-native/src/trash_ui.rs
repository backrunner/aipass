//! Confirmation matches TrashConfirmModal; mutations are ordered by Session.
use crate::{locale::Locale, parity::Observe, providers::controls::ghost_button, theme::Theme};
use aipass_desktop_native::trash::{Completion, Request};
use gpui::{
    div, percentage, prelude::*, px, relative, svg, Animation, AnimationExt, Context, EventEmitter,
    FocusHandle, FontWeight, Transformation, Window,
};
use gpui_base::{Dialog, DialogPopup};
use std::time::Duration;
use uuid::Uuid;

pub struct Confirm {
    theme: Theme,
    locale: Locale,
    focus: FocusHandle,
    return_focus: Option<FocusHandle>,
    open: bool,
    target: Option<(Uuid, String)>,
    generation: u64,
    busy: bool,
    error: Option<String>,
    request: Option<Request>,
}
pub struct Requested;
impl EventEmitter<Requested> for Confirm {}

impl Confirm {
    pub(crate) fn apply_appearance(&mut self, dark: bool, cx: &mut gpui::Context<Self>) {
        self.theme = Theme::new(dark);
        cx.notify();
    }

    pub fn new(dark: bool, chinese: bool, cx: &mut Context<Self>) -> Self {
        Self {
            theme: Theme::new(dark),
            locale: Locale::new(chinese),
            focus: cx.focus_handle(),
            return_focus: None,
            open: false,
            target: None,
            generation: 0,
            busy: false,
            error: None,
            request: None,
        }
    }

    pub fn show(
        &mut self,
        target: Option<(Uuid, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.open {
            return;
        }
        self.generation += 1;
        self.return_focus = window.focused(cx);
        self.target = target;
        self.error = None;
        self.open = true;
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.clear(cx);
        if let Some(focus) = self.return_focus.take() {
            focus.focus(window, cx);
        }
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        self.open = false;
        self.target = None;
        self.error = None;
        self.busy = false;
        self.request = None;
        cx.notify();
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        if !self.open || self.busy {
            return;
        }
        self.generation += 1;
        self.busy = true;
        self.error = None;
        self.request = Some(Request {
            generation: self.generation,
            target: self.target.as_ref().map(|(id, _)| *id),
        });
        cx.emit(Requested);
        cx.notify();
    }

    pub fn take_request(&mut self) -> Option<Request> {
        self.request.take()
    }

    pub fn complete(
        &mut self,
        completion: Completion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.open || !self.busy || completion.generation != self.generation {
            return;
        }
        self.busy = false;
        match completion.result {
            Ok(()) => self.close(window, cx),
            Err(error) => {
                self.error = Some(error.message);
                cx.notify();
            }
        }
    }

    fn title(&self) -> String {
        match &self.target {
            Some((_, title)) => self
                .locale
                .text("confirm.deleteProvider")
                .replace("{title}", title),
            None => self.locale.text("confirm.emptyTrash"),
        }
    }
}

impl Render for Confirm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open {
            return div().into_any_element();
        }
        let t = self.theme;
        let owner = cx.entity().downgrade();
        let label = self.locale.text(if self.target.is_some() {
            "providerDetail.deleteForever"
        } else {
            "providerList.emptyTrash"
        });
        let content = DialogPopup::new()
            .w(px(420.))
            .max_h(window.viewport_size().height - px(64.))
            .rounded(px(14.))
            .border_1()
            .border_color(t.border)
            .bg(t.surface)
            .text_color(t.text)
            .overflow_hidden()
            .flex()
            .flex_col()
            .shadow(vec![gpui_base::box_shadow(
                0.,
                24.,
                crate::theme::shadow_blur(64.),
                0.,
                if t.bg.r < 0.2 {
                    gpui::rgba(0x00000080).into()
                } else {
                    gpui::rgba(0x0f111038).into()
                },
            )])
            .child(
                div()
                    .id("trash-confirm-content")
                    .overflow_y_scroll()
                    .min_h_0()
                    .border_b_1()
                    .border_color(t.divider)
                    .px(px(20.))
                    .py(px(18.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .size(px(36.))
                            .flex_shrink_0()
                            .rounded_full()
                            .bg(t.danger_soft)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                svg()
                                    .path("triangle-alert")
                                    .size(px(20.))
                                    .text_color(t.danger),
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
                                    .font_weight(FontWeight(650.))
                                    .child(self.title())
                                    .observe("trash-confirm-title"),
                            )
                            .child(
                                div()
                                    .mt(px(6.))
                                    .text_size(px(12.))
                                    .line_height(px(18.))
                                    .text_color(t.secondary)
                                    .child(self.locale.text("confirm.cannotUndo"))
                                    .observe("trash-confirm-description"),
                            )
                            .children(self.error.as_ref().map(|error| {
                                div()
                                    .mt(px(6.))
                                    .text_size(px(12.))
                                    .line_height(px(18.))
                                    .text_color(t.danger)
                                    .child(error.clone())
                                    .observe("trash-confirm-error")
                            })),
                    ),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(t.divider)
                    .px(px(20.))
                    .py(px(14.))
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        ghost_button("trash-cancel", false, t)
                            .accessibility_label(self.locale.text("common.cancel"))
                            .child(self.locale.text("common.cancel"))
                            .bg(gpui::transparent_black())
                            .border_color(gpui::transparent_black())
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
                    )
                    .child(
                        ghost_button("trash-confirm", true, t)
                            .relative()
                            .accessibility_label(label.clone())
                            .child(div().when(self.busy, |el| el.opacity(0.)).child(label))
                            .when(self.busy, |el| {
                                let spinner = svg()
                                    .path("spinner")
                                    .size(px(12.))
                                    .absolute()
                                    .left(relative(0.5))
                                    .top(relative(0.5))
                                    .ml(px(-6.))
                                    .mt(px(-6.))
                                    .text_color(t.danger);
                                el.child(if cx.reduce_motion() {
                                    spinner.into_any_element()
                                } else {
                                    spinner
                                        .with_animation(
                                            "trash-delete-spinner",
                                            Animation::new(Duration::from_millis(700)).repeat(),
                                            |icon, progress| {
                                                icon.with_transformation(Transformation::rotate(
                                                    percentage(progress),
                                                ))
                                            },
                                        )
                                        .into_any_element()
                                })
                            })
                            .bg(gpui::transparent_black())
                            .border_color(gpui::transparent_black())
                            .text_color(t.danger)
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, _, cx| this.confirm(cx))),
                    ),
            )
            .observe("trash-confirm-dialog");
        Dialog::new(cx)
            .focus_handle(self.focus.clone())
            .accessibility_label(self.title())
            .close_on_escape(!self.busy)
            .close_on_backdrop_press(!self.busy)
            .on_ok(|_, _, _| false)
            .on_open_change(move |open, _, window, cx| {
                if !open {
                    let _ = owner.update(cx, |this, cx| this.close(window, cx));
                }
            })
            .backdrop(
                div()
                    .absolute()
                    .inset_0()
                    .bg(gpui::rgb(0x0f1110).opacity(0.5))
                    .backdrop_blur(px(4.)),
            )
            .popup(content)
            .into_any_element()
    }
}

#[cfg(all(test, feature = "parity-render"))]
mod tests;
