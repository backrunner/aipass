use crate::{
    backdrop::Backdrop,
    locale::Locale,
    theme::{columns, Theme},
};
use gpui::{
    div, prelude::*, px, rgb, rgba, svg, BoxShadow, Context, Div, Entity, EventEmitter, FontWeight,
    Subscription, Window,
};
use gpui_base::{
    input::{InputAccessibility, InputEvent, InputState},
    Button,
};

mod chrome;
use crate::menu;
mod notes;
mod provider_detail;
mod provider_list;
mod provider_menu;
mod providers;
mod tools;
pub use providers::ProviderFeedback;
#[cfg(all(test, feature = "parity-render"))]
mod tests;

pub enum WorkspaceEvent {
    Connect,
    Settings,
    Refresh,
    Provider,
    Credential,
    ProviderTools,
    Integration,
}
impl EventEmitter<WorkspaceEvent> for Workspace {}

pub struct Workspace {
    theme: Theme,
    locale: Locale,
    selected: usize,
    tag_filter: Option<String>,
    search: Entity<InputState>,
    menu_open: bool,
    filter_open: bool,
    menu_focus: menu::MenuFocus,
    provider_menu_focus: menu::MenuFocus,
    filter_focus: menu::MenuFocus,
    nav_focus: [gpui::FocusHandle; 11],
    window_controls_focus: [gpui::FocusHandle; 3],
    refresh_focus: gpui::FocusHandle,
    backdrop: Backdrop,
    dark: bool,
    chinese: bool,
    catalog: aipass_desktop_native::catalog::Catalog,
    selected_provider: Option<uuid::Uuid>,
    hovered_provider: Option<uuid::Uuid>,
    provider_busy: bool,
    provider_menu_open: bool,
    provider_advanced: bool,
    provider_disclosure: crate::disclosure::Disclosure,
    detail_scroll: gpui::ScrollHandle,
    provider_action: Option<aipass_desktop_native::catalog::Action>,
    provider_form: Option<Entity<crate::providers::Form>>,
    form_subscription: Option<Subscription>,
    tools: Entity<crate::provider_tools_ui::Tools>,
    notes: Entity<notes::Notes>,
    _tools_subscription: Subscription,
    _tools_observer: Subscription,
    credentials: Entity<crate::providers::credentials::Credentials>,
    integrations: Entity<crate::integrations::Integrations>,
    pub(crate) subscriptions: Entity<crate::subscription_ui::Subscriptions>,
    _subscriptions_observer: Subscription,
    _integrations_subscription: Subscription,
    _credentials_subscription: Subscription,
    _search_subscription: Subscription,
}

const NAV: [&str; 11] = [
    "sidebar.allItems",
    "sidebar.favorites",
    "sidebar.recent",
    "sidebar.official",
    "sidebar.thirdParty",
    "sidebar.selfHosted",
    "sidebar.custom",
    "agents.title",
    "sidebar.server",
    "sidebar.archive",
    "sidebar.trash",
];
// Synthetic workspace uses the same fixed filter choices as ProviderListPane.
// Tag filters require real entries and remain part of the unported data path.
const FILTERS: [(usize, &str); 10] = [
    (0, "providerList.allItems"),
    (2, "sidebar.recent"),
    (3, "sidebar.official"),
    (4, "sidebar.thirdParty"),
    (5, "sidebar.selfHosted"),
    (6, "sidebar.custom"),
    (11, "providerList.lowQuota"),
    (12, "providerList.expiringSoon"),
    (13, "providerList.oauth"),
    (14, "providerList.api"),
];
const ICONS: [&str; 11] = [
    "inbox",
    "star",
    "sparkles",
    "shield-check",
    "wifi",
    "terminal",
    "sparkles",
    "bot",
    "server",
    "archive",
    "trash-2",
];

