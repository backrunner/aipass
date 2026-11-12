use gpui::{div, prelude::*, App, Div, FocusHandle, KeyBinding, Stateful, Window};
use std::rc::Rc;

gpui::actions!(menu, [Next, Previous, First, Last, Leave, LeaveBack]);
struct KeysRegistered;
impl gpui::Global for KeysRegistered {}

pub(crate) fn init(cx: &mut App) {
    if cx.has_global::<KeysRegistered>() {
        return;
    }
    cx.set_global(KeysRegistered);
    cx.bind_keys([
        KeyBinding::new("down", Next, Some("AIPassMenu")),
        KeyBinding::new("up", Previous, Some("AIPassMenu")),
        KeyBinding::new("home", First, Some("AIPassMenu")),
        KeyBinding::new("end", Last, Some("AIPassMenu")),
        KeyBinding::new("tab", Leave, Some("AIPassMenu")),
        KeyBinding::new("shift-tab", LeaveBack, Some("AIPassMenu")),
    ]);
}

#[derive(Clone)]
pub(crate) struct MenuFocus {
    pub trigger: FocusHandle,
    pub container: FocusHandle,
    pub items: Vec<FocusHandle>,
}

impl MenuFocus {
    pub fn new(count: usize, cx: &mut App) -> Self {
        Self {
            trigger: cx.focus_handle(),
            container: cx.focus_handle(),
            items: (0..count).map(|_| cx.focus_handle()).collect(),
        }
    }

    pub fn surface(
        &self,
        id: &'static str,
        next_control: FocusHandle,
        previous_control: FocusHandle,
        close: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Stateful<Div> {
        let next = self.clone();
        let previous = self.clone();
        let first = self.items[0].clone();
        let last = self.items[self.items.len() - 1].clone();
        let close = Rc::new(close);
        let close_forward = close.clone();
        div()
            .id(id)
            .role(gpui::Role::Menu)
            .track_focus(&self.container)
            .key_context("AIPassMenu")
            .on_action(move |_: &Next, window, cx| next.step(true, window, cx))
            .on_action(move |_: &Previous, window, cx| previous.step(false, window, cx))
            .on_action(move |_: &First, window, cx| first.focus(window, cx))
            .on_action(move |_: &Last, window, cx| last.focus(window, cx))
            .on_action(move |_: &Leave, window, cx| {
                close_forward(window, cx);
                next_control.focus(window, cx);
            })
            .on_action(move |_: &LeaveBack, window, cx| {
                close(window, cx);
                previous_control.focus(window, cx);
            })
    }

    fn step(&self, forward: bool, window: &mut Window, cx: &mut App) {
        let current = self.items.iter().position(|focus| focus.is_focused(window));
        let index = match (current, forward) {
            (Some(index), true) => (index + 1).min(self.items.len() - 1),
            (Some(index), false) => index.saturating_sub(1),
            (None, true) => 0,
            (None, false) => self.items.len() - 1,
        };
        self.items[index].focus(window, cx);
    }
}
