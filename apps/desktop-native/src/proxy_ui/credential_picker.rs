//! Searchable credential picker. Search covers metadata, never plaintext secrets.
use super::*;
use crate::parity::Observe;
use gpui::{div, point, px, Bounds, Focusable, Pixels};
use gpui_base::{
    input::{Input, InputEvent, InputState},
    Button, ElementExt, Popup,
};

#[derive(Clone)]
struct OptionRow {
    key: String,
    entry: aipass_vault::EntrySummary,
    secret: aipass_provider_registry::SecretRef,
    added: bool,
}
pub(super) struct Picker {
    theme: Theme,
    locale: Locale,
    options: Vec<OptionRow>,
    disabled: bool,
    input: Option<Entity<InputState>>,
    subscription: Option<Subscription>,
    query: String,
    open: bool,
    suppress_focus: bool,
    focus: crate::menu::MenuFocus,
    scroll: ScrollHandle,
    bounds: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
    menu_bounds: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
}
impl gpui::EventEmitter<Selected> for Picker {}
impl Picker {
    pub fn new(theme: Theme, locale: &Locale, cx: &mut Context<Self>) -> Self {
        Self {
            theme,
            locale: Locale::new(locale.is_chinese()),
            options: vec![],
            disabled: false,
            input: None,
            subscription: None,
            query: String::new(),
            open: false,
            suppress_focus: false,
            focus: crate::menu::MenuFocus::new(0, cx),
            scroll: ScrollHandle::new(),
            bounds: Default::default(),
            menu_bounds: Default::default(),
        }
    }
    pub fn configure(
        &mut self,
        entries: Vec<aipass_vault::EntrySummary>,
        selected: Vec<(Uuid, String)>,
        theme: Theme,
        locale: &Locale,
        disabled: bool,
        cx: &mut Context<Self>,
    ) {
        self.theme = theme;
        if self.locale.is_chinese() != locale.is_chinese() {
            self.locale = Locale::new(locale.is_chinese());
        }
        self.disabled = disabled;
        self.options = entries
            .into_iter()
            .filter(|e| e.deleted_at.is_none() && e.archived_at.is_none())
            .flat_map(|entry| {
                entry
                    .secret_refs
                    .iter()
                    .filter(|s| super::route::target(&entry, s, 0).is_some())
                    .map(|secret| OptionRow {
                        key: format!("{}/{}", entry.id, secret.id),
                        added: selected.contains(&(entry.id, secret.id.clone())),
                        entry: entry.clone(),
                        secret: secret.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        if self.focus.items.len() != self.options.len() {
            self.focus.items = (0..self.options.len()).map(|_| cx.focus_handle()).collect();
        }
    }
    #[cfg(feature = "parity-render")]
    pub fn preview_open(&mut self) {
        self.open = true;
    }
    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.query.clear();
        cx.notify();
    }
    fn dismiss(&mut self, restore: bool, w: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.query.clear();
        self.scroll.set_offset(point(px(0.), px(0.)));
        if let Some(input) = &self.input {
            self.suppress_focus = restore && !input.focus_handle(cx).is_focused(w);
            input.update(cx, |s, cx| {
                s.set_value("", w, cx);
                if restore {
                    s.focus(w, cx);
                }
            });
        }
        cx.notify();
    }
    fn choose(&mut self, key: &str, w: &mut Window, cx: &mut Context<Self>) {
        if !self.open || self.disabled || !self.options.iter().any(|o| o.key == key && !o.added) {
            return;
        }
        cx.emit(Selected(key.to_owned()));
        self.dismiss(true, w, cx);
    }
}
impl Render for Picker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.input.is_none() {
            let input = crate::form_ui::input(
                "server.addMember",
                "",
                false,
                self.theme,
                &self.locale,
                window,
                cx,
            );
            self.subscription = Some(cx.subscribe(&input, |s, input, event: &InputEvent, cx| {
                if s.disabled {
                    return;
                }
                match event {
                    InputEvent::Change => {
                        s.query = input.read(cx).value().to_string();
                        s.open = true;
                        s.scroll.set_offset(point(px(0.), px(0.)));
                    }
                    InputEvent::Focus => {
                        if s.suppress_focus {
                            s.suppress_focus = false;
                        } else {
                            s.open = true;
                        }
                    }
                    _ => {}
                }
                cx.notify();
            }));
            self.focus.trigger = input.focus_handle(cx);
            self.input = Some(input);
        }
        let input = self.input.as_ref().unwrap().clone();
        input.update(cx, |s, cx| {
            s.set_editor_style(self.theme.input_style());
            s.set_disabled(self.disabled, cx);
            if !self.open && s.value().as_ref() != self.query {
                s.set_value(self.query.clone(), window, cx);
            }
            let placeholder = self.locale.text("server.searchCredentials");
            if s.presentation().placeholder().as_ref() != placeholder {
                s.set_placeholder(placeholder, window, cx);
            }
        });
        let t = self.theme;
        let open = self.open && !self.disabled;
        let frame = crate::menu::motion::sample("credential-picker-motion", open, window, cx);
        let origin = crate::menu::motion::Origin::new(false);
        let track = origin.clone();
        let query = self.query.to_lowercase();
        let rows = self
            .options
            .iter()
            .enumerate()
            .filter(|(_, o)| {
                let metadata = format!(
                    "{} {} {} {:?} {}",
                    o.entry.title,
                    o.secret.label,
                    o.secret.masked,
                    o.secret
                        .interface_type
                        .as_ref()
                        .unwrap_or(&o.entry.interface_type),
                    o.secret.group.as_deref().unwrap_or_default()
                )
                .to_lowercase();
                query.split_whitespace().all(|term| metadata.contains(term))
            })
            .map(|(i, o)| (i, o.clone()))
            .collect::<Vec<_>>();
        let mut focus = self.focus.clone();
        focus.scroll = Some(self.scroll.clone());
        focus.items = rows
            .iter()
            .filter(|(_, o)| !o.added)
            .map(|(i, _)| self.focus.items[*i].clone())
            .collect();
        let key_focus = focus.clone();
        let trigger_bounds = self.bounds.clone();
        let trigger = div()
            .id("credential-search")
            .h(px(36.))
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(10.))
            .pr(px(3.))
            .border_1()
            .border_color(if open { t.accent } else { t.border })
            .rounded(px(8.))
            .bg(t.surface)
            .when(open, |el| el.shadow(t.outline(t.focus_ring, 3., 0.)))
            .child(
                gpui::svg()
                    .path("search")
                    .size(px(15.))
                    .text_color(t.tertiary),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(13.))
                    .child(Input::new(&input)),
            )
            .child(
                Button::new("credential-toggle")
                    .disabled(self.disabled)
                    .size(px(28.))
                    .rounded(px(4.))
                    .accessibility_label(self.locale.text("server.addMember"))
                    .hover(move |s| s.bg(t.surface2))
                    .child(
                        gpui::svg()
                            .path("chevron-down")
                            .size(px(14.))
                            .text_color(t.tertiary),
                    )
                    .on_click(cx.listener(|s, _, w, cx| {
                        if s.open {
                            s.dismiss(true, w, cx);
                        } else {
                            s.open = true;
                            s.input.as_ref().unwrap().update(cx, |v, cx| v.focus(w, cx));
                            cx.notify();
                        }
                    })),
            )
            .capture_key_down(cx.listener(move |s, e: &gpui::KeyDownEvent, w, cx| {
                match e.keystroke.key.as_str() {
                    "down" if !s.disabled => {
                        s.open = true;
                        let focus = key_focus.clone();
                        if open {
                            focus.focus_item(0, w, cx);
                        } else {
                            w.on_next_frame(move |w, cx| focus.focus_item(0, w, cx));
                        }
                    }
                    "escape" if s.open => s.dismiss(true, w, cx),
                    "tab" => {
                        s.dismiss(false, w, cx);
                        return;
                    }
                    _ => return,
                }
                cx.stop_propagation();
                cx.notify();
            }));
        let outer_bounds = self.bounds.clone();
        let menu_bounds = self.menu_bounds.clone();
        let mut popup = Popup::new("credential-picker", trigger)
            .offset(px(5.))
            .on_position(move |position, bounds| track.resolve(position, bounds));
        if frame.visible {
            let owner = cx.entity().downgrade();
            let close = owner.clone();
            let surface = focus.surface(
                "credential-options",
                input.focus_handle(cx),
                input.focus_handle(cx),
                move |w, cx| {
                    let _ = close.update(cx, |s, cx| s.dismiss(true, w, cx));
                },
            );
            let record = self.menu_bounds.clone();
            let content =
                surface
                    .role(gpui::Role::ListBox)
                    .occlude()
                    .on_prepaint(move |bounds, _, _| record.set(bounds))
                    .child(
                        crate::menu::select::surface(
                            div().id("credential-menu-frame"),
                            self.bounds.get().size.width.max(px(200.)),
                            t,
                        )
                        .child(
                            div()
                                .id("credential-options-scroll")
                                .track_scroll(&self.scroll)
                                .max_h(px(256.))
                                .overflow_y_scroll()
                                .children(rows.iter().map(|(i, o)| {
                                    let id = o.key.clone();
                                    let owner = owner.clone();
                                    crate::menu::select::item(("credential-option", *i), false, t)
                                    .disabled(o.added || !open).when(o.added,|el|el.opacity(0.5))
                                    .role(gpui::Role::ListBoxOption)
                                    .aria_selected(o.added)
                                    .accessibility_label(format!(
                                        "{} {} {}",
                                        o.entry.title, o.secret.label, o.secret.masked
                                    ))
                                    .track_focus(&self.focus.items[*i])
                                    .min_h(px(38.))
                                    .gap(px(8.))
                                    .text_size(px(12.))
                                    .child(crate::provider_icon::icon(
                                        o.entry.provider_id.as_deref().unwrap_or(""),
                                        &o.entry.title,
                                        o.entry.credential_kind,
                                        &o.entry.provider_kind,
                                        24.,
                                        t.bg.r < 0.5,
                                    ))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(80.))
                                            .truncate()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .child(o.entry.title.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex_grow(0.65).flex_basis(px(0.)).min_w(px(50.))
                                            .truncate()
                                            .text_color(t.secondary)
                                            .child(o.secret.label.clone()),
                                    )
                                    .child(
                                        div()
                                            .w(px(82.))
                                            .truncate()
                                            .text_size(px(10.))
                                            .text_color(t.tertiary)
                                            .child(o.secret.masked.clone()),
                                    )
                                    .child(
                                        div()
                                            .w(px(120.))
                                            .truncate()
                                            .text_size(px(10.))
                                            .text_color(t.tertiary)
                                            .child(
                                                self.locale.text(
                                                    crate::providers::presentation::protocol_key(
                                                        o.secret
                                                            .interface_type
                                                            .as_ref()
                                                            .unwrap_or(&o.entry.interface_type),
                                                    ),
                                                ),
                                            ),
                                    )
                                    .child(crate::menu::select::indicator(o.added, t))
                                    .on_click(move |_, w, cx| {
                                        let _ = owner.update(cx, |s, cx| s.choose(&id, w, cx));
                                    })
                                }))
                                .when(rows.is_empty(), |el| {
                                    el.child(
                                        crate::admin_ui::hint(
                                            self.locale.text(if self.options.is_empty() {
                                                "common.noOptions"
                                            } else {
                                                "server.noMatchingCredentials"
                                            }),
                                            t,
                                        )
                                        .p(px(16.)),
                                    )
                                }),
                        ),
                    );
            popup = popup
                .content(frame.wrap_positioned(content.observe("proxy-credential-menu"), origin));
        }
        div()
            .id("credential-picker-root")
            .on_prepaint(move |bounds, _, _| trigger_bounds.set(bounds))
            .on_mouse_down_out(cx.listener(move |s, e: &gpui::MouseDownEvent, w, cx| {
                if s.open
                    && !outer_bounds.get().contains(&e.position)
                    && !menu_bounds.get().contains(&e.position)
                {
                    s.dismiss(false, w, cx);
                }
            }))
            .child(popup)
            .observe("proxy-credential-picker")
    }
}

