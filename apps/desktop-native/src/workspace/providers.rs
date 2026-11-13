use super::{Workspace, WorkspaceEvent};
use crate::providers::{Form, FormEvent};
use aipass_desktop_native::catalog::{Action, Catalog};
use aipass_vault::EntrySummary;
use gpui::{prelude::*, Context, Window};
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
        self.tools
            .update(cx, |view, cx| view.sync(entry.clone(), window, cx));
        self.notes
            .update(cx, |view, cx| view.sync(entry.as_ref(), window, cx));
        let editing = self.form_is_editing(cx);
        self.subscriptions
            .update(cx, |view, cx| view.sync_entry(entry.clone(), window, cx));
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
            "selectedProvider": self.selected_provider, "hoveredProvider": self.hovered_provider,
            "providerTools":self.tools.read(cx).presentation(),"subscription":self.subscriptions.read(cx).presentation(),"credentials": self.credentials.read(cx).presentation_state(), "integration":self.integrations.read(cx).presentation(cx)})
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
        if scenario.starts_with("subscription-") {
            self.catalog = Catalog::fixture();
            self.catalog.active = vec![crate::subscription_ui::fixtures::entry(scenario)];
            self.select_visible(window, cx);
            self.sync_credentials(window, cx);
            self.subscriptions
                .update(cx, |view, cx| view.fixture(scenario, window, cx));
            return;
        }
        if !scenario.starts_with("providers") {
            return;
        }
        self.catalog = Catalog::fixture();
        if scenario == "providers-notes-empty" {
            self.catalog.active[0].notes = None;
        }
        if scenario == "providers-notes-long" {
            self.catalog.active[0].notes=Some("Synthetic provider. Long notes wrap without changing the card width. 中文备注支持换行和复制。\n".repeat(12));
        }
        self.select_visible(window, cx);
        if scenario == "providers-list-selected-hover" {
            self.selected_provider = Some(self.catalog.active[1].id);
        }
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
        if matches!(scenario, "providers-add-menu" | "providers-add-all") {
            if let Some(form) = &self.provider_form {
                form.update(cx, |form, _| form.preview_optional(scenario));
            }
        }
        self.credentials
            .update(cx, |view, cx| view.preview(scenario, window, cx));
        self.integrations
            .update(cx, |view, cx| view.fixture(scenario, window, cx));
        self.tools
            .update(cx, |view, cx| view.fixture(scenario, window, cx));
        self.provider_busy = false;
    }

    #[cfg(feature = "parity-render")]
    pub fn prepare_notes_scroll(&self) {
        // Use the same reachable offset as the frozen reference, rather than
        // comparing two different clamped scroll positions at large heights.
        let offset = if self.detail_scroll.bounds().size.height > gpui::px(600.) {
            150.
        } else {
            300.
        };
        self.detail_scroll.set_offset(gpui::point(
            gpui::px(0.),
            -gpui::px(offset).min(self.detail_scroll.max_offset().y),
        ));
    }
    pub fn take_provider_action(&mut self) -> Option<Action> {
        self.provider_action.take()
    }
    #[cfg(feature = "parity-render")]
    pub fn prepare_subscription_overview_scroll(&self) {
        self.detail_scroll.set_offset(gpui::point(
            gpui::px(0.),
            -self.detail_scroll.max_offset().y,
        ));
    }
    #[cfg(feature = "parity-render")]
    pub fn prepare_subscription_scroll(&self, cx: &gpui::App) {
        self.subscriptions.read(cx).prepare_capture_scroll();
    }
    #[cfg(feature = "parity-render")]
    pub fn prepare_form_scroll(&self, cx: &gpui::App) {
        if let Some(form) = &self.provider_form {
            form.read(cx).prepare_capture_scroll();
        }
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
                self.tag_filter = None;
                self.search
                    .update(cx, |input, cx| input.set_value("", window, cx));
            }
            self.close_provider_form(window, cx);
        }
        if let Some(selected) = selected {
            self.selected_provider = Some(selected);
        }
        if let Some(id) = self
            .subscriptions
            .read(cx)
            .connected
            .filter(|id| self.catalog.find(*id).is_some())
        {
            self.selected_provider = Some(id);
            self.selected = 0;
            self.tag_filter = None;
            self.search
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.subscriptions
                .update(cx, |view, _| view.connected = None);
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
        self.subscriptions
            .update(cx, |view, cx| view.clear(window, cx));
        self.tools.update(cx, |view, cx| view.clear(window, cx));
        self.notes
            .update(cx, |view, cx| view.sync(None, window, cx));
        self.catalog = Catalog::default();
        self.tag_filter = None;
        self.hovered_provider = None;
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
            .into_iter()
            .filter(|entry| {
                self.tag_filter
                    .as_ref()
                    .is_none_or(|tag| entry.tags.contains(tag))
            })
            .collect()
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
    pub(super) fn request_provider(&mut self, action: Action, cx: &mut Context<Self>) {
        if self.provider_busy {
            return;
        }
        self.provider_menu_open = false;
        self.provider_busy = true;
        self.provider_action = Some(action);
        cx.emit(WorkspaceEvent::Provider);
        cx.notify();
    }
}
