//! Control panel settings, sharing the Agent's listener and credential lifecycle.
use super::*;
use crate::parity::Observe;
use gpui::{div, px};
impl Settings {
    pub(super) fn panel_form(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let t = self.theme;
        let panel = self.panel.clone().expect("panel loaded");
        let settings = panel.settings.clone();
        let mut addresses = vec![settings.address.clone()];
        for address in &panel.addresses {
            if !addresses.contains(address) {
                addresses.push(address.clone());
            }
        }
        let address = self.choice(
            "panel-address",
            "panel.address",
            settings.address.clone(),
            addresses.into_iter().map(|v| (v.clone(), v)).collect(),
            |s, v, cx| {
                if let Some(p) = &mut s.panel {
                    p.settings.address = v.into();
                }
                s.save_panel(None, false, cx);
            },
            cx,
        );
        let section = || {
            admin_ui::stack()
                .gap(px(12.))
                .pb(px(16.))
                .border_b_1()
                .border_color(t.divider)
        };
        let enable = section()
            .child(self.panel_toggle(
                "panel.enable",
                settings.enabled,
                if panel.has_access_code {
                    "panel.enableHint"
                } else {
                    "panel.enableNeedsCode"
                },
                |s, v, cx| {
                    if v {
                        s.save_panel(Some(true), false, cx);
                    } else {
                        s.apply(AgentRequest::ControlPanelStop, cx);
                    }
                },
                window,
                cx,
            ))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .child(
                        admin_ui::stack()
                            .gap(px(6.))
                            .flex_1()
                            .min_w_0()
                            .child(crate::form_ui::label(self.locale.text("panel.address"), t))
                            .child(address),
                    )
                    .child(div().w(px(112.)).flex_shrink_0().child(self.field(
                        "panel-port",
                        "panel.port",
                        &settings.port.to_string(),
                        false,
                        window,
                        cx,
                    ))),
            );
        let mut access = section()
            .child(
                admin_ui::row()
                    .gap(px(12.))
                    .child(
                        admin_ui::stack()
                            .gap(px(4.))
                            .flex_1()
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child(self.locale.text("panel.accessCode")),
                            )
                            .child(
                                admin_ui::hint(self.locale.text("panel.accessHint"), t)
                                    .line_height(px(16.8)),
                            ),
                    )
                    .child(
                        self.button(
                            "panel-code",
                            if panel.has_access_code {
                                "panel.rotate"
                            } else {
                                "panel.generate"
                            },
                            false,
                        )
                        .h(px(26.))
                        .px(px(9.))
                        .text_size(px(12.))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .on_click(cx.listener(|s, _, _, cx| {
                            s.request(
                                Operation::AccessCode {
                                    allow_remote_unlock: s.allow_remote_unlock,
                                },
                                false,
                                cx,
                            )
                        })),
                    ),
            )
            .child(self.panel_toggle(
                "panel.allowRemoteUnlock",
                self.allow_remote_unlock,
                if self.allow_remote_unlock {
                    "panel.remoteUnlockHint"
                } else {
                    ""
                },
                |s, v, cx| {
                    s.allow_remote_unlock = v;
                    cx.notify();
                },
                window,
                cx,
            ))
            .when(panel.remote_unlock_enabled, |el| {
                el.child(
                    admin_ui::hint(self.locale.text("panel.remoteGranted"), t)
                        .line_height(px(16.8)),
                )
                .child(
                    crate::providers::controls::ghost_button("panel-revoke", true, t)
                        .h(px(26.))
                        .px(px(9.))
                        .text_size(px(12.))
                        .self_start()
                        .disabled(self.pending)
                        .accessibility_label(self.locale.text("panel.revokeRemote"))
                        .child(self.locale.text("panel.revokeRemote"))
                        .on_click(cx.listener(|s, _, _, cx| {
                            s.hide_access_code();
                            s.allow_remote_unlock = false;
                            s.apply(AgentRequest::ControlPanelDisableRemoteUnlock, cx);
                        }))
                        .observe("settings-panel-revoke"),
                )
            });
        if let Some((code, _)) = &self.access_code {
            access = access
                .child(
                    admin_ui::row()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .px(px(10.))
                                .py(px(8.))
                                .border_1()
                                .border_color(t.border)
                                .rounded(px(8.))
                                .bg(t.surface2)
                                .font_family(".AppleSystemUIFontMonospaced")
                                .text_size(px(12.))
                                .line_height(px(15.))
                                .child(gpui_base::SelectableText::new(
                                    "panel-visible-code",
                                    code.expose().to_owned(),
                                ))
                                .observe("settings-panel-access-code-value"),
                        )
                        .child(
                            self.panel_action("panel-copy-code", "panel.copy")
                                .disabled(self.pending || self.copying)
                                .on_click(cx.listener(|s, _, w, cx| s.copy_access_code(w, cx))),
                        )
                        .child(
                            self.panel_action("panel-hide-code", "panel.hide")
                                .border_color(gpui::rgba(0))
                                .bg(gpui::rgba(0))
                                .on_click(cx.listener(|s, _, _, cx| {
                                    s.hide_access_code();
                                    cx.notify();
                                })),
                        )
                        .observe("settings-panel-access-code"),
                )
                .child(admin_ui::hint(self.locale.text("panel.codeOnce"), t).line_height(px(16.8)));
        }
        let mut transport = section().child(self.panel_toggle(
            "panel.https",
            settings.https,
            if settings.https {
                "panel.certHint"
            } else {
                "panel.httpHint"
            },
            |s, v, cx| {
                if let Some(p) = &mut s.panel {
                    p.settings.https = v;
                }
                s.save_panel(None, false, cx);
            },
            window,
            cx,
        ));
        if settings.https {
            if let Some(fingerprint) = &panel.fingerprint {
                transport = transport.child(
                    div()
                        .px(px(12.))
                        .py(px(10.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(t.divider)
                        .bg(t.surface2)
                        .text_size(px(11.))
                        .line_height(px(16.5))
                        .font_family(".AppleSystemUIFontMonospaced")
                        .text_color(t.secondary)
                        .child(format!("SHA-256: {fingerprint}")),
                );
            }
            transport = transport.child(
                admin_ui::row()
                    .justify_start()
                    .min_h(px(0.))
                    .gap(px(8.))
                    .child(
                        self.panel_action("panel-export-certificate", "panel.export")
                            .disabled(
                                self.pending || self.picking || panel.certificate_pem.is_none(),
                            )
                            .on_click(cx.listener(|s, _, w, cx| s.export_certificate(w, cx))),
                    )
                    .child(
                        self.panel_action("panel-certificate-regenerate", "panel.regenerate")
                            .on_click(cx.listener(|s, _, _, cx| {
                                s.certificate_file = None;
                                s.private_key_file = None;
                                s.save_panel(None, true, cx);
                            })),
                    ),
            );
            let frame = self
                .certificate_motion
                .sample(self.certificate_open, window, cx);
            let files = div()
                .flex()
                .gap(px(12.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(self.certificate_picker(false, cx)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(self.certificate_picker(true, cx)),
                );
            transport = transport.child(
                div()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(t.border)
                    .bg(t.surface)
                    .overflow_hidden()
                    .child(
                        div()
                            .border_b_1()
                            .border_color(if self.certificate_open {
                                t.divider
                            } else {
                                gpui::rgba(0)
                            })
                            .child(
                                gpui_base::Button::new("panel-certificate-toggle")
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .px(px(12.))
                                    .py(px(10.))
                                    .accessibility_label(self.locale.text("panel.import"))
                                    .aria_expanded(self.certificate_open)
                                    .disabled(self.pending)
                                    .hover(move |s| s.bg(t.surface2))
                                    .focus_visible(move |s| {
                                        s.shadow(t.outline(t.focus_ring, 2., 0.))
                                    })
                                    .child(
                                        gpui::svg()
                                            .path("file-key-2")
                                            .size(px(15.))
                                            .text_color(t.tertiary),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(px(12.))
                                            .line_height(px(16.8))
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .child(self.locale.text("panel.import")),
                                    )
                                    .child(frame.chevron(t.tertiary, 15.))
                                    .on_click(cx.listener(|s, _, _, cx| {
                                        s.certificate_open = !s.certificate_open;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(frame.body(
                        "panel-certificate-body",
                        div().p(px(12.)).child(files).when(
                            self.certificate_file.is_some() != self.private_key_file.is_some(),
                            |el| {
                                el.child(
                                    admin_ui::hint(self.locale.text("panel.certificatePair"), t)
                                        .mt(px(8.)),
                                )
                            },
                        ),
                    )),
            );
        }
        let mut body = admin_ui::rows()
            .on_children_prepainted(|children, _, cx| {
                for (index, bounds) in children.iter().take(3).enumerate() {
                    crate::parity::record_bounds(
                        &format!("settings-panel-section-{index}"),
                        *bounds,
                        cx,
                    );
                }
            })
            .py(px(12.))
            .gap(px(16.))
            .child(enable)
            .child(access)
            .child(transport);
        if let Some(url) = panel.url {
            let open = url.clone();
            let copy = url.clone();
            body = body
                .child(
                    admin_ui::row()
                        .justify_start()
                        .gap(px(8.))
                        .child(
                            self.button("panel-open", "panel.open", false)
                                .on_click(move |_, _, cx| cx.open_url(&open)),
                        )
                        .child(
                            self.button("panel-copy-address", "panel.copyAddress", false)
                                .on_click(cx.listener(move |s, _, _, cx| {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        copy.clone(),
                                    ));
                                    s.notice = s.locale.text("panel.copied");
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(15.))
                        .text_color(t.accent)
                        .child(url),
                );
        }
        if let Some(error) = panel.error {
            body = body.child(div().text_color(t.danger).child(error));
        }
        admin_ui::card_actions(
            self.locale.text("panel.title"),
            t,
            body,
            div()
                .h(px(19.))
                .px(px(8.))
                .flex()
                .items_center()
                .rounded_full()
                .border_1()
                .border_color(if panel.running {
                    gpui::rgba(0)
                } else {
                    t.border
                })
                .bg(if panel.running {
                    t.success.opacity(0.10)
                } else {
                    gpui::rgba(0)
                })
                .text_color(if panel.running {
                    t.success
                } else {
                    t.secondary
                })
                .text_size(px(11.))
                .font_weight(gpui::FontWeight::MEDIUM)
                .child(self.locale.text(if panel.running {
                    "panel.running"
                } else {
                    "panel.stopped"
                })),
        )
        .into_any_element()
    }
    fn panel_action(&self, id: &str, label: &str) -> gpui_base::Button {
        self.button(id, label, false)
            .h(px(26.))
            .px(px(9.))
            .text_size(px(12.))
            .font_weight(gpui::FontWeight::MEDIUM)
    }
    fn panel_toggle(
        &self,
        key: &str,
        on: bool,
        description: &str,
        change: impl Fn(&mut Self, bool, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let owner = cx.entity().downgrade();
        let t = self.theme;
        admin_ui::row()
            .gap(px(12.))
            .min_h(px(36.))
            .child(
                admin_ui::stack()
                    .gap(px(2.))
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .line_height(px(16.))
                            .child(self.locale.text(key)),
                    )
                    .when(!description.is_empty(), |s| {
                        s.child(presentation::description(self.locale.text(description), t))
                    }),
            )
            .child(
                admin_ui::switch(
                    key.to_owned(),
                    self.locale.text(key),
                    on,
                    self.pending
                        || (key == "panel.enable"
                            && !on
                            && self.panel.as_ref().is_none_or(|p| !p.has_access_code)),
                    t,
                    window,
                    cx,
                )
                .on_change(move |value, _, _, cx| {
                    let _ = owner.update(cx, |s, cx| change(s, value, cx));
                }),
            )
    }
    pub(super) fn save_panel(
        &mut self,
        enabled: Option<bool>,
        regenerate_certificate: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(panel) = &self.panel else {
            return;
        };
        let mut settings = panel.settings.clone();
        let Ok(port) = self.fields.value("panel-port", cx).parse::<u16>() else {
            self.error = self.locale.text("settings.saveFailed");
            cx.notify();
            return;
        };
        if port == 0 {
            self.error = self.locale.text("settings.saveFailed");
            cx.notify();
            return;
        }
        settings.port = port;
        if let Some(enabled) = enabled {
            settings.enabled = enabled;
        }
        if settings.https && self.certificate_file.is_some() != self.private_key_file.is_some() {
            self.error = self.locale.text("panel.certificatePair");
            cx.notify();
            return;
        }
        let certificate = if settings.https {
            self.certificate_file
                .as_ref()
                .zip(self.private_key_file.as_ref())
                .map(|((_, cert), (_, key))| ControlPanelCertificate {
                    certificate_pem: cert.clone(),
                    private_key_pem: SensitiveString::new(key.expose()),
                })
        } else {
            None
        };
        self.apply(
            AgentRequest::ControlPanelConfigure {
                settings,
                certificate,
                regenerate_certificate,
            },
            cx,
        );
    }
    pub(super) fn retire_panel(&mut self) {
        self.hide_access_code();
        self.certificate_file = None;
        self.private_key_file = None;
        self.certificate_open = false;
        self.picking = false;
    }
    pub(super) fn hide_access_code(&mut self) {
        self.access_code = None;
        if let Some(cancel) = self.copy_cancel.take() {
            cancel.cancel();
        }
        self.copying = false;
    }
    pub(super) fn show_access_code(&mut self, code: SensitiveString, cx: &mut Context<Self>) {
        self.hide_access_code();
        let at = std::time::Instant::now();
        let generation = self.generation;
        self.access_code = Some((code, at));
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(60))
                .await;
            let _ = this.update(cx, |s, cx| {
                if s.generation == generation
                    && s.access_code
                        .as_ref()
                        .is_some_and(|(_, created)| *created == at)
                {
                    s.hide_access_code();
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
