//! Shared usage range menu: hover opens without stealing keyboard focus.
use super::*;
use crate::parity::Observe;
use gpui::{div, px, EventEmitter};
use gpui_base::{Button, ElementExt, Popup};
pub(super) struct Range {
    pub value: String,
    pub theme: Theme,
    pub locale: Locale,
    open: bool,
    hover_opened: bool,
    close_task: Option<gpui::Task<()>>,
    focus: crate::menu::MenuFocus,
    trigger_bounds: std::rc::Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
    menu_bounds: std::rc::Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
}

#[cfg(all(test, feature = "parity-render"))]
mod tests {
    use super::*;
    #[gpui::test]
    fn hover_preserves_focus_and_gap_timer_retires_the_menu(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_base::init(cx);
            crate::menu::init(cx);
            cx.set_reduce_motion(true);
        });
        let (view, cx) =
            cx.add_window_view(|_, cx| Range::new(Theme::new(false), &Locale::new(false), cx));
        cx.update(|w, cx| w.draw(cx).clear(cx));
        let before = cx.update(|w, cx| w.focused(cx));
        view.update(cx, |s, cx| s.hover(true, cx));
        cx.update(|w, cx| w.draw(cx).clear(cx));
        cx.update(|w, cx| assert_eq!(before, w.focused(cx)));
        view.update(cx, |s, cx| s.hover(false, cx));
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        assert!(view.read_with(cx, |s, _| s.open));
        view.update(cx, |s, cx| s.hover(true, cx));
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        assert!(view.read_with(cx, |s, _| s.open));
        view.update(cx, |s, cx| s.hover(false, cx));
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(180));
        cx.run_until_parked();
        assert!(!view.read_with(cx, |s, _| s.open));
    }
}
impl EventEmitter<Selected> for Range {}
impl Range {
    pub fn new(t: Theme, l: &Locale, cx: &mut Context<Self>) -> Self {
        Self {
            value: "7".into(),
            theme: t,
            locale: Locale::new(l.is_chinese()),
            open: false,
            hover_opened: false,
            close_task: None,
            focus: crate::menu::MenuFocus::new(3, cx),
            trigger_bounds: Default::default(),
            menu_bounds: Default::default(),
        }
    }
    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.close_task = None;
        cx.notify();
    }
    fn hover(&mut self, inside: bool, cx: &mut Context<Self>) {
        self.close_task = None;
        if inside {
            if !self.open {
                self.open = true;
                self.hover_opened = true;
            }
        } else if self.hover_opened {
            self.close_task = Some(cx.spawn(async move |s, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(180))
                    .await;
                let _ = s.update(cx, |s, cx| s.close(cx));
            }));
        }
        cx.notify();
    }
}
impl Render for Range {
    fn render(&mut self, w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let options = [
            ("24h", "server.last24Hours"),
            ("7", "server.last7Days"),
            ("30", "server.last30Days"),
        ];
        let selected = options
            .iter()
            .position(|(v, _)| *v == self.value)
            .unwrap_or(1);
        let frame = crate::menu::motion::sample("range-motion", self.open, w, cx);
        let origin = crate::menu::motion::Origin::new(false);
        let track = origin.clone();
        let focus = self.focus.clone();
        let key_focus = focus.clone();
        let trigger_bounds = self.trigger_bounds.clone();
        let trigger = Button::new("usage-range-trigger")
            .track_focus(&focus.trigger)
            .h(px(26.))
            .px(px(6.))
            .rounded(px(4.))
            .text_size(px(12.))
            .text_color(t.tertiary)
            .flex()
            .items_center()
            .gap(px(4.))
            .hover(move |s| s.bg(t.surface2).text_color(t.text))
            .when(self.open, |el| el.bg(t.surface2).text_color(t.text))
            .focus_visible(move |s| s.shadow(t.outline(t.focus_ring, 2., 2.)))
            .accessibility_label(self.locale.text("server.usageChart"))
            .child(self.locale.text(options[selected].1))
            .child(
                gpui::svg()
                    .path("chevron-down")
                    .size(px(12.))
                    .text_color(t.tertiary),
            )
            .on_hover(cx.listener(|s, inside: &bool, _, cx| s.hover(*inside, cx)))
            .on_click(cx.listener(move |s, _, w, cx| {
                if s.open && !s.hover_opened {
                    s.close(cx);
                    return;
                }
                s.open = true;
                s.hover_opened = false;
                s.close_task = None;
                let f = focus.clone();
                w.on_next_frame(move |w, cx| f.focus_item(selected, w, cx));
                cx.notify();
            }))
            .capture_key_down(cx.listener(move |s, e: &gpui::KeyDownEvent, w, cx| {
                if matches!(e.keystroke.key.as_str(), "down" | "up") {
                    s.open = true;
                    s.hover_opened = false;
                    s.close_task = None;
                    let f = key_focus.clone();
                    w.on_next_frame(move |w, cx| f.focus_item(selected, w, cx));
                    cx.stop_propagation();
                    cx.notify();
                }
            }));
        let mut popup = Popup::new(
            "usage-range",
            div()
                .on_prepaint(move |b, _, _| trigger_bounds.set(b))
                .child(trigger),
        )
        .anchor(gpui::Anchor::TopRight)
        .offset(px(6.))
        .on_position(move |p, b| track.resolve(p, b));
        if frame.visible {
            let owner = cx.entity().downgrade();
            let menu_bounds = self.menu_bounds.clone();
            let content = self
                .focus
                .surface(
                    "usage-range-surface",
                    self.focus.trigger.clone(),
                    self.focus.trigger.clone(),
                    move |_, cx| {
                        let _ = owner.update(cx, |s, cx| s.close(cx));
                    },
                )
                .on_prepaint(move |b, _, _| menu_bounds.set(b))
                .on_hover(cx.listener(|s, inside: &bool, _, cx| {
                    if *inside {
                        s.close_task = None;
                    } else {
                        s.hover(false, cx);
                    }
                }))
                .child(
                    crate::menu::select::surface(div().id("usage-range-items"), px(140.), t)
                        .children(options.into_iter().enumerate().map(|(i, (value, key))| {
                            crate::menu::select::item(("usage-range-option", i), i == selected, t)
                                .track_focus(&self.focus.items[i])
                                .disabled(!self.open)
                                .role(gpui::Role::MenuItemRadio)
                                .aria_selected(i == selected)
                                .px(px(9.))
                                .py(px(7.))
                                .text_size(px(12.))
                                .min_h(px(0.))
                                .child(div().flex_1().child(self.locale.text(key)))
                                .child(crate::menu::select::indicator(i == selected, t))
                                .on_click(cx.listener(move |s, _, w, cx| {
                                    s.value = value.into();
                                    s.close(cx);
                                    s.focus.trigger.focus(w, cx);
                                    cx.emit(Selected(value.into()));
                                }))
                        })),
                );
            popup = popup.content(frame.wrap_positioned(content, origin));
        }
        let trigger = self.trigger_bounds.clone();
        let menu = self.menu_bounds.clone();
        div()
            .id("usage-range-root")
            .child(popup)
            .on_mouse_down_out(cx.listener(move |s, e: &gpui::MouseDownEvent, _, cx| {
                if s.open
                    && !trigger.get().contains(&e.position)
                    && !menu.get().contains(&e.position)
                {
                    s.close(cx);
                }
            }))
            .observe("proxy-usage-range")
    }
}
