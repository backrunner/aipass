use super::*;
use crate::appearance;

impl Desktop {
    #[cfg(feature = "parity-render")]
    pub fn begin_appearance_sample(&mut self) {
        self.appearance.preference = appearance::ThemePreference::System;
        self.appearance.next_frozen = Some(Duration::ZERO);
    }
    #[cfg(feature = "parity-render")]
    pub fn freeze_appearance(&mut self, milliseconds: u64, cx: &mut Context<Self>) {
        // The native application override drives the real appearance observer;
        // this only freezes the synthetic capture's interpolation clock.
        self.appearance.frozen = Some(Duration::from_millis(milliseconds));
        cx.notify();
    }
    #[cfg(feature = "parity-render")]
    pub fn appearance_state(&self) -> serde_json::Value {
        serde_json::json!({
            "preference": self.appearance.preference,
            "dark": self.appearance.dark,
            "requested": self.appearance.requested,
            "progress": self.appearance.progress(),
        })
    }
    pub fn follow_preferences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.update_appearance_preference(cx);
        self._preferences_subscription = Some(
            cx.observe_global_in::<crate::preferences::Preferences>(window, |this, _, cx| {
                this.update_appearance_preference(cx);
            }),
        );
    }
    fn update_appearance_preference(&mut self, cx: &mut Context<Self>) {
        let preference = cx.global::<crate::preferences::Preferences>().0.theme;
        self.appearance.preference = preference;
        cx.set_window_appearance(appearance::forced(preference));
        self.appearance.requested = appearance::resolve(preference, cx.window_appearance());
        cx.notify();
    }
    pub(super) fn prepare_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dark = self.appearance.requested;
        let auth = self
            .auth
            .update(cx, |view, cx| view.prepare_appearance(dark, window, cx));
        let workspace = self
            .workspace
            .update(cx, |view, cx| view.prepare_appearance(dark, window, cx));
        if dark == self.appearance.dark || !auth || !workspace {
            return;
        }
        self.auth
            .update(cx, |view, cx| view.apply_appearance(dark, cx));
        self.workspace
            .update(cx, |view, cx| view.apply_appearance(dark, cx));
        self.toast
            .update(cx, |view, cx| view.apply_appearance(dark, cx));
        self.recovery
            .update(cx, |view, cx| view.appearance(dark, cx));
        self.theme = Theme::new(dark);
        if let Some(curtain) = &self.curtain {
            curtain.update(cx, |view, cx| view.apply_appearance(dark, cx));
        }
        self.appearance.begin(cx.reduce_motion());
    }
}
