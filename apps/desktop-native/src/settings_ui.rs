//! Global settings drawer. Requests are scoped to a drawer lifetime.
use crate::{
    admin_ui::{self, Fields},
    locale::Locale,
    theme::Theme,
};
use aipass_agent_protocol::*;
use aipass_desktop_native::administration::{
    Completion, Operation, Outcome, Request, Scope, Surface,
};
use aipass_desktop_platform::preferences::AppPreferences;
use gpui::{prelude::*, Context, EventEmitter, FocusHandle, ScrollHandle, Window};
use std::collections::VecDeque;
mod about;
mod backup;
mod choices;
mod copying;
#[cfg(all(test, feature = "parity-render"))]
mod failure_tests;
mod feedback;
#[cfg(feature = "parity-render")]
mod fixtures;
mod general;
mod motion;
mod panel;
mod panel_files;
mod presentation;
mod security;
mod server;
mod sync;
mod sync_records;
#[cfg(all(test, feature = "parity-render"))]
mod tests;
mod view;
const TABS: [&str; 6] = [
    "settings.general",
    "settings.security",
    "settings.sync",
    "settings.server",
    "settings.backup",
    "settings.about",
];
pub enum Event {
    CheckUpdates,
    UpdateChannel(UpdateChannel),
    InstallUpdate,
}
pub struct Settings {
    pub open: bool,
    tab: usize,
    theme: Theme,
    locale: Locale,
    pub(crate) preferences: AppPreferences,
    connected: bool,
    generation: u64,
    pending: bool,
    pending_read: bool,
    outbox: VecDeque<Request>,
    refresh_after: bool,
    loading: bool,
    load_failed: bool,
    submitted_preferences: bool,
    error: String,
    notice: String,
    build_time: String,
    fields: Fields,
    choices: std::collections::BTreeMap<String, gpui::Entity<crate::form_ui::select::Choice>>,
    choice_subscriptions: Vec<gpui::Subscription>,
    segments: std::collections::BTreeMap<String, Vec<FocusHandle>>,
    sync_settings: SyncSettings,
    sync_mode: SyncMode,
    sync_loaded: bool,
    conflicts: Vec<SyncConflictResponse>,
    devices: Vec<aipass_vault::DeviceRecord>,
    detection: Option<CcSwitchDetection>,
    config: Option<aipass_proxy::ProxyConfig>,
    panel: Option<ControlPanelStatus>,
    allow_remote_unlock: bool,
    certificate_open: bool,
    certificate_motion: crate::disclosure::Disclosure,
    certificate_file: Option<(String, String)>,
    private_key_file: Option<(String, SensitiveString)>,
    picking: bool,
    copying: bool,
    copy_cancel: Option<aipass_desktop_platform::clipboard::Cancellation>,
    access_code: Option<(SensitiveString, std::time::Instant)>,
    confirm: Option<&'static str>,
    confirm_focus: FocusHandle,
    confirm_cancel: FocusHandle,
    confirm_previous: Option<FocusHandle>,
    extension: Option<aipass_desktop_platform::browser::BrowserExtensionStatus>,
    update_channel: UpdateChannel,
    pub update: Option<aipass_desktop_native::updates::Snapshot>,
    scroll: ScrollHandle,
    feedback_scroll: ScrollHandle,
    focus: FocusHandle,
    close_focus: FocusHandle,
    tab_focus: [FocusHandle; 6],
    previous_focus: Option<FocusHandle>,
    motion: motion::DrawerMotion,
}
impl EventEmitter<Event> for Settings {}
impl Settings {
    fn select_update_channel(&mut self, channel: UpdateChannel, cx: &mut Context<Self>) {
        let Some(update) = &self.update else {
            self.updates_unavailable(cx);
            return;
        };
        if update.busy {
            return;
        }
        // The worker owns the selected feed. Keep the acknowledged channel
        // visible if switching fails or the bounded command queue is full.
        self.error.clear();
        cx.emit(Event::UpdateChannel(channel));
        cx.notify();
    }
    pub fn sync_update(
        &mut self,
        update: Option<aipass_desktop_native::updates::Snapshot>,
        cx: &mut Context<Self>,
    ) {
        if let Some(channel) = update.as_ref().and_then(|u| u.channel) {
            self.update_channel = channel;
        }
        if serde_json::to_string(&self.update).ok() != serde_json::to_string(&update).ok() {
            self.update = update;
            cx.notify();
        }
    }
    pub fn updates_unavailable(&mut self, cx: &mut Context<Self>) {
        self.error = self.locale.text("error.updateReleaseUnavailable");
        cx.notify();
    }
    pub fn new(dark: bool, chinese: bool, cx: &mut Context<Self>) -> Self {
        Self {
            open: false,
            tab: 0,
            theme: Theme::new(dark),
            locale: Locale::new(chinese),
            preferences: AppPreferences::default(),
            connected: false,
            generation: 0,
            pending: false,
            pending_read: false,
            outbox: VecDeque::new(),
            refresh_after: false,
            loading: false,
            load_failed: false,
            error: String::new(),
            notice: String::new(),
            build_time: option_env!("AIPASS_BUILD_TIME").unwrap_or("—").into(),
            fields: Fields::with_plain_passwords(),
            choices: Default::default(),
            choice_subscriptions: vec![],
            segments: Default::default(),
            sync_settings: SyncSettings::default(),
            sync_mode: SyncMode::Local,
            sync_loaded: false,
            conflicts: vec![],
            devices: vec![],
            detection: None,
            config: None,
            panel: None,
            allow_remote_unlock: false,
            certificate_open: false,
            certificate_motion: Default::default(),
            certificate_file: None,
            private_key_file: None,
            picking: false,
            copying: false,
            copy_cancel: None,
            access_code: None,
            confirm: None,
            confirm_focus: cx.focus_handle(),
            confirm_cancel: cx.focus_handle(),
            confirm_previous: None,
            submitted_preferences: false,
            extension: None,
            update_channel: UpdateChannel::Official,
            update: None,
            scroll: ScrollHandle::new(),
            feedback_scroll: ScrollHandle::new(),
            focus: cx.focus_handle(),
            close_focus: cx.focus_handle(),
            tab_focus: std::array::from_fn(|_| cx.focus_handle()),
            previous_focus: None,
            motion: Default::default(),
        }
    }
    pub fn show(
        &mut self,
        tab: usize,
        connected: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear(window, cx);
        self.open = true;
        self.connected = connected;
        self.tab = tab.min(5);
        self.preferences = cx.global::<crate::preferences::Preferences>().0.clone();
        self.previous_focus = window.focused(cx);
        self.close_focus.focus(window, cx);
        if connected {
            self.load(cx);
        }
        cx.notify();
    }
    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.generation = self.generation.wrapping_add(1);
        self.pending = false;
        self.outbox.clear();
        self.refresh_after = false;
        self.loading = false;
        self.load_failed = false;
        self.fields.clear(window, cx);
        self.choices.clear();
        self.choice_subscriptions.clear();
        self.retire_panel();
        self.confirm = None;
        self.confirm_previous = None;
        self.submitted_preferences = false;
        self.sync_loaded = false;
        self.error.clear();
        self.notice.clear();
        self.config = None;
        self.panel = None;
        self.extension = None;
        self.detection = None;
        self.conflicts.clear();
        self.devices.clear();
        cx.notify();
    }
    fn show_confirmation(
        &mut self,
        kind: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_previous = window.focused(cx);
        self.confirm = Some(kind);
        self.confirm_cancel.focus(window, cx);
        cx.notify();
    }
    fn close_confirmation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm = None;
        if let Some(focus) = self.confirm_previous.take() {
            focus.focus(window, cx);
        }
        cx.notify();
    }
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending && !self.pending_read {
            return;
        }
        self.pending = false;
        self.open = false;
        self.retire_panel();
        self.confirm = None;
        self.generation = self.generation.wrapping_add(1);
        self.outbox.clear();
        if let Some(focus) = self.previous_focus.take() {
            focus.focus(window, cx);
        }
        cx.notify();
    }
    pub fn appearance(&mut self, dark: bool, cx: &mut Context<Self>) {
        self.theme = Theme::new(dark);
        self.fields.appearance(self.theme, cx);
        cx.notify();
    }
    pub fn take_request(&mut self) -> Option<Request> {
        self.outbox.pop_front()
    }
    fn request(&mut self, operation: Operation, refresh: bool, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        self.pending = true;
        self.load_failed = false;
        self.feedback_scroll
            .set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
        self.pending_read = matches!(
            &operation,
            Operation::LoadPreferences { .. }
                | Operation::LoadSync
                | Operation::LoadServer
                | Operation::DetectImport
                | Operation::BrowserExtension { install: false }
        );
        self.submitted_preferences = matches!(operation, Operation::Preferences { .. });
        self.refresh_after = refresh;
        self.error.clear();
        self.notice.clear();
        self.outbox.push_back(Request {
            scope: Scope {
                surface: Surface::Settings,
                generation: self.generation,
            },
            operation,
        });
        cx.notify();
    }
    fn apply(&mut self, request: AgentRequest, cx: &mut Context<Self>) {
        self.request(Operation::Apply(Box::new(request)), true, cx);
    }
    fn load(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        let op = match self.tab {
            0 | 1 => Operation::LoadPreferences {
                path: cx.global::<crate::preferences::Preferences>().1.clone(),
                preferences: self.preferences.clone(),
            },
            2 => Operation::LoadSync,
            3 => Operation::LoadServer,
            5 => Operation::BrowserExtension { install: false },
            _ => {
                self.loading = false;
                return;
            }
        };
        self.request(op, false, cx);
    }
    fn select_tab(&mut self, tab: usize, window: &mut Window, cx: &mut Context<Self>) {
        if (self.pending && !self.pending_read) || self.tab == tab {
            return;
        }
        self.fields.clear(window, cx);
        self.retire_panel();
        self.generation = self.generation.wrapping_add(1);
        self.choices.clear();
        self.choice_subscriptions.clear();
        self.tab = tab;
        self.sync_loaded = false;
        self.pending = false;
        self.outbox.clear();
        self.loading = false;
        self.error.clear();
        self.notice.clear();
        self.scroll
            .set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
        if self.connected {
            self.load(cx);
        }
        cx.notify();
    }
    fn save_preferences(&mut self, cx: &mut Context<Self>) {
        if !self.connected {
            cx.set_global(crate::preferences::Preferences(
                self.preferences.clone(),
                None,
            ));
            cx.notify();
            return;
        }
        self.request(
            Operation::Preferences {
                path: cx.global::<crate::preferences::Preferences>().1.clone(),
                previous: cx.global::<crate::preferences::Preferences>().0.clone(),
                preferences: self.preferences.clone(),
            },
            false,
            cx,
        );
    }
    pub fn complete(
        &mut self,
        completion: Completion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.open
            || completion.scope
                != (Scope {
                    surface: Surface::Settings,
                    generation: self.generation,
                })
        {
            return;
        }
        self.pending = false;
        self.load_failed = self.loading && self.pending_read && completion.result.is_err();
        self.loading = false;
        match completion.result {
            Err(e) => {
                if self.submitted_preferences {
                    self.preferences = cx.global::<crate::preferences::Preferences>().0.clone();
                    self.restore_clipboard_field(window, cx);
                }
                self.error = e.message;
                self.refresh_after = false;
            }
            Ok(Outcome::Preferences(preferences)) => {
                self.preferences = preferences.clone();
                self.restore_clipboard_field(window, cx);
                let path = cx.global::<crate::preferences::Preferences>().1.clone();
                cx.set_global(crate::preferences::Preferences(preferences, path));
                if self.tab == 0 && self.detection.is_none() {
                    self.request(Operation::DetectImport, false, cx);
                }
            }
            Ok(Outcome::PreferencesSaved { preferences, error }) => {
                self.preferences = preferences.clone();
                self.restore_clipboard_field(window, cx);
                let path = cx.global::<crate::preferences::Preferences>().1.clone();
                cx.set_global(crate::preferences::Preferences(preferences, path));
                if let Some(error) = error {
                    self.error = error.message;
                } else {
                    self.notice = self.locale.text_in(
                        self.preferences
                            .resolved_locale
                            .unwrap_or(self.locale.language()),
                        "providerRuntime.saved",
                    );
                }
            }
            Ok(Outcome::Detection(value)) => self.detection = Some(value),
            Ok(Outcome::BrowserExtension(status)) => self.extension = Some(*status),
            Ok(Outcome::Sync {
                settings,
                conflicts,
                devices,
            }) => {
                if !self.sync_loaded {
                    self.sync_mode = settings.mode;
                }
                self.sync_loaded = true;
                self.sync_settings = settings;
                self.conflicts = conflicts;
                self.devices = devices;
            }
            Ok(Outcome::SyncSaved { settings, error }) => {
                self.sync_loaded = true;
                self.sync_mode = settings.mode;
                self.sync_settings = settings;
                self.fields.reset("webdav-password", window, cx);
                self.notice = self.locale.text("settings.syncSettingsSaved");
                if let Some(error) = error {
                    self.error = error.message;
                    self.refresh_after = false;
                }
            }
            Ok(Outcome::Server { config, panel }) => {
                self.config = Some(*config);
                self.panel = Some(panel);
            }
            Ok(Outcome::AccessCode(code)) => {
                if let Some(panel) = &mut self.panel {
                    panel.has_access_code = true;
                    panel.remote_unlock_enabled = self.allow_remote_unlock;
                }
                self.show_access_code(code.access_code, cx);
            }
            Ok(Outcome::Panel(panel)) => {
                self.panel = Some(panel);
                self.certificate_file = None;
                self.private_key_file = None;
                self.refresh_after = false;
                self.notice = self.locale.text("providerRuntime.saved");
            }
            Ok(Outcome::Applied) => {
                self.notice = self.locale.text("providerRuntime.saved");
            }
            Ok(_) => {}
        }
        if self.refresh_after {
            self.refresh_after = false;
            let notice = self.notice.clone();
            self.load(cx);
            self.notice = notice;
        }
        cx.notify();
    }
    fn restore_clipboard_field(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.fields.set_value(
            "clipboard-seconds",
            self.preferences.clipboard_clear_seconds.to_string(),
            window,
            cx,
        );
    }
    fn field(
        &mut self,
        key: &str,
        label: &str,
        value: &str,
        secret: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let field = self.fields.field(
            (key, label, value, secret),
            self.theme,
            &self.locale,
            window,
            cx,
        );
        let placeholder = match key {
            "sync-folder" => "~/Sync/AIPass".into(),
            "webdav-url" => "https://cloud.example/dav".into(),
            "webdav-user" => self.locale.text("settings.usernamePlaceholder"),
            "export-path" | "import-path" => "~/Backups/aipass.aipexport".into(),
            _ if secret => "••••••••".into(),
            _ => String::new(),
        };
        self.fields
            .configure(key, &placeholder, self.pending, window, cx);
        field
    }
    fn button(&self, id: &str, label: &str, primary: bool) -> gpui_base::Button {
        crate::providers::controls::button(
            id.to_owned(),
            self.locale.text(label),
            primary,
            self.theme,
        )
        .disabled(self.pending)
    }
}