impl Workspace {
    pub fn preview_state(&mut self, scenario: &str) {
        self.menu_open = scenario == "menu";
        self.filter_open = scenario == "filter";
    }
    #[cfg(feature = "parity-render")]
    pub fn capture_ready(&self) -> Result<bool, String> {
        self.backdrop.ready()
    }
    pub fn new(dark: bool, chinese: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        menu::init(cx);
        let locale = Locale::new(chinese);
        let search = cx.new(|cx| {
            let mut input = InputState::new(window, cx);
            input.set_editor_style(Theme::new(dark).input_style());
            input.set_accessibility(
                InputAccessibility::new(Some(gpui::Role::TextInput))
                    .with_label(Some(locale.text("providerList.search").into())),
            );
            input
        });
        let search_subscription =
            cx.subscribe_in(&search, window, |this: &mut Self, _, event, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.select_visible(window, cx);
                    cx.notify();
                }
            });
        let credentials =
            cx.new(|cx| crate::providers::credentials::Credentials::new(dark, chinese, window, cx));
        let credentials_subscription = cx.subscribe(&credentials, |this, view, event, cx| {
            if matches!(event, crate::providers::credentials::Event::Request)
                && view.read(cx).has_request()
            {
                this.provider_busy = true;
                cx.emit(WorkspaceEvent::Credential);
            }
            cx.notify();
        });
        let integrations = cx.new(|cx| crate::integrations::Integrations::new(dark, chinese, cx));
        let integrations_subscription = cx.subscribe(&integrations, |this, view, _, cx| {
            if view.read(cx).has_request() {
                this.provider_busy = true;
                cx.emit(WorkspaceEvent::Integration);
            }
            cx.notify();
        });
        let tools = cx.new(|cx| crate::provider_tools_ui::Tools::new(dark, chinese, window, cx));
        let tools_observer = cx.observe(&tools, |_, _, cx| cx.notify());
        let tools_subscription = cx.subscribe(&tools, |this, tools, _, cx| {
            if tools.read(cx).has_request() {
                this.provider_busy = true;
                cx.emit(WorkspaceEvent::ProviderTools);
            }
            cx.notify();
        });
        let notes = cx.new(|cx| notes::Notes::new(dark, chinese, window, cx));
        let subscriptions =
            cx.new(|cx| crate::subscription_ui::Subscriptions::new(dark, chinese, window, cx));
        let subscriptions_observer = cx.observe(&subscriptions, |_, _, cx| cx.notify());
        Self {
            theme: Theme::new(dark),
            locale,
            selected: 0,
            tag_filter: None,
            search,
            menu_open: false,
            filter_open: false,
            menu_focus: menu::MenuFocus::new(3, cx),
            filter_focus: menu::MenuFocus::new(FILTERS.len(), cx),
            provider_menu_focus: menu::MenuFocus::new(4, cx),
            nav_focus: std::array::from_fn(|_| cx.focus_handle()),
            window_controls_focus: std::array::from_fn(|_| cx.focus_handle()),
            refresh_focus: cx.focus_handle(),
            backdrop: Backdrop::new(dark, false),
            dark,
            chinese,
            catalog: Default::default(),
            selected_provider: None,
            provider_busy: false,
            hovered_provider: None,
            provider_menu_open: false,
            provider_advanced: false,
            provider_disclosure: Default::default(),
            detail_scroll: gpui::ScrollHandle::new(),
            provider_action: None,
            provider_form: None,
            form_subscription: None,
            tools,
            notes,
            _tools_subscription: tools_subscription,
            _tools_observer: tools_observer,
            credentials,
            integrations,
            subscriptions,
            _subscriptions_observer: subscriptions_observer,
            _integrations_subscription: integrations_subscription,
            _credentials_subscription: credentials_subscription,
            _search_subscription: search_subscription,
        }
    }

    fn pane(&self, tint: gpui::Rgba) -> Div {
        div()
            .relative()
            .h_full()
            .min_w_0()
            .overflow_hidden()
            .rounded(px(14.))
            .border_1()
            .border_color(self.theme.border.opacity(0.6))
            .when(tint.a > 0., |el| el.child(self.backdrop.glass(tint, 14.)))
            .shadow(vec![BoxShadow::new(
                px(0.),
                px(1.),
                self.theme.surface.opacity(0.6).into(),
            )
            .inset()])
    }

    fn row(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let active = self.selected == index;
        Button::new(("nav", index))
            .track_focus(&self.nav_focus[index])
            .accessibility_label(self.locale.text(NAV[index]))
            .flex()
            .items_center()
            .gap(px(12.))
            .w_full()
            .h(px(32.))
            .px(px(12.))
            .rounded(px(8.))
            .text_size(px(13.))
            .line_height(px(16.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(if active { t.accent } else { t.secondary })
            .when(active, |el| el.bg(t.accent_soft))
            .when(!active, |el| {
                el.hover(move |style| style.bg(t.surface2).text_color(t.text))
            })
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| {
                if this.provider_busy {
                    return;
                }
                this.close_provider_form(window, cx);
                this.provider_menu_open = false;
                this.provider_advanced = false;
                this.selected = index;
                this.tag_filter = None;
                this.select_visible(window, cx);
                cx.notify();
            }))
            .child(
                svg()
                    .path(ICONS[index])
                    .size(px(16.))
                    .text_color(if active { t.accent } else { t.secondary }),
            )
            .child(div().flex_1().child(self.locale.text(NAV[index])))
            .when(index < 7 || index == 10, |el| {
                el.child(
                    div()
                        .text_size(px(11.))
                        .text_color(if active { t.accent } else { t.tertiary })
                        .child(self.catalog.entries(index, "").len().to_string()),
                )
            })
    }

    fn sidebar(&self, width: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        self.pane(t.sidebar.opacity(0.88))
            .w(px(width))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .pt(px(42.))
            .px(px(10.))
            .pb(px(14.))
            .gap(px(18.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children((0..3).map(|i| self.row(i, cx))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .px(px(12.))
                            .text_size(px(11.))
                            .line_height(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.tertiary)
                            .child(self.locale.text("sidebar.providers").to_uppercase()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .children((3..7).map(|i| self.row(i, cx))),
                    ),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .pt(px(12.))
                    .border_t_1()
                    .border_color(t.divider)
                    .children((7..11).map(|i| self.row(i, cx))),
            )
    }

    fn empty(
        &self,
        title: &str,
        description: &str,
        detail: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let t = self.theme;
        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(14.))
            .child(
                div()
                    .size(px(40.))
                    .rounded_full()
                    .bg(t.surface2)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(t.tertiary)
                    .child(
                        svg()
                            .path(if !detail && self.selected == 1 {
                                "star"
                            } else if !detail && self.selected == 10 {
                                "trash-2"
                            } else {
                                "key-round"
                            })
                            .size(px(22.))
                            .text_color(t.tertiary),
                    ),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.locale.text(title)),
            )
            .child(
                div()
                    .absolute()
                    .top(gpui::relative(0.5))
                    .mt(px(47.))
                    .px(px(16.))
                    .w_full()
                    .text_center()
                    .text_size(px(12.))
                    .line_height(px(16.8))
                    .text_color(t.tertiary)
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(div().max_w(px(240.)).child(self.locale.text(description)))
                    .when(
                        !detail
                            && !matches!(self.selected, 1 | 9 | 10)
                            && self.search.read(cx).value().is_empty(),
                        |el| {
                            el.child(
                                div()
                                    .flex()
                                    .justify_center()
                                    .flex_wrap()
                                    .gap(px(8.))
                                    .mt(px(10.))
                                    .child(
                                        self.button(
                                            "empty-add",
                                            "providerList.addProvider",
                                            "plus",
                                            true,
                                            true,
                                        )
                                        .on_click(
                                            cx.listener(|this, _, window, cx| {
                                                this.open_provider_form(None, window, cx)
                                            }),
                                        ),
                                    )
                                    .child(
                                        self.button(
                                            "empty-connect",
                                            "oauthConnect.title",
                                            "plug",
                                            false,
                                            true,
                                        )
                                        .on_click(
                                            cx.listener(|_, _, _, cx| {
                                                cx.emit(WorkspaceEvent::Connect)
                                            }),
                                        ),
                                    ),
                            )
                        },
                    ),
            )
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_credentials(window, cx);
        let (sidebar, list) = columns(f32::from(window.viewport_size().width));
        let t = self.theme;
        let (title, desc) = if !self.search.read(cx).value().trim().is_empty() {
            ("providerList.noMatchingProviders", "")
        } else {
            match self.selected {
                1 => (
                    "providerList.favoritesEmpty",
                    "providerList.favoritesEmptyDesc",
                ),
                9 => ("providerList.archiveEmpty", "providerList.archiveEmptyDesc"),
                10 => ("providerList.trashEmpty", "providerList.trashEmptyDesc"),
                2..=6 => ("providerList.groupEmpty", ""),
                _ => ("providerList.noProviders", "providerList.noProvidersDesc"),
            }
        };
        div()
            .size_full()
            .relative()
            .bg(t.bg)
            .text_color(t.text)
            .font_family(".AppleSystemUIFont")
            .text_size(px(14.))
            .child(self.backdrop.render(window, cx))
            .child(
                div()
                    .relative()
                    .size_full()
                    .on_children_prepainted(crate::parity::record(&[
                        "sidebar",
                        "list-pane",
                        "detail",
                    ]))
                    .p(px(8.))
                    .flex()
                    .gap(px(8.))
                    .child(self.sidebar(sidebar, cx))
                    .child(
                        self.pane(t.surface.opacity(0.86))
                            .w(px(list))
                            .flex_shrink_0()
                            .child(self.toolbar(window, cx))
                            .flex()
                            .flex_col()
                            .child(if self.visible_providers(cx).is_empty() {
                                self.empty(title, desc, false, cx).into_any_element()
                            } else {
                                self.provider_list(window, cx).into_any_element()
                            }),
                    )
                    .child(
                        self.pane(if self.visible_providers(cx).is_empty() {
                            rgba(0x00000000)
                        } else {
                            t.surface.opacity(0.86)
                        })
                        .flex_1()
                        .child(self.provider_detail(window, cx)),
                    ),
            )
            .child(self.titlebar(sidebar, list, window, cx))
            .children(self.integrations.read(cx).modal())
            .children(self.integrations.read(cx).login_modal())
            .child(self.subscriptions.clone())
            .child(self.tools.clone())
            .children(
                self.provider_form
                    .clone()
                    .filter(|_| !self.form_is_editing(cx)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(19.))
                    .top(px(19.))
                    .flex()
                    .gap(px(8.))
                    .children([0xff5f57, 0xfebc2e, 0x28c840].into_iter().enumerate().map(
                        |(index, color)| {
                            Button::new(("window-control", index))
                                .track_focus(&self.window_controls_focus[index])
                                .accessibility_label(self.locale.text(
                                    [
                                        "common.close",
                                        "titlebar.minimize",
                                        "titlebar.toggleMaximize",
                                    ][index],
                                ))
                                .size(px(12.))
                                .rounded_full()
                                .bg(rgb(color))
                                .border_1()
                                .border_color(rgba(0x00000026))
                                .on_click(move |_, window, cx| match index {
                                    0 => window
                                        .dispatch_action(Box::new(crate::desktop::CloseWindow), cx),
                                    1 => window.minimize_window(),
                                    _ => window.zoom_window(),
                                })
                        },
                    )),
            )
    }
}
