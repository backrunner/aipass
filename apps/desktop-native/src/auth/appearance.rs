use super::*;
impl AuthView {
    pub fn prepare_appearance(
        &mut self,
        dark: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.backdrop.prepare_appearance(dark, window, cx)
    }
    pub fn apply_appearance(&mut self, dark: bool, cx: &mut Context<Self>) {
        self.theme = Theme::new(dark);
        self.import.update(cx, |s, cx| s.appearance(dark, cx));
        self.backdrop.apply_appearance(dark);
        for input in [
            &self.password,
            &self.confirmation,
            &self.recovery,
            &self.reset_confirmation,
        ] {
            let mut style = self.theme.input_style();
            if input == &self.reset_confirmation {
                style.muted_foreground = rgb(0xa9a9a9).into();
            }
            input.update(cx, |input, cx| {
                input.set_editor_style(style);
                cx.notify();
            });
        }
        cx.notify();
    }
}
