use super::*;
use crate::parity::Observe;
use aipass_provider_registry::{CredentialKind, ProviderKind};
use gpui::{div, px, svg, AnyElement, FontWeight};
use gpui_base::{Button, Dialog, DialogPopup};

impl Subscriptions {
    pub(super) fn brand(&self, id: &str, title: &str, size: f32) -> gpui::Div {
        crate::provider_icon::icon(
            id,
            title,
            CredentialKind::OAuth,
            &ProviderKind::Official,
            size,
            self.dark,
        )
    }
    fn directory(&self, cx: &Context<Self>) -> AnyElement {
        let query = self.query.read(cx).value().trim().to_lowercase();
        let mut providers = vec![Provider {
            id: "claude".into(),
            name: "Claude".into(),
            name_key: None,
            local_authorization: false,
            methods: vec![],
        }];
        providers.extend(self.providers.clone());
        let visible = providers
            .into_iter()
            .filter(|p| {
                format!("{} {}", self.name(p), p.id)
                    .to_lowercase()
                    .contains(&query)
            })
            .collect::<Vec<_>>();
        div()
            .id("subscription-directory-body")
            .min_h_0()
            .overflow_y_scroll()
            .px(px(20.))
            .py(px(16.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(self.search())
            .when(self.pending == Some(Kind::Catalog), |el| {
                el.child(self.paragraph(self.text("common.loading"), 12.))
            })
            .when(!self.error.is_empty(), |el| {
                el.child(
                    self.paragraph(self.text("communityConnect.loadFailed"), 12.)
                        .line_height(px(15.)),
                )
            })
            .child(
                div()
                    .id("subscription-provider-grid")
                    .min_h_0()
                    .max_h(px(380.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .children(visible.chunks(2).enumerate().map(|(row, providers)| {
                        div()
                            .flex()
                            .gap(px(6.))
                            .flex_shrink_0()
                            .children(providers.iter().enumerate().map(|(column, p)| {
                                let id = p.id.clone();
                                let name = self.name(p);
                                let t = self.theme;
                                Button::new(format!("subscription-provider-{id}"))
                                    .flex_1()
                                    .min_w_0()
                                    .h(px(50.))
                                    .px(px(10.))
                                    .py(px(8.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(t.border)
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .text_color(t.text)
                                    .cursor_pointer()
                                    .accessibility_label(name.clone())
                                    .hover(move |s| s.bg(t.surface2).border_color(t.accent))
                                    .child(self.brand(&id, &name, 32.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(px(12.))
                                            .line_height(px(15.))
                                            .font_weight(FontWeight::BOLD)
                                            .child(name),
                                    )
                                    .child(
                                        svg().path("arrow-right").size(px(16.)).text_color(t.text),
                                    )
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.choose(id.clone(), window, cx)
                                    }))
                                    .observe(format!("subscription-provider-{}", row * 2 + column))
                            }))
                            .when(providers.len() == 1, |el| el.child(div().flex_1()))
                    }))
                    .when(visible.is_empty(), |el| {
                        el.child(
                            self.paragraph(self.text("communityConnect.noResults"), 12.)
                                .line_height(px(15.)),
                        )
                    })
                    .observe("subscription-provider-grid"),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .flex_shrink_0()
                    .child(
                        self.button(
                            "subscription-import",
                            self.text("oauthConnect.importCli"),
                            "terminal",
                            false,
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.show_import(window, cx))),
                    )
                    .when(!self.error.is_empty(), |el| {
                        el.child(
                            self.button(
                                "subscription-load-retry",
                                self.text("providerRuntime.retry"),
                                "",
                                false,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.error.clear();
                                this.queue(Operation::Catalog, cx);
                            })),
                        )
                    })
                    .observe("subscription-directory-actions"),
            )
            .observe("subscription-directory-body")
            .into_any_element()
    }
}
impl Render for Subscriptions {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame = self.modal_motion.sample(self.open, window, cx);
        if !frame.visible {
            if self.runtime_options.discard_after_close {
                self.discard_options_draft(window, cx);
            }
            if let Some(restore) = self.restore.take() {
                restore.focus(window, cx);
            }
        }
        let community = self.screen == Screen::Connect && self.provider != "claude";
        let importing = self.screen == Screen::Import;
        let runtime = self.screen == Screen::Options;
        let title = self.text(if runtime {
            "providerRuntime.title"
        } else if importing {
            "subscriptionImport.title"
        } else if self.reconnect_entry.is_some() {
            "oauthConnect.reauth"
        } else {
            "oauthConnect.title"
        });
        let t = self.theme;
        let header = div()
            .flex_shrink_0()
            .px(px(if community { 24. } else { 20. }))
            .py(px(if community {
                20.
            } else if importing {
                16.
            } else {
                14.
            }))
            .flex()
            .items_center()
            .gap(px(if community { 12. } else { 10. }))
            .border_b_1()
            .border_color(t.divider)
            .when(!importing, |el| {
                el.child(
                    div()
                        .when(community, |el| {
                            el.size(px(36.))
                                .rounded(px(10.))
                                .border_1()
                                .border_color(t.border)
                                .flex()
                                .items_center()
                                .justify_center()
                        })
                        .child(
                            svg()
                                .path("key-round")
                                .size(px(18.))
                                .text_color(t.secondary),
                        ),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .min_w_0()
                            .text_size(px(15.))
                            .line_height(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title.clone()),
                    )
                    .when(importing, |el| el.child(self.import_count())),
            )
            .child(
                self.icon_button(
                    "subscription-close",
                    "x",
                    self.text("common.close"),
                    false,
                    false,
                )
                .track_focus(&self.focus)
                .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
            )
            .observe("subscription-header");
        let content = div()
            .w_full()
            .min_h_0()
            .max_h(window.viewport_size().height - px(50.))
            .flex()
            .flex_col()
            .text_color(t.text)
            .child(header)
            .child(match self.screen {
                Screen::Directory => self.directory(cx),
                Screen::Connect if self.provider == "claude" => div()
                    .id("subscription-claude-body")
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.body_scroll)
                    .px(px(20.))
                    .py(px(16.))
                    .child(self.claude(window, cx))
                    .observe("subscription-claude-body")
                    .into_any_element(),
                Screen::Connect => self.connection(window, cx),
                Screen::Import => self.import_body(cx),
                Screen::Options => self.options_content(window, cx),
            })
            .child(if importing {
                self.import_footer(window, cx)
            } else if community {
                self.connection_footer(cx)
            } else {
                div()
                    .flex_shrink_0()
                    .px(px(20.))
                    .py(px(10.))
                    .border_t_1()
                    .border_color(t.divider)
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(11.))
                    .line_height(px(14.))
                    .text_color(t.tertiary)
                    .child(
                        svg()
                            .path("shield-check")
                            .size(px(14.))
                            .flex_shrink_0()
                            .text_color(t.tertiary),
                    )
                    .child(self.text("communityConnect.localNote"))
                    .observe("subscription-footer")
                    .into_any_element()
            });
        let content = if runtime {
            self.options_content(window, cx)
        } else {
            content.into_any_element()
        };
        let owner = cx.entity().downgrade();
        Dialog::new(cx)
            .open(frame.visible)
            // The dialog scope and its close button must own distinct AX focus nodes.
            .focus_handle(self.dialog_focus.clone())
            .accessibility_label(title)
            .on_open_change(move |open, _, window, cx| {
                if !open {
                    let _ = owner.update(cx, |this, cx| {
                        if let Some(key) = this.picker.take() {
                            if let Some(trigger) = this.picker_triggers.get(&key) {
                                trigger.focus(window, cx);
                            }
                            cx.notify();
                        } else {
                            this.close(window, cx);
                        }
                    });
                }
            })
            .backdrop(
                div()
                    .absolute()
                    .inset_0()
                    .bg(gpui::rgba(0x0f111073))
                    .backdrop_blur(px(4.))
                    .opacity(frame.overlay),
            )
            .on_ok(|_, _, _| false)
            .popup(motion::Slide::new(
                crate::composite::Scale::new(
                    div().opacity(frame.popup).child(
                        DialogPopup::new()
                            .w(px(if runtime {
                                680.
                            } else if importing {
                                840.
                            } else if community {
                                720.
                            } else {
                                620.
                            }))
                            .max_h(window.viewport_size().height - px(48.))
                            .flex()
                            .flex_col()
                            .rounded(px(14.))
                            .border_1()
                            .border_color(t.border)
                            .bg(t.surface)
                            .shadow(vec![gpui::BoxShadow {
                                color: if self.dark {
                                    gpui::rgba(0x00000080).into()
                                } else {
                                    gpui::rgba(0x0f111038).into()
                                },
                                offset: gpui::point(px(0.), px(24.)),
                                blur_radius: crate::theme::shadow_blur(64.),
                                spread_radius: px(0.),
                                inset: false,
                            }])
                            .overflow_hidden()
                            .child(content),
                    ),
                    frame.scale,
                ),
                frame.offset,
            ))
    }
}
