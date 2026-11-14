use super::*;

impl Workspace {
    pub fn prepare_appearance(
        &mut self,
        dark: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.backdrop.prepare_appearance(dark, window, cx)
    }
    pub fn apply_appearance(&mut self, dark: bool, cx: &mut Context<Self>) {
        self.dark = dark;
        self.theme = Theme::new(dark);
        self.backdrop.apply_appearance(dark);
        self.search.update(cx, |input, cx| {
            input.set_editor_style(self.theme.input_style());
            cx.notify();
        });
        self.agents.update(cx, |view, cx| view.appearance(dark, cx));
        self.notes
            .update(cx, |view, cx| view.apply_appearance(dark, cx));
        self.tools
            .update(cx, |view, cx| view.apply_appearance(dark, cx));
        self.pricing
            .update(cx, |view, cx| view.apply_appearance(dark, cx));
        self.credentials
            .update(cx, |view, cx| view.apply_appearance(dark, cx));
        self.integrations
            .update(cx, |view, cx| view.apply_appearance(dark, cx));
        self.subscriptions
            .update(cx, |view, cx| view.apply_appearance(dark, cx));
        self.trash
            .update(cx, |view, cx| view.apply_appearance(dark, cx));
        if let Some(form) = &self.provider_form {
            form.update(cx, |view, cx| view.apply_appearance(dark, cx));
        }
        cx.notify();
    }
}
