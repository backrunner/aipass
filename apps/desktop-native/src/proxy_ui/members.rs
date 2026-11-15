use super::*;
use gpui::{div, px, FontWeight};
#[derive(Clone)]
struct MemberDrag {
    owner: gpui::EntityId,
    id: Uuid,
    title: String,
    theme: Theme,
}
impl Render for MemberDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(12.))
            .py(px(8.))
            .rounded(px(8.))
            .bg(self.theme.surface)
            .text_color(self.theme.text)
            .child(self.title.clone())
    }
}
impl Proxy {
    pub(super) fn members(
        &mut self,
        route: &ProxyRouteConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let t = self.theme;
        let mut body = admin_ui::stack().gap(px(10.));
        let owner_id = cx.entity_id();
        for (index, target) in route.targets.iter().enumerate() {
            let id = target.id;
            let entry = self
                .entries
                .iter()
                .find(|e| {
                    e.id == target.provider_entry_id
                        && e.secret_refs.iter().any(|k| k.id == target.secret_id)
                })
                .cloned();
            if entry.is_none() {
                body = body.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .px(px(12.))
                        .py(px(10.))
                        .rounded(px(8.))
                        .border_1()
                        .border_dashed()
                        .border_color(t.warning.opacity(0.5))
                        .child(div().w(px(18.)))
                        .child(
                            gpui::svg()
                                .path("key-round")
                                .size(px(24.))
                                .text_color(t.tertiary),
                        )
                        .child(
                            admin_ui::stack()
                                .flex_1()
                                .min_w_0()
                                .gap(px(4.))
                                .opacity(0.5)
                                .child(div().truncate().font_weight(FontWeight::SEMIBOLD).child(
                                    if target.label.is_empty() {
                                        target.provider_entry_id.to_string()
                                    } else {
                                        target.label.clone()
                                    },
                                ))
                                .child(
                                    div()
                                        .font_family(".AppleSystemUIFontMonospaced")
                                        .text_size(px(12.))
                                        .child(target.provider_entry_id.to_string()),
                                )
                                .child(admin_ui::hint(self.locale.text("server.memberMissing"), t)),
                        )
                        .child(self.member_action(
                            id,
                            "remove",
                            "trash-2",
                            "server.removeTarget",
                            false,
                            cx,
                        )),
                );
                continue;
            }
            let degraded = target.enabled
                && route.enabled
                && self
                    .status
                    .as_ref()
                    .is_some_and(|s| s.running && s.degraded_target_ids.contains(&id));
            let title = entry
                .as_ref()
                .map(|e| e.title.clone())
                .unwrap_or_else(|| self.locale.text("server.memberMissing"));
            let identity = admin_ui::stack()
                .gap(px(2.))
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .text_size(px(13.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child(title.clone()),
                        )
                        .when_some(entry.as_ref(), |el, e| {
                            el.child(
                                div()
                                    .flex_shrink_0()
                                    .whitespace_nowrap()
                                    .px(px(8.))
                                    .py(px(3.))
                                    .rounded_full()
                                    .border_1()
                                    .border_color(t.border)
                                    .text_size(px(11.))
                                    .line_height(px(11.))
                                    .text_color(t.secondary)
                                    .child(match e.credential_kind {
                                        aipass_provider_registry::CredentialKind::Api => "API",
                                        _ => "OAuth",
                                    }),
                            )
                        }),
                )
                .when(self.bind_models, |el| {
                    el.flex_none().w(px(200.)).min_w(px(140.))
                })
                .when(!target.enabled, |el| el.opacity(0.5))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            admin_ui::hint(target.label.clone(), t)
                                .text_size(px(11.))
                                .line_height(crate::typography::normal_line_height(
                                    window,
                                    &target.label,
                                    11.,
                                )),
                        )
                        .when(!target.enabled, |el| {
                            el.child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(t.tertiary)
                                    .child(self.locale.text("server.memberDisabled")),
                            )
                        })
                        .when(degraded, |el| {
                            el.child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(t.warning)
                                    .child(self.locale.text("server.degraded")),
                            )
                        }),
                );
            let mut model = None;
            if self.bind_models {
                model = Some(
                    self.fields.editor(
                        (
                            &format!("member-model-{id}"),
                            "server.memberModel",
                            target
                                .model
                                .as_deref()
                                .or_else(|| {
                                    entry.as_ref().and_then(|e| {
                                        e.secret_refs
                                            .iter()
                                            .find(|s| s.id == target.secret_id)
                                            .and_then(|s| s.default_model.as_deref())
                                            .or(e.default_model.as_deref())
                                    })
                                })
                                .unwrap_or_default(),
                            false,
                        ),
                        t,
                        &self.locale,
                        window,
                        cx,
                    ),
                );
                self.fields.configure(
                    &format!("member-model-{id}"),
                    &self.locale.text("server.memberModel"),
                    self.busy() || !target.enabled,
                    window,
                    cx,
                );
            }
            let owner = cx.entity().downgrade();
            let enabled = admin_ui::switch(
                format!("member-enabled-{id}"),
                format!("{title}: {}", self.locale.text("server.enabled")),
                target.enabled,
                self.busy(),
                t,
                window,
                cx,
            )
            .on_change(move |enabled, _, _, cx| {
                let _ = owner.update(cx, |s, cx| {
                    if let Some(r) = &mut s.route {
                        if let Some(target) = r.targets.iter_mut().find(|t| t.id == id) {
                            target.enabled = enabled;
                        }
                    }
                    cx.notify();
                });
            });
            let mut controls = div().flex().items_center().gap(px(12.)).flex_shrink_0();
            if route.strategy == aipass_proxy::RouteStrategy::RoundRobin {
                controls = controls.child(div().w(px(56.)).child(self.field(
                    &format!("member-weight-{id}"),
                    "server.weight",
                    &target.weight.to_string(),
                    false,
                    window,
                    cx,
                )));
            }
            controls = controls.child(enabled).child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(self.member_action(
                        id,
                        "up",
                        "chevron-up",
                        "server.moveUp",
                        index == 0,
                        cx,
                    ))
                    .child(self.member_action(
                        id,
                        "down",
                        "chevron-down",
                        "server.moveDown",
                        index + 1 == route.targets.len(),
                        cx,
                    ))
                    .child(self.member_action(
                        id,
                        "remove",
                        "trash-2",
                        "providerDetail.removeKey",
                        false,
                        cx,
                    )),
            );
            body = body.child(
                div()
                    .id(format!("member-{id}"))
                    .flex()
                    .items_center()
                    .gap(px(if self.bind_models { 14. } else { 12. }))
                    .px(px(12.))
                    .py(px(10.))
                    .min_h(px(if self.bind_models { 64. } else { 56. }))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(t.border)
                    .drag_over::<MemberDrag>(move |s, drag, _, _| {
                        if drag.owner == owner_id {
                            s.border_color(t.accent)
                        } else {
                            s
                        }
                    })
                    .on_drop(cx.listener(move |s, drag: &MemberDrag, _, cx| {
                        if drag.owner == cx.entity_id() {
                            s.move_member(drag.id, id, cx);
                        }
                    }))
                    .child(
                        div()
                            .id(format!("member-drag-{id}"))
                            .w(px(20.))
                            .ml(px(-4.))
                            .h(px(28.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor(gpui::CursorStyle::OpenHand)
                            .child(
                                gpui::svg()
                                    .path("grip-vertical")
                                    .size(px(14.))
                                    .text_color(t.tertiary),
                            )
                            .when(!self.busy(), |el| {
                                el.on_drag(
                                    MemberDrag {
                                        owner: owner_id,
                                        id,
                                        title: title.clone(),
                                        theme: t,
                                    },
                                    |drag, _, _, cx| cx.new(|_| drag.clone()),
                                )
                            }),
                    )
                    .when_some(entry, |el, e| {
                        el.child(crate::provider_icon::icon(
                            e.provider_id.as_deref().unwrap_or(""),
                            &e.title,
                            e.credential_kind,
                            &e.provider_kind,
                            32.,
                            self.dark,
                        ))
                    })
                    .child(identity)
                    .when_some(model, |el, model| {
                        el.child(div().flex_1().min_w_0().child(model))
                    })
                    .child(controls),
            );
        }
        body.into_any_element()
    }
    fn member_action(
        &self,
        id: Uuid,
        action: &'static str,
        icon: &'static str,
        label: &str,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> gpui_base::Button {
        crate::providers::controls::icon(
            format!("member-{action}-{id}"),
            icon,
            self.locale.text(label),
            self.theme,
        )
        .size(px(26.))
        .disabled(self.busy() || disabled)
        .on_click(cx.listener(move |s, _, _, cx| {
            if s.busy() {
                return;
            }
            let Some(route) = &mut s.route else {
                return;
            };
            let Some(index) = route.targets.iter().position(|t| t.id == id) else {
                return;
            };
            match action {
                "up" if index > 0 => route.targets.swap(index, index - 1),
                "down" if index + 1 < route.targets.len() => route.targets.swap(index, index + 1),
                "remove" => {
                    route.targets.remove(index);
                    s.fields.remove(&format!("member-model-{id}"));
                    s.fields.remove(&format!("member-weight-{id}"));
                }
                _ => {}
            }
            s.reconcile_picker(cx);
            cx.notify();
        }))
    }
    fn move_member(&mut self, id: Uuid, before: Uuid, cx: &mut Context<Self>) {
        if self.busy() || id == before {
            return;
        }
        let Some(route) = &mut self.route else {
            return;
        };
        let Some(from) = route.targets.iter().position(|t| t.id == id) else {
            return;
        };
        let Some(to) = route.targets.iter().position(|t| t.id == before) else {
            return;
        };
        let item = route.targets.remove(from);
        route.targets.insert(to, item);
        cx.notify();
    }
}
