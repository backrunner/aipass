use super::*;
use crate::form_ui::select::{Choice, Selected};
impl Settings {
    pub(super) fn choice(
        &mut self,
        key: &str,
        label: &str,
        selected: String,
        options: Vec<(String, String)>,
        change: impl Fn(&mut Self, &str, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> gpui::Entity<Choice> {
        if !self.choices.contains_key(key) {
            let choice = cx.new(|cx| {
                Choice::new(
                    self.locale.text(label),
                    options.clone(),
                    &selected,
                    self.theme,
                    cx,
                )
            });
            self.choice_subscriptions
                .push(cx.subscribe(&choice, move |s, _, v: &Selected, cx| change(s, &v.0, cx)));
            self.choices.insert(key.into(), choice);
        }
        let view = self.choices[key].clone();
        view.update(cx, |v, cx| {
            v.inline = true;
            v.height = Some(34.);
            v.theme = self.theme;
            v.label = self.locale.text(label);
            v.selected = selected;
            v.disabled = self.pending;
            v.set_options(options, cx);
        });
        view
    }
    pub(super) fn save_blurred_preferences(&mut self, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        let blurs = self.fields.take_blurs();
        if self.tab == 3 {
            if blurs.iter().any(|k| k == "panel-port") {
                self.save_panel(None, false, cx);
            } else if blurs
                .iter()
                .any(|k| k.starts_with("retry-") || k.starts_with("price-") || k == "upstream-url")
            {
                self.save_server(cx);
            }
            return;
        }
        if !blurs.iter().any(|k| k == "clipboard-seconds") {
            return;
        }
        match self.fields.value("clipboard-seconds", cx).parse::<u16>() {
            Ok(seconds) if (1..=600).contains(&seconds) => {
                if self.preferences.clipboard_clear_seconds != seconds {
                    self.preferences.clipboard_clear_seconds = seconds;
                    self.save_preferences(cx);
                }
            }
            _ => {
                self.error = self.locale.text("settings.saveFailed");
                cx.notify();
            }
        }
    }
}
