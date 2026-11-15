use super::*;
impl Settings {
    pub(super) fn backup(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let t = self.theme;
        let export = self.field("export-path", "settings.outputFile", "", false, window, cx);
        let export_password = self.field(
            "export-password",
            "settings.exportPassword",
            "",
            true,
            window,
            cx,
        );
        let import = self.field("import-path", "settings.inputFile", "", false, window, cx);
        let import_password = self.field(
            "import-password",
            "settings.importPassword",
            "",
            true,
            window,
            cx,
        );
        presentation::cards()
            .child(admin_ui::card(
                self.locale.text("settings.export"),
                t,
                presentation::rows()
                    .child(export)
                    .child(export_password)
                    .child(
                        admin_ui::row().justify_end().child(
                            self.icon_button(
                                "backup-export",
                                "settings.exportVault",
                                "download",
                                false,
                            )
                            .disabled(
                                self.pending
                                    || self.fields.value("export-path", cx).trim().is_empty()
                                    || self.fields.value("export-password", cx).is_empty(),
                            )
                            .on_click(cx.listener(|s, _, w, cx| {
                                let output = s.fields.value("export-path", cx).trim().into();
                                let export_password =
                                    SensitiveString::new(s.fields.value("export-password", cx));
                                s.fields.reset("export-password", w, cx);
                                s.apply(
                                    AgentRequest::VaultExport {
                                        output,
                                        export_password,
                                    },
                                    cx,
                                );
                            })),
                        ),
                    ),
            ))
            .child(admin_ui::card(
                self.locale.text("settings.import"),
                t,
                presentation::rows()
                    .child(import)
                    .child(import_password)
                    .child(admin_ui::hint(self.locale.text("settings.importDesc"), t))
                    .child(
                        admin_ui::row().justify_end().child(
                            self.icon_button(
                                "backup-import",
                                "settings.importAndLock",
                                "upload",
                                false,
                            )
                            .disabled(
                                self.pending
                                    || self.fields.value("import-path", cx).trim().is_empty()
                                    || self.fields.value("import-password", cx).is_empty(),
                            )
                            .on_click(
                                cx.listener(|s, _, w, cx| s.show_confirmation("import", w, cx)),
                            ),
                        ),
                    ),
            ))
            .into_any_element()
    }
    pub(super) fn import_backup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.fields.value("import-path", cx).trim().into();
        let export_password = SensitiveString::new(self.fields.value("import-password", cx));
        self.fields.reset("import-password", window, cx);
        self.apply(
            AgentRequest::VaultImport {
                input,
                export_password,
            },
            cx,
        );
    }
}
