//! Populated sync cards. Metadata stays in distinct layout fields in both locales.
use super::*;
use crate::parity::Observe;
use gpui::{div, px, FontWeight};

impl Settings {
    pub(super) fn conflict_rows(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let t = self.theme;
        let mut rows = presentation::rows();
        if self.conflicts.is_empty() {
            return rows.child(admin_ui::hint(self.locale.text("settings.noConflicts"), t));
        }
        let mut items = admin_ui::stack().pixel_snap(false).gap(px(8.));
        for (i, c) in self.conflicts.iter().enumerate() {
            let title = match c.object.object_type.as_str() {
                "invalid_snapshot" => self.locale.text("settings.invalidSnapshot"),
                "vault_snapshot" => self.locale.text("settings.vaultSnapshot"),
                _ => c
                    .conflict_summary
                    .as_ref()
                    .or(c.target_summary.as_ref())
                    .map(|s| s.title.clone())
                    .unwrap_or(c.object.object_type.clone()),
            };
            let scope = serde_json::to_value(&c.scope).expect("scope");
            let origin = self
                .locale
                .text("settings.incomingOrigin")
                .replace("{origin}", &c.origin);
            let line = window.text_system().shape_line(
                origin.clone().into(),
                px(11.),
                &[gpui::TextRun {
                    len: origin.len(),
                    font: gpui::font(".AppleSystemUIFont"),
                    color: t.tertiary.into(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }],
                None,
            );
            // A mixed-script CSS line box includes every fallback face, not
            // only the explicitly requested system font's metrics.
            let text = window.text_system();
            let ascent = line
                .runs
                .iter()
                .map(|r| text.ascent(r.font_id, px(11.)))
                .fold(line.ascent, |a, b| a.max(b));
            let descent = line
                .runs
                .iter()
                .map(|r| text.descent(r.font_id, px(11.)).abs())
                .fold(line.descent.abs(), |a, b| a.max(b));
            let metadata_height = px(13.).max(ascent + descent);
            let mut item = admin_ui::stack()
                .gap(px(8.))
                .p(px(12.))
                .border_1()
                .border_color(t.divider)
                .rounded(px(8.))
                .bg(t.surface)
                .child(
                    admin_ui::stack()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::BOLD)
                                .child(title),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(px(8.))
                                .text_size(px(11.))
                                .line_height(metadata_height)
                                .text_color(t.tertiary)
                                .child(scope.as_str().unwrap_or_default().to_owned())
                                .child(origin),
                        ),
                );
            if c.object.object_type == "vault_snapshot" {
                item = item.child(admin_ui::hint(
                    self.locale.text("settings.snapshotConflictHint"),
                    t,
                ));
            }
            if let Some(summary) = &c.snapshot_summary {
                item = item.child(
                    admin_ui::stack()
                        .gap(px(0.))
                        .text_size(px(12.))
                        .line_height(px(17.4))
                        .text_color(t.tertiary)
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(px(8.))
                                .child(aipass_desktop_native::pricing::presentation::local_date(
                                    c.object.updated_at.unix_timestamp(),
                                    self.locale.language(),
                                ))
                                .child(
                                    self.locale
                                        .text("settings.snapshotCount")
                                        .replace("{count}", &summary.provider_count.to_string()),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(px(8.))
                                .children(summary.titles.iter().cloned()),
                        ),
                );
            } else {
                let mut versions = admin_ui::stack()
                    .line_height(px(15.))
                    .gap(px(6.))
                    .px(px(10.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .bg(t.surface2);
                for (label, summary, prefix) in [
                    ("settings.current", &c.target_summary, "target "),
                    ("settings.incoming", &c.conflict_summary, ""),
                ] {
                    let mut value = div()
                        .flex()
                        .gap(px(8.))
                        .flex_1()
                        .min_w_0()
                        .text_size(px(12.))
                        .font_family(".AppleSystemUIFontMonospaced");
                    if let Some(summary) = summary {
                        value =
                            value
                                .child(div().truncate().child(summary.masked_secret.clone()))
                                .child(div().truncate().child(
                                    summary.fingerprint.chars().take(12).collect::<String>(),
                                ));
                    } else {
                        value = value.child(div().truncate().child(format!(
                            "{prefix}{}",
                            c.object.hash_hex.chars().take(12).collect::<String>()
                        )));
                    }
                    versions = versions.child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .items_center()
                            .child(
                                div()
                                    .w(px(80.))
                                    .flex_shrink_0()
                                    .text_size(px(11.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(t.tertiary)
                                    .child(self.locale.text(label).to_uppercase()),
                            )
                            .child(value),
                    );
                }
                item = item.child(versions);
            }
            let request = SyncConflictActionRequest {
                scope: c.scope.clone(),
                dir: self.sync_settings.sync_folder.clone(),
                provider: match self.sync_settings.mode {
                    SyncMode::ICloud => Some(CloudSyncProvider::ICloud),
                    SyncMode::OneDrive => Some(CloudSyncProvider::OneDrive),
                    _ => None,
                },
                conflict_path: c.conflict_path.clone(),
            };
            let accept = request.clone();
            item = item.child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        self.sync_action(
                            &format!("sync-accept-{i}"),
                            "settings.acceptIncoming",
                            Some("check"),
                            false,
                        )
                        .disabled(self.pending || c.object.object_type == "invalid_snapshot")
                        .on_click(cx.listener(move |s, _, _, cx| {
                            s.apply(
                                AgentRequest::SyncAcceptConflict {
                                    request: accept.clone(),
                                },
                                cx,
                            )
                        })),
                    )
                    .child(
                        self.sync_action(
                            &format!("sync-discard-{i}"),
                            "settings.keepCurrent",
                            Some("trash-2"),
                            true,
                        )
                        .on_click(cx.listener(move |s, _, _, cx| {
                            s.apply(
                                AgentRequest::SyncDiscardConflict {
                                    request: request.clone(),
                                },
                                cx,
                            )
                        })),
                    ),
            );
            items = items.child(item.observe(format!("settings-conflict-{i}")));
        }
        rows = rows.child(items);
        rows
    }
    pub(super) fn device_rows(&self, cx: &mut Context<Self>) -> gpui::Div {
        let t = self.theme;
        let mut rows = presentation::rows();
        if self.devices.is_empty() {
            return rows.child(admin_ui::hint(
                self.locale.text("settings.noTrustedDevices"),
                t,
            ));
        }
        let mut items = admin_ui::stack().pixel_snap(false).gap(px(8.));
        for (i, d) in self.devices.iter().enumerate() {
            let id = d.id;
            let row = div()
                .flex()
                .items_center()
                .gap(px(12.))
                .px(px(12.))
                .py(px(10.))
                .border_1()
                .border_color(t.divider)
                .rounded(px(8.))
                .bg(t.surface)
                .child(
                    admin_ui::stack()
                        .gap(px(2.))
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::BOLD)
                                .truncate()
                                .child(d.name.clone()),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(px(8.))
                                .text_size(px(12.))
                                .line_height(px(15.))
                                .text_color(t.tertiary)
                                .child(self.locale.text(if d.trusted {
                                    "settings.trusted"
                                } else {
                                    "settings.revoked"
                                }))
                                .child(
                                    self.locale
                                        .text("settings.epoch")
                                        .replace("{epoch}", &d.last_epoch.to_string()),
                                ),
                        ),
                )
                .when(d.trusted, |el| {
                    el.child(
                        self.sync_action(
                            &format!("device-revoke-{i}"),
                            "settings.revoke",
                            None,
                            true,
                        )
                        .on_click(cx.listener(move |s, _, _, cx| {
                            s.apply(AgentRequest::DeviceRevoke { id }, cx)
                        })),
                    )
                })
                .when(!d.trusted, |el| {
                    el.child(
                        div()
                            .rounded_full()
                            .border_1()
                            .border_color(t.danger.opacity(0.3))
                            .px(px(10.))
                            .py(px(3.))
                            .text_size(px(11.))
                            .font_weight(FontWeight::MEDIUM)
                            .bg(t.danger_soft)
                            .text_color(t.danger)
                            .child(self.locale.text("settings.revoked")),
                    )
                });
            items = items.child(row.observe(format!("settings-device-{i}")));
        }
        rows = rows.child(items);
        rows
    }
    fn sync_action(
        &self,
        id: &str,
        label: &str,
        icon: Option<&'static str>,
        ghost: bool,
    ) -> gpui_base::Button {
        self.button(id, label, false)
            .h(px(26.))
            .px(px(9.))
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .gap(px(6.))
            .when(ghost, |s| s.border_color(gpui::rgba(0)).bg(gpui::rgba(0)))
            .when_some(icon, |s, icon| {
                s.child(
                    gpui::svg()
                        .path(icon)
                        .size(px(13.))
                        .text_color(self.theme.text),
                )
                .flex_row_reverse()
            })
    }
}
