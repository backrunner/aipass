//! Native file dialogs never retain a private key after the drawer is retired.
use super::*;
use gpui::{div, px};
impl Settings {
    pub(super) fn certificate_picker(&self, key: bool, cx: &mut Context<Self>) -> gpui::AnyElement {
        let label = if key {
            "panel.privateKey"
        } else {
            "panel.certificate"
        };
        let name = if key {
            self.private_key_file.as_ref().map(|(name, _)| name)
        } else {
            self.certificate_file.as_ref().map(|(name, _)| name)
        };
        admin_ui::stack()
            .gap(px(6.))
            .child(crate::form_ui::label(self.locale.text(label), self.theme))
            .child(
                div()
                    .flex()
                    .items_center()
                    .min_w_0()
                    .h(px(34.))
                    .border_1()
                    .border_color(self.theme.border)
                    .rounded(px(8.))
                    .overflow_hidden()
                    .child(
                        gpui_base::Button::new(if key {
                            "panel-key-file"
                        } else {
                            "panel-cert-file"
                        })
                        .accessibility_label(self.locale.text(label))
                        .h_full()
                        .px(px(8.))
                        .flex_shrink_0()
                        .border_r_1()
                        .border_color(self.theme.divider)
                        .bg(self.theme.surface2)
                        .text_color(self.theme.secondary)
                        .text_size(px(11.))
                        .child(self.locale.text("auth.import.browse"))
                        .disabled(self.pending || self.picking)
                        .on_click(cx.listener(move |s, _, w, cx| s.pick_certificate(key, w, cx))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px(px(8.))
                            .truncate()
                            .text_size(px(11.))
                            .text_color(self.theme.tertiary)
                            .child(name.cloned().unwrap_or_else(|| "—".into())),
                    ),
            )
            .into_any_element()
    }
    fn pick_certificate(&mut self, key: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.picking {
            return;
        }
        self.error.clear();
        self.picking = true;
        let generation = self.generation;
        let picker = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(
                self.locale
                    .text(if key {
                        "panel.privateKey"
                    } else {
                        "panel.certificate"
                    })
                    .into(),
            ),
        });
        cx.spawn_in(window, async move |this, cx| {
            let selected = picker.await;
            // The native picker can outlive closing, locking, or switching tabs.
            let current = this
                .update(cx, |s, _| {
                    s.open && s.generation == generation && s.tab == 3
                })
                .unwrap_or(false);
            if !current {
                return;
            }
            let result = match selected {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        Some(
                            cx.background_executor()
                                .spawn(async move { read_pem(&path, key) })
                                .await,
                        )
                    } else {
                        None
                    }
                }
                Ok(Ok(None)) => None,
                _ => Some(Err("settings.saveFailed")),
            };
            let _ = this.update(cx, |s, cx| {
                if !s.open || s.generation != generation || s.tab != 3 {
                    return;
                }
                s.picking = false;
                match result {
                    Some(Ok((name, pem))) => {
                        if key {
                            s.private_key_file = Some((name, pem));
                        } else {
                            s.certificate_file = Some((name, pem.expose().to_owned()));
                        }
                        if s.certificate_file.is_some() && s.private_key_file.is_some() {
                            s.save_panel(None, false, cx);
                        }
                    }
                    Some(Err(label)) => s.error = s.locale.text(label),
                    None => {}
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub(super) fn export_certificate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pem) = self.panel.as_ref().and_then(|p| p.certificate_pem.clone()) else {
            return;
        };
        if self.pending || self.picking {
            return;
        }
        self.error.clear();
        self.picking = true;
        let generation = self.generation;
        let picker =
            cx.prompt_for_new_path(&std::env::temp_dir(), Some("aipass-control-panel.pem"));
        cx.spawn_in(window, async move |this, cx| {
            let selected = picker.await;
            if !this
                .update(cx, |s, _| {
                    s.open && s.generation == generation && s.tab == 3
                })
                .unwrap_or(false)
            {
                return;
            }
            let result = match selected {
                Ok(Ok(Some(path))) => {
                    cx.background_executor()
                        .spawn(async move { std::fs::write(path, pem).map_err(|_| ()) })
                        .await
                }
                Ok(Ok(None)) => Ok(()),
                _ => Err(()),
            };
            let _ = this.update(cx, |s, cx| {
                if s.generation != generation {
                    return;
                }
                s.picking = false;
                if result.is_err() {
                    s.error = s.locale.text("settings.saveFailed");
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
fn read_pem(path: &std::path::Path, key: bool) -> Result<(String, SensitiveString), &'static str> {
    use std::io::Read;
    let limit = if key { 16384 } else { 65536 };
    let file = std::fs::File::open(path).map_err(|_| "settings.saveFailed")?;
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "settings.saveFailed")?;
    if bytes.len() as u64 > limit {
        return Err("panel.certificateSize");
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "settings.saveFailed")?;
    Ok((
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        SensitiveString::new(text),
    ))
}
#[cfg(test)]
mod tests;
