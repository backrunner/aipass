use crate::parity::Observe;
use crate::theme::Theme;
use gpui::{div, prelude::*, px, svg, Anchor, Context, EventEmitter, FontWeight, Window};
use gpui_base::{Button, Popover};
pub struct Selected(pub String);
pub struct Choice {
    pub selected: String,
    pub placeholder: String,
    pub disabled: bool,
    pub next: Option<gpui::FocusHandle>,
    pub label: String,
    pub aria_label: Option<String>,
    pub options: Vec<(String, String)>,
    pub theme: Theme,
    open: bool,
    focus: crate::menu::MenuFocus,
    bounds: std::rc::Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
}
impl EventEmitter<Selected> for Choice {}
impl Choice {
    pub fn new(
        label: String,
        options: Vec<(String, String)>,
        selected: &str,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Self {
        crate::menu::init(cx);
        Self {
            focus: crate::menu::MenuFocus::new(options.len(), cx),
            selected: selected.into(),
            placeholder: String::new(),
            disabled: false,
            next: None,
            label,
            aria_label: None,
            options,
            theme,
            open: false,
            bounds: Default::default(),
        }
    }
    pub fn set_options(&mut self, options: Vec<(String, String)>, cx: &mut Context<Self>) {
        if self.options != options {
            self.focus.items = (0..options.len()).map(|_| cx.focus_handle()).collect();
            self.options = options;
            self.open = false;
        }
    }
    #[cfg(feature = "parity-render")]
    pub fn preview_open(&mut self) {
        self.open = true;
    }
    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        cx.notify();
    }
}
impl Render for Choice {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let open = self.open && !self.disabled;
        let options = self.options.clone();
        let selected = options.iter().position(|(id, _)| *id == self.selected);
        let value = options
            .get(selected.unwrap_or(usize::MAX))
            .map(|(_, text)| text.clone())
            .unwrap_or_else(|| self.placeholder.clone());
        let frame = crate::menu::motion::sample("form-choice-motion", open, window, cx);
        let origin = crate::menu::motion::Origin::new(false);
        let track = origin.clone();
        let bounds = self.bounds.clone();
        let width = bounds.get().size.width.max(px(1.));
        let scroll = window
            .use_keyed_state("form-choices-scroll", cx, |_, _| gpui::ScrollHandle::new())
            .read(cx)
            .clone();
        let mut focus = self.focus.clone();
        focus.scroll = Some(scroll.clone());
        let initial = focus.clone();
        let keyboard = focus.clone();
        let owner = cx.entity().downgrade();
        let close = owner.clone();
        let next = self.next.clone().unwrap_or_else(|| focus.trigger.clone());
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .on_children_prepainted(move |children, _, cx| {
                if let Some(rect) = children.last() {
                    bounds.set(*rect);
                    crate::parity::record_bounds("form-choice-trigger", *rect, cx);
                }
            })
            .child(crate::form_ui::label(self.label.clone(), t).font_weight(FontWeight::MEDIUM))
            .child(
                Popover::new("form-choice-picker")
                    .open(open)
                    .retain_content(frame.visible)
                    .anchor(Anchor::TopLeft)
                    .offset(px(4.))
                    .track_focus(&focus.container)
                    .on_position(move |position, trigger| track.resolve(position, trigger))
                    .on_open_change(cx.listener(move |this, open, window, cx| {
                        this.open = *open && !this.disabled;
                        if this.open {
                            initial.focus_item(selected.unwrap_or(0), window, cx);
                        }
                        cx.notify();
                    }))
                    .trigger(
                        Button::new("form-choice")
                            .disabled(self.disabled)
                            .track_focus(&focus.trigger)
                            .w_full()
                            .h(px(34.))
                            .px(px(10.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(if open { t.accent } else { t.border })
                            .bg(t.surface)
                            .text_color(t.text)
                            .text_size(px(13.))
                            .font_weight(FontWeight::NORMAL)
                            .line_height(px(20.))
                            .when(open, |el| el.shadow(t.outline(t.focus_ring, 3., 0.)))
                            .focus_visible(move |s| s.shadow(t.outline(t.focus_ring, 3., 0.)))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .accessibility_label(
                                self.aria_label
                                    .clone()
                                    .unwrap_or_else(|| self.label.clone()),
                            )
                            .child(div().flex_1().min_w_0().text_ellipsis().child(value))
                            .child(
                                svg()
                                    .path("chevron-down")
                                    .size(px(14.))
                                    .text_color(t.secondary),
                            )
                            .on_key_down(cx.listener(
                                move |this, event: &gpui::KeyDownEvent, window, cx| {
                                    if matches!(event.keystroke.key.as_str(), "down" | "up")
                                        && !this.disabled
                                    {
                                        this.open = true;
                                        let focus = keyboard.clone();
                                        window.on_next_frame(move |window, cx| {
                                            focus.focus_item(selected.unwrap_or(0), window, cx)
                                        });
                                        cx.stop_propagation();
                                        cx.notify();
                                    }
                                },
                            )),
                    )
                    .content(move |_, _, _| {
                        let surface = focus.surface(
                            "form-choices-surface",
                            next,
                            focus.trigger.clone(),
                            move |_, cx| {
                                let _ = close.update(cx, |this, cx| {
                                    this.open = false;
                                    cx.notify();
                                });
                            },
                        );
                        let items = div()
                            .id("form-choices-scroll")
                            .max_h(px(278.))
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
                            .flex()
                            .flex_col()
                            .children(options.into_iter().enumerate().map(
                                |(index, (id, label))| {
                                    let owner = owner.clone();
                                    crate::menu::select::item(
                                        ("form-choice-option", index),
                                        Some(index) == selected,
                                        t,
                                    )
                                    .disabled(!open)
                                    .track_focus(&focus.items[index])
                                    .accessibility_label(label.clone())
                                    .child(div().flex_1().min_w_0().text_ellipsis().child(label))
                                    .child(crate::menu::select::indicator(
                                        Some(index) == selected,
                                        t,
                                    ))
                                    .on_click(
                                        move |_, window, cx| {
                                            let _ = owner.update(cx, |this, cx| {
                                                if !this.open || this.disabled {
                                                    return;
                                                }
                                                this.selected = id.clone();
                                                cx.emit(Selected(id.clone()));
                                                this.open = false;
                                                this.focus.trigger.focus(window, cx);
                                                cx.notify();
                                            });
                                        },
                                    )
                                },
                            ));
                        frame.wrap_positioned(
                            crate::menu::select::fade(
                                crate::menu::select::surface(surface, width, t)
                                    .child(items)
                                    .observe("form-choice-content"),
                                scroll,
                                t,
                            ),
                            origin,
                        )
                    }),
            )
            .into_any_element()
    }
}
