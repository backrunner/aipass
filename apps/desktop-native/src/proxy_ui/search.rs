use super::*;
use aipass_proxy::{SearchApiConfig, SearchModelConfig, SearchVendor};
use gpui::{div, px};
const VENDORS: [(&str, &str); 9] = [
    ("tavily", "Tavily"),
    ("brave", "Brave"),
    ("exa", "Exa"),
    ("firecrawl", "Firecrawl"),
    ("you", "You.com"),
    ("deepseek", "DeepSeek"),
    ("searxng", "SearXNG"),
    ("bocha", "Bocha"),
    ("yuanbao", "Yuanbao"),
];
impl Proxy {
    pub(super) fn choice(
        &mut self,
        key: String,
        label: &str,
        selected: String,
        options: Vec<(String, String)>,
        change: impl Fn(&mut Self, &str, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> Entity<Choice> {
        if !self.choices.contains_key(&key) {
            let choice = cx.new(|cx| {
                Choice::new(
                    self.locale.text(label),
                    options.clone(),
                    &selected,
                    self.theme,
                    cx,
                )
            });
            self.choice_subscriptions
                .push(cx.subscribe(&choice, move |s, _, e: &Selected, cx| change(s, &e.0, cx)));
            self.choices.insert(key.clone(), choice);
        }
        let choice = self.choices[&key].clone();
        choice.update(cx, |v, cx| {
            v.theme = self.theme;
            v.disabled = self.busy();
            v.selected = selected;
            v.label = self.locale.text(label);
            v.set_options(options, cx);
        });
        choice
    }
    pub(super) fn search_form(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let t = self.theme;
        let mut options = vec![(String::new(), self.locale.text("search.noModel"))];
        if let Some(c) = &self.config {
            options.extend(
                c.routes
                    .iter()
                    .filter(|r| r.enabled)
                    .map(|r| (r.id.to_string(), r.name.clone())),
            );
        }
        let selected = self
            .search_config
            .model
            .as_ref()
            .map(|m| m.route_id.to_string())
            .unwrap_or_default();
        let route = self.choice(
            "search-model-route".into(),
            "search.modelRoute",
            selected,
            options,
            |s, v, cx| {
                s.search_config.model = Uuid::parse_str(v).ok().map(|route_id| SearchModelConfig {
                    route_id,
                    model: s.fields.value("search-model", cx),
                });
                cx.notify();
            },
            cx,
        );
        let model = self
            .search_config
            .model
            .as_ref()
            .map(|m| m.model.clone())
            .unwrap_or_default();
        let mut body = admin_ui::stack()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .child(div().flex_1().min_w_0().child(route))
                    .child(div().flex_1().min_w_0().child(self.field(
                        "search-model",
                        "search.model",
                        &model,
                        false,
                        window,
                        cx,
                    ))),
            )
            .child(
                admin_ui::hint(self.locale.text("search.modelHint"), t)
                    .line_height(px(14.4))
                    .text_color(t.secondary),
            )
            .child(
                admin_ui::row()
                    .min_h(px(26.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.locale.text("search.apis")),
                    )
                    .child(
                        crate::providers::controls::base_button("search-add", false, t)
                            .gap(px(6.))
                            .h(px(26.))
                            .text_size(px(12.))
                            .px(px(9.))
                            .accessibility_label(self.locale.text("search.add"))
                            .child(gpui::svg().path("plus").size(px(14.)).text_color(t.text))
                            .child(self.locale.text("search.add"))
                            .disabled(self.busy() || self.search_config.apis.len() >= 32)
                            .on_click(cx.listener(|s, _, _, cx| {
                                s.search_config.apis.push(SearchApiConfig {
                                    id: Uuid::new_v4(),
                                    vendor: SearchVendor::Tavily,
                                    provider_entry_id: None,
                                    secret_id: None,
                                    base_url: None,
                                    enabled: true,
                                });
                                cx.notify();
                            })),
                    ),
            );
        self.fields.configure(
            "search-model",
            "claude-sonnet-5-5",
            self.busy() || self.search_config.model.is_none(),
            window,
            cx,
        );
        if self.search_config.apis.is_empty() {
            body = body.child(
                admin_ui::hint(self.locale.text("search.empty"), t)
                    .line_height(px(14.4))
                    .text_color(t.secondary),
            );
        }
        for (i, api) in self.search_config.apis.clone().into_iter().enumerate() {
            let id = api.id;
            let vendor = self.choice(
                format!("search-vendor-{id}"),
                "search.vendor",
                serde_json::to_value(api.vendor)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .into(),
                VENDORS
                    .iter()
                    .map(|(id, label)| {
                        (
                            id.to_string(),
                            if *id == "yuanbao" {
                                self.locale.text("search.yuanbao")
                            } else {
                                label.to_string()
                            },
                        )
                    })
                    .collect(),
                move |s, value, cx| {
                    if let Some(api) = s.search_config.apis.iter_mut().find(|a| a.id == id) {
                        if let Ok(v) =
                            serde_json::from_value(serde_json::Value::String(value.into()))
                        {
                            api.vendor = v;
                            api.provider_entry_id = None;
                            api.secret_id = None;
                            api.base_url = None;
                            s.fields.remove(&format!("search-url-{id}"));
                        }
                    }
                    cx.notify();
                },
                cx,
            );
            let mut credentials = vec![];
            if api.vendor == SearchVendor::Searxng {
                credentials.push((String::new(), self.locale.text("search.noKey")));
            }
            credentials.extend(
                self.entries
                    .iter()
                    .filter(|e| {
                        e.credential_kind == aipass_provider_registry::CredentialKind::Api
                            && e.deleted_at.is_none()
                            && e.archived_at.is_none()
                    })
                    .flat_map(|e| {
                        e.secret_refs.iter().map(|secret| {
                            (
                                format!("{}/{}", e.id, secret.id),
                                format!("{} / {}", e.title, secret.label),
                            )
                        })
                    }),
            );
            let selected = api
                .provider_entry_id
                .map(|v| format!("{}/{}", v, api.secret_id.as_deref().unwrap_or_default()))
                .unwrap_or_default();
            let credential = self.choice(
                format!("search-key-{id}"),
                "search.credential",
                selected,
                credentials,
                move |s, value, cx| {
                    if let Some(api) = s.search_config.apis.iter_mut().find(|a| a.id == id) {
                        let pair = value.split_once('/');
                        api.provider_entry_id = pair.and_then(|(v, _)| Uuid::parse_str(v).ok());
                        api.secret_id = pair.map(|(_, v)| v.to_string());
                    }
                    cx.notify();
                },
                cx,
            );
            vendor.update(cx, |v, _| v.inline = true);
            credential.update(cx, |v, _| {
                v.inline = true;
                v.placeholder = self.locale.text("search.credential");
            });
            let endpoint_key = format!("search-url-{id}");
            let endpoint = self.fields.editor(
                (
                    &endpoint_key,
                    "search.endpoint",
                    api.base_url.as_deref().unwrap_or_default(),
                    false,
                ),
                t,
                &self.locale,
                window,
                cx,
            );
            self.fields
                .configure(&endpoint_key, api.vendor.base(), self.busy(), window, cx);
            let owner = cx.entity().downgrade();
            let row = admin_ui::stack()
                .gap(px(8.))
                .p(px(10.))
                .border_1()
                .border_color(t.border)
                .rounded(px(8.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .gap(px(10.))
                                .child(div().flex_1().min_w_0().child(vendor))
                                .child(div().flex_1().min_w_0().child(credential)),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(
                                    crate::form_ui::check(
                                        "search-enabled",
                                        String::new(),
                                        api.enabled,
                                        t,
                                    )
                                    .accessibility_label(self.locale.text("search.enabled"))
                                    .disabled(self.busy())
                                    .on_change(
                                        move |checked, _, _, cx| {
                                            let _ = owner.update(cx, |s, cx| {
                                                if let Some(a) = s
                                                    .search_config
                                                    .apis
                                                    .iter_mut()
                                                    .find(|a| a.id == id)
                                                {
                                                    a.enabled = checked
                                                        == gpui_base::CheckboxState::Checked;
                                                }
                                                cx.notify();
                                            });
                                        },
                                    ),
                                )
                                .child(
                                    crate::providers::controls::icon(
                                        format!("search-up-{id}"),
                                        "arrow-up",
                                        self.locale.text("search.moveUp"),
                                        t,
                                    )
                                    .size(px(28.))
                                    .disabled(self.busy() || i == 0)
                                    .on_click(cx.listener(
                                        move |s, _, _, cx| {
                                            s.search_config.apis.swap(i, i - 1);
                                            cx.notify();
                                        },
                                    )),
                                )
                                .child(
                                    crate::providers::controls::icon(
                                        format!("search-down-{id}"),
                                        "arrow-down",
                                        self.locale.text("search.moveDown"),
                                        t,
                                    )
                                    .size(px(28.))
                                    .disabled(self.busy() || i + 1 == self.search_config.apis.len())
                                    .on_click(cx.listener(
                                        move |s, _, _, cx| {
                                            s.search_config.apis.swap(i, i + 1);
                                            cx.notify();
                                        },
                                    )),
                                )
                                .child(
                                    crate::providers::controls::icon(
                                        format!("search-remove-{id}"),
                                        "trash-2",
                                        self.locale.text("common.remove"),
                                        t,
                                    )
                                    .size(px(28.))
                                    .disabled(self.busy())
                                    .on_click(cx.listener(
                                        move |s, _, _, cx| {
                                            s.search_config.apis.retain(|a| a.id != id);
                                            s.fields.remove(&format!("search-url-{id}"));
                                            for key in [
                                                format!("search-vendor-{id}"),
                                                format!("search-key-{id}"),
                                            ] {
                                                s.choices.remove(&key);
                                            }
                                            cx.notify();
                                        },
                                    )),
                                ),
                        ),
                )
                .child(endpoint);
            body = body.child(div().id(format!("search-api-{id}")).child(row));
        }
        body = body.child(
            self.toggle(
                "search.apiFirst",
                self.search_config.api_first,
                |s, v, cx| {
                    s.search_config.api_first = v;
                    cx.notify();
                },
                window,
                cx,
            )
            .min_h(px(36.)),
        );
        if let Some(c) = &self.config {
            let endpoint = format!(
                "http://{}/mcp/aipass/web-search",
                super::view::advertised(&c.bind_addr)
            );
            body = body.child(
                admin_ui::row()
                    .border_t_1()
                    .border_color(t.divider)
                    .pt(px(12.))
                    .child(
                        admin_ui::stack()
                            .gap(px(5.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child("MCP"),
                            )
                            .child(
                                div()
                                    .font_family(".AppleSystemUIFontMonospaced")
                                    .text_size(px(12.))
                                    .child(endpoint.clone()),
                            )
                            .child(
                                admin_ui::hint(self.locale.text("search.mcpAuth"), t)
                                    .line_height(px(14.4))
                                    .text_color(t.secondary),
                            ),
                    )
                    .child(
                        crate::providers::controls::icon(
                            "search-copy",
                            "copy",
                            self.locale.text("common.copy"),
                            t,
                        )
                        .size(px(28.))
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(endpoint.clone()))
                        }),
                    ),
            );
        }
        body.into_any_element()
    }
    pub(super) fn save_search(&mut self, cx: &mut Context<Self>) {
        let Some(mut config) = self.config.clone() else {
            return;
        };
        let mut search = self.search_config.clone();
        if let Some(model) = &mut search.model {
            model.model = self.fields.value("search-model", cx).trim().into();
        }
        for api in &mut search.apis {
            let value = self.fields.value(&format!("search-url-{}", api.id), cx);
            api.base_url = (!value.trim().is_empty()).then(|| value.trim().into());
        }
        if search.validate().is_err() {
            self.error = self.locale.text("search.incomplete");
            cx.notify();
            return;
        }
        config.search = search;
        self.apply(AgentRequest::ServerConfigSet { config }, cx);
    }
}
