use super::{Workspace, WorkspaceEvent};
use crate::providers::presentation;
use crate::providers::{Form, FormEvent};
use aipass_desktop_native::catalog::{Action, Catalog};
use aipass_provider_registry::{CredentialKind, EndpointKind};
use aipass_vault::EntrySummary;
use gpui::{div, prelude::*, px, svg, Anchor, AnyElement, Context, FontWeight, Window};
use gpui_base::{Button, Popover};
use uuid::Uuid;

#[derive(Default)]
pub struct ProviderFeedback {
    pub selected: Option<Uuid>,
    pub busy: bool,
    pub saved: bool,
    pub error: Option<String>,
}

impl Workspace {
    pub fn take_integration_request(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<aipass_desktop_native::integration::Request> {
        self.integrations.update(cx, |view, _| view.take_request())
    }
    pub fn complete_integration(
        &mut self,
        completion: aipass_desktop_native::integration::Completion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.provider_busy = false;
        self.integrations
            .update(cx, |view, cx| view.complete(completion, window, cx));
    }
    pub fn take_credential_request(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<aipass_desktop_native::credentials::Request> {
        self.credentials.update(cx, |view, _| view.take_request())
    }
    pub fn complete_credential(
        &mut self,
        completion: aipass_desktop_native::credentials::Completion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.provider_busy = false;
        self.credentials
            .update(cx, |view, cx| view.complete(completion, window, cx));
    }
    pub(super) fn sync_credentials(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entry = self
            .visible_providers(cx)
            .into_iter()
            .find(|entry| Some(entry.id) == self.selected_provider)
            .cloned();
        let editing = self.form_is_editing(cx);
        self.credentials.update(cx, |view, cx| {
            view.sync(entry.clone(), editing, self.provider_busy, window, cx)
        });
        self.integrations.update(cx, |view, cx| {
            view.sync(
                if matches!(self.selected, 9 | 10) {
                    None
                } else {
                    entry
                },
                self.provider_busy || editing,
                cx,
            )
        });
    }
    #[cfg(feature = "parity-render")]
    pub fn presentation_state(&self, cx: &gpui::App) -> serde_json::Value {
        serde_json::json!({"active": self.catalog.active.len(), "archived": self.catalog.archived.len(), "trash": self.catalog.trash.len(),
            "form": self.provider_form.as_ref().map(|form| if form.read(cx).is_editing() { "edit" } else { "add" }),
            "credentials": self.credentials.read(cx).presentation_state(), "integration":self.integrations.read(cx).presentation(cx)})
    }
    pub(super) fn select_visible(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let previous = self.selected_provider;
        let visible = self.visible_providers(cx);
        if !visible
            .iter()
            .any(|entry| Some(entry.id) == self.selected_provider)
        {
            self.selected_provider = visible.first().map(|entry| entry.id);
        }
        if previous != self.selected_provider {
            if self.form_is_editing(cx) {
                self.close_provider_form(window, cx);
            }
            self.sync_credentials(window, cx);
        }
    }
    #[cfg(feature = "parity-render")]
    pub fn preview_providers(
        &mut self,
        scenario: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !scenario.starts_with("providers") {
            return;
        }
        self.catalog = Catalog::fixture();
        self.select_visible(window, cx);
        self.sync_credentials(window, cx);
        if scenario.starts_with("providers-add") {
            self.open_provider_form(None, window, cx);
        }
        if scenario.starts_with("providers-edit") {
            self.open_provider_form(self.catalog.active.first().cloned(), window, cx);
        }
        if scenario.ends_with("-advanced") {
            if let Some(form) = &self.provider_form {
                form.update(cx, |form, _| form.preview_advanced());
            }
        }
        self.credentials
            .update(cx, |view, cx| view.preview(scenario, window, cx));
        self.integrations
            .update(cx, |view, cx| view.fixture(scenario, window, cx));
        self.provider_busy = false;
    }

    pub fn take_provider_action(&mut self) -> Option<Action> {
        self.provider_action.take()
    }
    pub fn has_provider_form(&self) -> bool {
        self.provider_form.is_some()
    }
    pub(super) fn form_is_editing(&self, cx: &gpui::App) -> bool {
        self.provider_form
            .as_ref()
            .is_some_and(|form| form.read(cx).is_editing())
    }
    pub fn sync_providers(
        &mut self,
        catalog: Catalog,
        feedback: ProviderFeedback,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ProviderFeedback {
            selected,
            busy,
            saved,
            error,
        } = feedback;
        let previous = self.selected_provider;
        self.catalog = catalog;
        self.provider_busy = busy;
        if saved {
            if self
                .provider_form
                .as_ref()
                .is_some_and(|form| !form.read(cx).is_editing())
            {
                self.selected = 0;
                self.search
                    .update(cx, |input, cx| input.set_value("", window, cx));
            }
            self.close_provider_form(window, cx);
        }
        if let Some(selected) = selected {
            self.selected_provider = Some(selected);
        }
        let visible = self.visible_providers(cx);
        if selected.is_none()
            && !visible
                .iter()
                .any(|entry| Some(entry.id) == self.selected_provider)
        {
            self.selected_provider = visible.first().map(|entry| entry.id);
        }
        if previous != self.selected_provider && self.form_is_editing(cx) {
            self.close_provider_form(window, cx);
        }
        self.sync_credentials(window, cx);
        if let Some(form) = &self.provider_form {
            form.update(cx, |form, cx| {
                if let Some(entry) = self.selected_provider.and_then(|id| self.catalog.find(id)) {
                    form.refresh_original(entry, window, cx);
                }
                form.set_busy(busy, cx);
                if let Some(error) = error {
                    form.set_error(error);
                }
            });
        }
        cx.notify();
    }
    pub fn clear_providers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_provider_form(window, cx);
        self.credentials
            .update(cx, |view, cx| view.clear(window, cx));
        self.integrations.update(cx, |view, cx| view.clear(cx));
        self.catalog = Catalog::default();
        self.selected_provider = None;
        self.provider_action = None;
        self.provider_busy = false;
        self.provider_menu_open = false;
        self.provider_advanced = false;
        self.search
            .update(cx, |input, cx| input.set_value("", window, cx));
        cx.notify();
    }
    pub(super) fn visible_providers(&self, cx: &gpui::App) -> Vec<&EntrySummary> {
        self.catalog
            .entries(self.selected, self.search.read(cx).value().as_ref())
    }
    pub(super) fn open_provider_form(
        &mut self,
        entry: Option<EntrySummary>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.provider_busy {
            return;
        }
        self.close_provider_form(window, cx);
        let form = cx.new(|cx| Form::new(entry, self.dark, self.chinese, window, cx));
        form.update(cx, |form, cx| {
            form.attach_credentials(self.credentials.clone(), cx)
        });
        self.form_subscription =
            Some(
                cx.subscribe_in(&form, window, |this, form, event, window, cx| {
                    match event {
                        FormEvent::Close => this.close_provider_form(window, cx),
                        FormEvent::Save => {
                            this.provider_action = form.update(cx, |form, _| form.take_action());
                            this.provider_busy = true;
                            cx.emit(WorkspaceEvent::Provider);
                        }
                    }
                    cx.notify();
                }),
            );
        self.provider_form = Some(form);
        cx.notify();
    }
    pub(super) fn close_provider_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(form) = self.provider_form.take() {
            form.update(cx, |form, cx| form.clear(window, cx));
        }
        self.form_subscription = None;
    }
    fn request_provider(&mut self, action: Action, cx: &mut Context<Self>) {
        if self.provider_busy {
            return;
        }
        self.provider_menu_open = false;
        self.provider_busy = true;
        self.provider_action = Some(action);
        cx.emit(WorkspaceEvent::Provider);
        cx.notify();
    }
    pub(super) fn provider_list(&self, cx: &Context<Self>) -> impl IntoElement {
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
                        let subtitle = entry
                            .account_identity
                            .clone()
                            .or_else(|| entry.domains.first().cloned())
                            .or_else(|| {
                                entry
                                    .endpoints
                                    .first()
                                    .and_then(|endpoint| endpoint.url.clone())
                            })
                            .unwrap_or_default();
                        Button::new(("provider-entry", index))
                            .role(gpui::Role::ListBoxOption)
                            .w_full()
                            .h(px(56.))
                            .flex_shrink_0()
                            .p(px(8.))
                            .rounded(px(10.))
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .bg(if selected {
                                t.accent_soft
                            } else {
                                gpui::rgba(0)
                            })
                            .cursor_pointer()
                            .text_color(if selected { t.accent } else { t.text })
                            .hover(move |style| style.bg(t.accent_soft))
                            .accessibility_label(entry.title.clone())
                            .child(
                                div()
                                    .w(px(36.))
                                    .flex_shrink_0()
                                    .child(presentation::identity(entry, false, self.dark)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_ellipsis()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .child(entry.title.clone()),
                                    )
                                    .when(!subtitle.is_empty(), |el| {
                                        el.child(
                                            div()
                                                .text_size(px(12.))
                                                .text_color(t.tertiary)
                                                .text_ellipsis()
                                                .overflow_hidden()
                                                .whitespace_nowrap()
                                                .child(subtitle),
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
                    }),
            )
    }
    pub(super) fn provider_detail(&self, _: &Window, cx: &Context<Self>) -> AnyElement {
        if self.form_is_editing(cx) {
            return self
                .provider_form
                .as_ref()
                .unwrap()
                .clone()
                .into_any_element();
        }
        let visible = self.visible_providers(cx);
        let entry = self
            .selected_provider
            .and_then(|id| visible.iter().find(|entry| entry.id == id).copied())
            .or_else(|| visible.first().copied());
        let Some(entry) = entry else {
            return self
                .empty(
                    "providerDetail.noneSelected",
                    "providerDetail.noneSelectedDesc",
                    true,
                    cx,
                )
                .into_any_element();
        };
        let t = self.theme;
        let id = entry.id;
        let editing = entry.clone();
        let favorite = entry.favorite;
        let card = || {
            div()
                .w_full()
                .flex_shrink_0()
                .bg(t.surface.opacity(0.86))
                .rounded(px(10.))
                .border_1()
                .border_color(t.border)
                .overflow_hidden()
                .flex()
                .flex_col()
        };
        let row = |label: String, value: String| {
            div()
                .flex_shrink_0()
                .px(px(16.))
                .py(px(12.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .border_b_1()
                .border_color(t.divider)
                .child(div().text_size(px(11.)).text_color(t.tertiary).child(label))
                .child(div().text_size(px(13.)).child(value))
        };
        let owner = cx.entity().downgrade();
        let archive = self.locale.text("sidebar.archive");
        let trash = self.locale.text("providerDetail.moveToTrash");
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(20.))
                    .py(px(18.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .border_b_1()
                    .border_color(t.divider)
                    .child(presentation::identity(entry, true, self.dark))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .h(px(28.))
                                            .line_height(px(28.))
                                            .text_size(px(18.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_ellipsis()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .child(entry.title.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .when(self.selected != 10, |el| {
                                                el.child(
                                                    Button::new("favorite-provider")
                                                        .size(px(32.)).rounded(px(8.)).flex_shrink_0()
                                                        .flex().items_center().justify_center().cursor_pointer()
                                                        .accessibility_label(self.locale.text(if favorite {
                                                            "providerDetail.removeFavorite"
                                                        } else {
                                                            "providerDetail.addFavorite"
                                                        }))
                                                        .child(svg().path(if favorite { "star-filled" } else { "star" })
                                                            .size(px(16.)).text_color(if favorite { t.accent } else { t.secondary }))
                                                        .hover(move |s| s.bg(t.accent_soft))
                                                    .when(favorite, |el| el.bg(t.accent_soft))
                                                    .disabled(self.provider_busy)
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.request_provider(
                                                            Action::Favorite(id, !favorite),
                                                            cx,
                                                        )
                                                    })),
                                                )
                                            })
                                            .when(matches!(self.selected, 9 | 10), |el| {
                                                el.child(
                                                    self.button(
                                                        "restore-provider",
                                                        "providerDetail.restore",
                                                        "undo-2",
                                                        true,
                                                        false,
                                                    )
                                                    .disabled(self.provider_busy)
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.request_provider(
                                                            Action::Restore(id),
                                                            cx,
                                                        )
                                                    })),
                                                )
                                            })
                                            .when(
                                                !matches!(self.selected, 9 | 10)
                                                    && entry.credential_kind == CredentialKind::Api,
                                                |el| {
                                                    el.child(
                                                        self.button(
                                                            "edit-provider",
                                                            "providerDetail.edit",
                                                            "pencil",
                                                            true,
                                                            false,
                                                        )
                                                        .disabled(self.provider_busy)
                                                        .on_click(cx.listener(
                                                            move |this, _, window, cx| {
                                                                this.open_provider_form(
                                                                    Some(editing.clone()),
                                                                    window,
                                                                    cx,
                                                                )
                                                            },
                                                        )),
                                                    )
                                                },
                                            )
                                            .when(self.selected != 10, |el| {
                                                el.child(
                                    Popover::new("provider-actions")
                                        .open(self.provider_menu_open)
                                        .on_open_change(cx.listener(|this, open: &bool, _, cx| {
                                            this.provider_menu_open = *open;
                                            cx.notify();
                                        }))
                                        .anchor(Anchor::TopRight)
                                        .offset(px(6.))
                                        .trigger(
                                            Button::new("provider-more")
                                                .size(px(32.)).rounded(px(8.)).border_1().border_color(t.border)
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .accessibility_label(
                                                    self.locale.text("providerDetail.moreActions"),
                                                )
                                                .child(
                                                    svg()
                                                        .path("ellipsis")
                                                        .size(px(16.))
                                                        .text_color(t.secondary),
                                                ),
                                        )
                                        .content(move |_, _, _| {
                                            div()
                                                .w(px(180.))
                                                .p(px(4.))
                                                .rounded(px(8.))
                                                .border_1()
                                                .border_color(t.border)
                                                .bg(t.surface)
                                                .flex()
                                                .flex_col()
                                                .children(
                                                    [(0_usize, archive), (1, trash)]
                                                        .into_iter()
                                                        .map(|(index, label)| {
                                                            let owner = owner.clone();
                                                            Button::new(("provider-action", index))
                                                                .px(px(10.))
                                                                .py(px(7.))
                                                                .text_size(px(13.))
                                                                .text_color(t.text)
                                                                .hover(move |style| {
                                                                    style.bg(t.accent_soft)
                                                                })
                                                                .accessibility_label(label.clone())
                                                                .child(label)
                                                                .on_click(move |_, _, cx| {
                                                                    let _ = owner.update(
                                                                        cx,
                                                                        |this, cx| {
                                                                            this.request_provider(
                                                                                if index == 0 {
                                                                                    Action::Archive(
                                                                                        id,
                                                                                    )
                                                                                } else {
                                                                                    Action::Trash(
                                                                                        id,
                                                                                    )
                                                                                },
                                                                                cx,
                                                                            )
                                                                        },
                                                                    );
                                                                })
                                                        }),
                                                )
                                        }),
                                )
                                            }),
                                    ),
                            )
                            .child(presentation::badges(entry, t, &self.locale, self.dark)),
                    ),
            )
            .child(
                div()
                    .id("provider-detail-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(24.))
                    .pt(px(24.))
                    .pb(px(36.))
                    .flex()
                    .flex_col()
                    .gap(px(18.))
                    .child(
                        card()
                            .child(
                                div()
                                    .px(px(16.))
                                    .py(px(12.))
                                    .text_size(px(13.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .border_b_1()
                                    .border_color(t.divider)
                                    .child(self.locale.text("providerDetail.credentials")),
                            )
                            .children(
                                entry
                                    .endpoints
                                    .iter()
                                    .filter(|endpoint| endpoint.kind == EndpointKind::Api)
                                    .map(|endpoint| {
                                        row(
                                            self.locale.text("providerDetail.endpoint"),
                                            endpoint.url.clone().unwrap_or_default(),
                                        )
                                    }),
                            )
                            .child(self.credentials.clone())
                            .child(row(
                                self.locale.text("providerDetail.defaultModel"),
                                entry.default_model.clone().unwrap_or_else(|| "—".into()),
                            ))
                            .child(row(
                                self.locale.text("providerDetail.tags"),
                                if entry.tags.is_empty() {
                                    "—".into()
                                } else {
                                    entry.tags.join(", ")
                                },
                            ))
                            .child(
                                Button::new("provider-detail-advanced")
                                    .w_full()
                                    .px(px(16.))
                                    .py(px(13.))
                                    .text_size(px(13.))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .accessibility_label(self.locale.text("providerForm.advanced"))
                                    .child(
                                        div()
                                            .flex_1()
                                            .child(self.locale.text("providerForm.advanced")),
                                    )
                                    .child(
                                        svg()
                                            .path(if self.provider_advanced {
                                                "chevron-down"
                                            } else {
                                                "chevron-right"
                                            })
                                            .size(px(14.)),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.provider_advanced = !this.provider_advanced;
                                        cx.notify();
                                    })),
                            )
                            .when(self.provider_advanced, |el| {
                                el.child(row(
                                    self.locale.text("providerForm.provider"),
                                    entry.provider_id.clone().unwrap_or_default(),
                                ))
                                .child(row(
                                    self.locale.text("providerForm.domains"),
                                    entry.domains.join(", "),
                                ))
                                .child(row(
                                    self.locale.text("providerDetail.aliases"),
                                    entry
                                        .model_aliases
                                        .iter()
                                        .map(|(a, b)| format!("{a}={b}"))
                                        .collect::<Vec<_>>()
                                        .join(", "),
                                ))
                                .child(row(
                                    self.locale.text("providerDetail.headers"),
                                    entry.header_names.join(", "),
                                ))
                            }),
                    )
                    .when(entry.quota.is_some(), |el| {
                        el.child(
                            card().child(row(
                                entry
                                    .quota
                                    .as_ref()
                                    .and_then(|quota| quota.label.clone())
                                    .unwrap_or_default(),
                                entry
                                    .quota
                                    .as_ref()
                                    .map(|quota| {
                                        format!(
                                            "{} {}",
                                            quota.remaining.as_deref().unwrap_or_default(),
                                            quota.unit.as_deref().unwrap_or_default()
                                        )
                                    })
                                    .unwrap_or_default(),
                            )),
                        )
                    })
                    .when_some(entry.notes.clone(), |el, value| {
                        el.child(card().child(row(self.locale.text("providerDetail.notes"), value)))
                    })
                    .child(self.integrations.clone()),
            )
            .into_any_element()
    }
}
