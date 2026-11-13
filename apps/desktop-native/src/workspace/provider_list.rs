//! Provider list rows and selection.
use super::Workspace;
use crate::{parity::Observe, providers::presentation};
use gpui::{div, prelude::*, px, Context, FontWeight, Rgba, Window};
use gpui_base::{
    motion::{transition, Interpolate},
    Button, Transition,
};
use std::time::Duration;

#[derive(Clone, PartialEq)]
struct RowColor(Rgba);

impl Interpolate for RowColor {
    fn interpolate(&self, target: &Self, progress: f32) -> Self {
        // Interpolate associated alpha so transparent idle rows do not darken
        // their neighbors during the background fade.
        let (from, to) = (self.0, target.0);
        let alpha = from.a + (to.a - from.a) * progress;
        let channel = |a: f32, b: f32| {
            if alpha > 0. {
                (a * from.a * (1. - progress) + b * to.a * progress) / alpha
            } else {
                0.
            }
        };
        Self(Rgba {
            r: channel(from.r, to.r),
            g: channel(from.g, to.g),
            b: channel(from.b, to.b),
            a: alpha,
        })
    }
}

fn timing(milliseconds: u64) -> Transition {
    Transition::new(Duration::from_millis(milliseconds)).ease(|t| {
        aipass_desktop_native::motion::cubic_bezier(f64::from(t), 0.25, 0.1, 0.25, 1.) as f32
    })
}

impl Workspace {
    pub(super) fn provider_list(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = self.theme;
        div()
            .id("provider-entries")
            .role(gpui::Role::ListBox)
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(8.))
            .pb(px(8.))
            .flex()
            .flex_col()
            .children(
                self.visible_providers(cx)
                    .into_iter()
                    .enumerate()
                    .map(|(index, entry)| {
                        let id = entry.id;
                        let selected = self.selected_provider == Some(id);
                        let background = transition(
                            format!("provider-row-background-{id}"),
                            RowColor(if selected {
                                t.accent_soft
                            } else if self.hovered_provider == Some(id) {
                                t.surface2
                            } else {
                                gpui::rgba(0)
                            }),
                            timing(80),
                            window,
                            cx,
                        )
                        .0;
                        let foreground = transition(
                            format!("provider-row-title-{id}"),
                            RowColor(if selected { t.accent } else { t.text }),
                            timing(120),
                            window,
                            cx,
                        )
                        .0;
                        let account = (entry.credential_kind
                            == aipass_provider_registry::CredentialKind::OAuth)
                            .then(|| {
                                aipass_desktop_native::subscriptions::presentation::account(
                                    entry,
                                    &|key| self.locale.text(key),
                                )
                            });
                        let title = account
                            .as_ref()
                            .map(|a| a.title.clone())
                            .unwrap_or_else(|| entry.title.clone());
                        let subtitle = account.map(|a| a.subtitle).unwrap_or_else(|| {
                            entry
                                .account_identity
                                .clone()
                                .or_else(|| entry.domains.first().cloned())
                                .or_else(|| {
                                    entry
                                        .endpoints
                                        .first()
                                        .and_then(|endpoint| endpoint.url.clone())
                                })
                                .unwrap_or_default()
                        });
                        Button::new(format!("provider-entry-{id}"))
                            .role(gpui::Role::ListBoxOption)
                            .aria_selected(selected)
                            .w_full()
                            .h(px(56.))
                            .flex_shrink_0()
                            .p(px(8.))
                            .rounded(px(8.))
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .bg(background)
                            .cursor_pointer()
                            .text_color(foreground)
                            .on_hover(cx.listener(move |this, hovered, _, cx| {
                                if *hovered {
                                    this.hovered_provider = Some(id);
                                } else if this.hovered_provider == Some(id) {
                                    this.hovered_provider = None;
                                }
                                cx.notify();
                            }))
                            .accessibility_label(title.clone())
                            .child(
                                div().w(px(36.)).flex_shrink_0().child(
                                    presentation::identity(entry, false, self.dark)
                                        .observe(format!("provider-list-icon-{index}")),
                                ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div().flex().items_center().gap(px(6.))
                                        .child(div().flex_1().min_w_0()
                                            .text_size(px(13.))
                                            .line_height(px(16.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_ellipsis()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .child(title)
                                            .observe(format!("provider-list-title-{index}")))
                                        .when(entry.credential_kind == aipass_provider_registry::CredentialKind::OAuth, |el| {
                                            el.child(div().flex_shrink_0().px(px(8.)).py(px(3.)).border_1().border_color(t.border).rounded_full().text_size(px(11.)).line_height(px(11.)).font_weight(FontWeight::MEDIUM).text_color(t.secondary).child(self.locale.text("providerDetail.oauth")))
                                        }),
                                    )
                                    .when(!subtitle.is_empty(), |el| {
                                        el.child(
                                            div()
                                                .text_size(px(12.))
                                                .line_height(px(15.))
                                                .text_color(t.tertiary)
                                                .text_ellipsis()
                                                .overflow_hidden()
                                                .whitespace_nowrap()
                                                .child(subtitle)
                                                .observe(format!("provider-list-subtitle-{index}")),
                                        )
                                    }),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if this.provider_busy {
                                    return;
                                }
                                this.close_provider_form(window, cx);
                                this.selected_provider = Some(id);
                                this.sync_credentials(window, cx);
                                this.provider_advanced = false;
                                cx.notify();
                            }))
                            .observe(format!("provider-list-row-{index}"))
                    }),
            )
            .observe("provider-list-entries")
    }
}