#[cfg(all(test, feature = "parity-render"))]
mod tests {
    use super::*;
    #[gpui::test]
    fn search_keyboard_selection_skips_added_credentials_and_restores_input(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_base::init(cx);
            crate::menu::init(cx);
            cx.set_reduce_motion(true);
        });
        let mut picker = None;
        let (_, cx) = cx.add_window_view(|w, cx| {
            let view = cx.new(|cx| Picker::new(Theme::new(false), &Locale::new(false), cx));
            picker = Some(view.clone());
            gpui_base::Root::new(view, w, cx)
        });
        let picker = picker.unwrap();
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/desktop-parity/fixtures/administration.json"
        ))
        .unwrap();
        let entries: Vec<aipass_vault::EntrySummary> =
            serde_json::from_value(fixture["entries"].clone()).unwrap();
        let chosen = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let values = chosen.clone();
        let _subscription = cx.update(|_, cx| {
            cx.subscribe(&picker, move |_, e: &Selected, _| {
                values.borrow_mut().push(e.0.clone())
            })
        });
        cx.update(|_, cx| {
            picker.update(cx, |s, cx| {
                s.configure(
                    entries.clone(),
                    vec![(entries[0].id, entries[0].secret_refs[0].id.clone())],
                    Theme::new(false),
                    &Locale::new(false),
                    false,
                    cx,
                );
            })
        });
        cx.update(|w, cx| w.draw(cx).clear(cx));
        cx.update(|w, cx| {
            picker.update(cx, |s, cx| {
                s.input.as_ref().unwrap().update(cx, |v, cx| {
                    v.focus(w, cx);
                    v.replace_all("Gateway", w, cx);
                })
            })
        });
        cx.run_until_parked();
        cx.update(|w, cx| w.draw(cx).clear(cx));
        cx.simulate_keystrokes("down");
        cx.run_until_parked();
        cx.update(|w, cx| w.draw(cx).clear(cx));
        cx.update(|w, cx| {
            picker.update(cx, |s, cx| {
                assert!(
                    s.focus.items[1].is_focused(w),
                    "arrow down: open={}, query={}, input={}, focused={:?}, bounds={:?}",
                    s.open,
                    s.query,
                    s.input.as_ref().unwrap().focus_handle(cx).is_focused(w),
                    s.focus
                        .items
                        .iter()
                        .map(|h| h.is_focused(w))
                        .collect::<Vec<_>>(),
                    s.menu_bounds.get()
                )
            })
        });
        let keystroke = gpui::Keystroke::parse("enter").unwrap();
        cx.simulate_event(gpui::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(gpui::KeyUpEvent { keystroke });
        cx.run_until_parked();
        cx.update(|w, cx| {
            picker.update(cx, |s, cx| {
                assert!(
                    !s.open,
                    "chosen={:?}, query={}, suppressed={}",
                    chosen.borrow(),
                    s.query,
                    s.suppress_focus
                );
                assert!(s.query.is_empty());
                assert!(s.input.as_ref().unwrap().focus_handle(cx).is_focused(w));
            })
        });
        assert_eq!(
            &*chosen.borrow(),
            &[format!(
                "{}/{}",
                entries[0].id, entries[0].secret_refs[1].id
            )]
        );
    }
}
