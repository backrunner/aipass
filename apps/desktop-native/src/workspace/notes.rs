//! Read-only textarea retains selection/copy and wraps like ProviderItemFields.
use crate::{locale::Locale, parity::Observe, theme::Theme};
use gpui::{div, prelude::*, px, Context, Entity, FontWeight, Window};
use gpui_base::input::{InputAccessibility, Textarea, TextareaState};

pub(super) struct Notes {
    input: Entity<TextareaState>,
    identity: Option<(uuid::Uuid, String)>,
    theme: Theme,
    locale: Locale,
    height: gpui::Pixels,
    resize_start: Option<(gpui::Pixels, gpui::Pixels)>,
}
#[derive(Clone)]
struct ResizeNotes(gpui::EntityId);
impl Render for ResizeNotes {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
impl Notes {
    pub(crate) fn apply_appearance(&mut self, dark: bool, cx: &mut gpui::Context<Self>) {
        self.theme = Theme::new(dark);
        self.input.update(cx, |input, cx| {
            input.set_editor_style(self.theme.input_style());
            cx.notify();
        });
        cx.notify();
    }

    pub fn new(dark: bool, chinese: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let theme = Theme::new(dark);
        let locale = Locale::new(chinese);
        let input = cx.new(|cx| {
            let mut input = TextareaState::new(window, cx).rows(3);
            input.set_readonly(true, cx);
            input.set_editor_style(theme.input_style());
            input.set_accessibility(
                InputAccessibility::new(Some(gpui::Role::MultilineTextInput))
                    .with_label(Some(locale.text("providerDetail.notes").into())),
            );
            input
        });
        Self {
            input,
            identity: None,
            theme,
            locale,
            height: px(66.6),
            resize_start: None,
        }
    }
    pub fn sync(
        &mut self,
        entry: Option<&aipass_vault::EntrySummary>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let identity = entry.map(|e| (e.id, e.notes.clone().unwrap_or_default()));
        if self.identity != identity {
            self.resize_start = None;
            if identity.is_none() {
                self.height = px(66.6);
            }
            self.input.update(cx, |input, cx| {
                input.set_value(
                    identity
                        .as_ref()
                        .map(|(_, value)| value.clone())
                        .unwrap_or_default(),
                    window,
                    cx,
                )
            });
            self.identity = identity;
            cx.notify();
        }
    }
}
impl Render for Notes {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        div()
            .w_full()
            .flex_shrink_0()
            .bg(t.surface)
            .rounded(px(14.))
            .border_1()
            .border_color(t.border)
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(16.))
                    .py(px(12.))
                    .border_b_1()
                    .border_color(t.divider)
                    .text_size(px(13.))
                    .line_height(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.locale.text("providerDetail.notes"))
                    .observe("provider-notes-heading"),
            )
            .child(
                div()
                    .relative()
                    .mx(px(16.))
                    .my(px(10.))
                    .py(px(4.))
                    .h(self.height)
                    .border_b_1()
                    .border_color(gpui::rgba(0))
                    .text_size(px(12.))
                    .line_height(px(19.2))
                    .when(self.input.read(cx).value().is_empty(), |el| {
                        el.child(
                            div()
                                .absolute()
                                .top(px(4.))
                                .left(px(0.))
                                .text_color(t.tertiary)
                                .child(self.locale.text("providerForm.notes")),
                        )
                    })
                    .child(Textarea::new(&self.input))
                    .child(
                        div()
                            .id("provider-notes-resize")
                            .absolute()
                            .bottom_0()
                            .right_0()
                            .size(px(12.))
                            .cursor(gpui::CursorStyle::ResizeUpDown)
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                                    this.resize_start = Some((event.position.y, this.height));
                                    cx.stop_propagation();
                                }),
                            )
                            .on_drag(ResizeNotes(cx.entity_id()), |drag, _, _, cx| {
                                cx.stop_propagation();
                                cx.new(|_| drag.clone())
                            })
                            .on_drag_move(cx.listener(
                                |this, event: &gpui::DragMoveEvent<ResizeNotes>, _, cx| {
                                    if event.drag(cx).0 == cx.entity_id() {
                                        if let Some((y, height)) = this.resize_start {
                                            this.height =
                                                (height + event.event.position.y - y).max(px(64.));
                                            cx.notify();
                                        }
                                    }
                                },
                            ))
                            .children([3., 7.].map(|size| {
                                div()
                                    .absolute()
                                    .bottom(px(1.))
                                    .right(px(1.))
                                    .size(px(size))
                                    .border_b_1()
                                    .border_r_1()
                                    .border_color(t.tertiary.opacity(0.5))
                            }))
                            .observe("provider-notes-resize"),
                    )
                    .observe("provider-notes-value"),
            )
            .observe("provider-notes-card")
    }
}

#[cfg(all(test, feature = "parity-render"))]
mod tests {
    use super::*;
    use gpui::Focusable;
    #[gpui::test]
    fn notes_allow_copy_but_reject_edits_and_clear_on_lock(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_base::init);
        let mut notes = None;
        let entry = aipass_desktop_native::catalog::Catalog::fixture()
            .active
            .remove(0);
        let expected = entry.notes.clone().unwrap();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| Notes::new(false, false, window, cx));
            view.update(cx, |view, cx| {
                view.sync(Some(&entry), window, cx);
                view.input.read(cx).focus_handle(cx).focus(window, cx);
            });
            notes = Some(view.clone());
            gpui_base::Root::new(view, window, cx)
        });
        let view = notes.unwrap();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.simulate_keystrokes("cmd-a");
        cx.simulate_keystrokes("cmd-c");
        cx.update(|_, cx| {
            assert_eq!(
                cx.read_from_clipboard().and_then(|item| item.text()),
                Some(expected.clone())
            )
        });
        cx.simulate_input("replacement");
        cx.simulate_keystrokes("backspace");
        assert_eq!(
            view.read_with(cx, |view, cx| view.input.read(cx).value().to_string()),
            expected
        );
        // Resizing a read-only textarea must alter its viewport, not the text.
        let start = cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            let b = cx.global::<crate::parity::Geometry>().0["provider-notes-resize"];
            gpui::point(px(b[0] + b[2] / 2.), px(b[1] + b[3] / 2.))
        });
        cx.simulate_mouse_down(start, gpui::MouseButton::Left, Default::default());
        for dy in [10., 80.] {
            cx.simulate_mouse_move(
                start + gpui::point(px(0.), px(dy)),
                gpui::MouseButton::Left,
                Default::default(),
            );
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }
        cx.simulate_mouse_up(
            start + gpui::point(px(0.), px(80.)),
            gpui::MouseButton::Left,
            Default::default(),
        );
        assert!(view.read_with(cx, |view, _| view.height > px(130.)));
        assert_eq!(
            view.read_with(cx, |view, cx| view.input.read(cx).value().to_string()),
            expected
        );
        cx.update(|window, cx| view.update(cx, |view, cx| view.sync(None, window, cx)));
        assert!(view.read_with(cx, |view, cx| view.input.read(cx).value().is_empty()));
        assert_eq!(view.read_with(cx, |view, _| view.height), px(66.6));
    }
}
