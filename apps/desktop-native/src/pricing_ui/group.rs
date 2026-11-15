use super::*;
use crate::{parity::Observe, providers::controls};
use gpui::{div, px, svg, AnyElement, FontWeight};
use gpui_base::{Button, Checkbox, CheckboxState};

impl Pricing {
    pub(super) fn group_body(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme;
        if self.panel != Some(Panel::Group) {
            let existing = self.current_group().is_some_and(|g| !g.versions.is_empty());
            return div()
                .flex()
                .flex_col()
                .gap(px(if self.panel == Some(Panel::Confirm) {
                    8.
                } else {
                    22.
                }))
                .when(self.panel == Some(Panel::Confirm), |el| {
                    el.child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(17.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.locale.text(if existing {
                                "pricing.confirmUpdateTitle"
                            } else {
                                "pricing.confirmFirstTitle"
                            })),
                    )
                })
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(if self.panel == Some(Panel::Confirm) {
                            18.
                        } else {
                            17.
                        }))
                        .text_color(t.tertiary)
                        .child(self.locale.text(match self.panel {
                            Some(Panel::DeleteGroup) => "pricing.deleteGroupConfirm",
                            Some(Panel::DeleteVersion(_)) => "pricing.deleteVersionConfirm",
                            _ if existing => "pricing.confirmUpdateDesc",
                            _ => "pricing.confirmFirstDesc",
                        })),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(if self.panel == Some(Panel::Confirm) {
                            18.
                        } else {
                            17.
                        }))
                        .text_color(t.tertiary)
                        .child(
                            self.locale
                                .text("pricing.sharedImpact")
                                .replace("{count}", &self.assigned_count().to_string()),
                        ),
                )
                .observe("pricing-confirmation")
                .into_any_element();
        }
        div()
            .flex()
            .flex_col()
            .gap(px(22.))
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex_1()
                            .max_w(px(340.))
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                self.label("pricing.group")
                                    .text_size(px(11.))
                                    .line_height(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(t.tertiary),
                            )
                            .child(self.field(&self.name, "pricing-group-name", window, cx)),
                    )
                    .child(
                        div()
                            .mb(px(6.))
                            .px(px(8.))
                            .py(px(4.))
                            .rounded_full()
                            .border_1()
                            .border_color(t.border)
                            .text_size(px(10.))
                            .line_height(px(12.))
                            .text_color(t.secondary)
                            .bg(t.surface2)
                            .child(
                                self.locale
                                    .text("pricing.assignedKeys")
                                    .replace("{count}", &self.assigned_count().to_string()),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .line_height(px(15.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(self.locale.text("pricing.modelRules")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(10.))
                                            .line_height(px(14.))
                                            .text_color(t.tertiary)
                                            .px(px(8.))
                                            .py(px(4.))
                                            .border_1()
                                            .border_color(t.border)
                                            .rounded_full()
                                            .child(self.locale.text("pricing.priceUnit")),
                                    ),
                            )
                            .child(
                                controls::base_button("pricing-add-rule", false, t)
                                    .accessibility_label(self.locale.text("pricing.addRule"))
                                    .disabled(self.busy())
                                    .h(px(26.))
                                    .px(px(10.))
                                    .text_size(px(12.))
                                    .gap(px(6.))
                                    .child(svg().path("plus").size(px(13.)).text_color(t.text))
                                    .child(self.locale.text("pricing.addRule"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.add_rule(Default::default(), window, cx)
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .border_1()
                            .border_color(t.border)
                            .rounded(px(10.))
                            .overflow_hidden()
                            .child(
                                div()
                                    .flex()
                                    .gap(px(8.))
                                    .px(px(12.))
                                    .py(px(10.))
                                    .bg(t.surface2)
                                    .text_size(px(11.))
                                    .line_height(px(13.))
                                    .text_color(t.tertiary)
                                    .children(fields::RULE_LABELS[..6].iter().enumerate().map(
                                        |(i, key)| {
                                            div()
                                                .flex_basis(px(0.))
                                                .flex_grow(if i == 0 { 1.6 } else { 1. })
                                                .min_w(px(if i == 0 { 150. } else { 68. }))
                                                .child(self.locale.text(key))
                                        },
                                    ))
                                    .child(div().w(px(28.)).flex_shrink_0())
                                    .observe("pricing-rule-columns"),
                            )
                            .children(
                                self.rules
                                    .iter()
                                    .map(|rule| self.rule_card(rule, window, cx)),
                            ),
                    )
                    .child(self.hint("info", self.locale.text("pricing.rulesHelp"))),
            )
            .when(
                self.current_group().is_some_and(|g| !g.versions.is_empty()),
                |el| el.child(self.history(window, cx)),
            )
            .observe("pricing-group-content")
            .into_any_element()
    }
    fn rule_card(
        &self,
        rule: &RuleInputs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.theme;
        let id = rule.id;

        div()
            .p(px(12.))
            .border_t_1()
            .border_color(t.divider)
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .items_end()
                    .children(rule.inputs[..6].iter().enumerate().map(|(i, input)| {
                        div()
                            .flex_basis(px(0.))
                            .flex_grow(if i == 0 { 1.6 } else { 1. })
                            .min_w(px(if i == 0 { 150. } else { 68. }))
                            .when(i == 0, |el| el.font_family("Menlo"))
                            .child(if i == 0 {
                                self.field(input, format!("pricing-rule-{id}-{i}"), window, cx)
                            } else {
                                self.number(
                                    input,
                                    format!("pricing-rule-{id}-{i}"),
                                    fields::RULE_LABELS[i],
                                    false,
                                    window,
                                    cx,
                                )
                            })
                    }))
                    .child(
                        controls::icon(
                            ("pricing-remove-rule", id),
                            "trash-2",
                            self.locale.text("common.remove"),
                            t,
                        )
                        .size(px(28.))
                        .mb(px(3.))
                        .disabled(self.busy())
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.rules.retain(|r| r.id != id);
                                if this.rules.is_empty() {
                                    this.add_rule(Default::default(), window, cx);
                                }
                                cx.notify();
                            },
                        )),
                    ),
            )
            .child(
                Checkbox::new(("pricing-off-peak", id))
                    .h(px(18.))
                    .checked(rule.off_peak)
                    .disabled(self.busy())
                    .accessibility_label(self.locale.text("pricing.offPeak"))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .line_height(px(15.))
                    .font_weight(FontWeight::NORMAL)
                    .text_color(t.secondary)
                    .child(
                        div()
                            .size(px(12.))
                            .mx(px(2.))
                            .rounded(px(3.))
                            .border(px(1.5))
                            .border_color(if rule.off_peak {
                                t.accent
                            } else {
                                gpui::rgb(0x808080)
                            })
                            .bg(if rule.off_peak { t.accent } else { t.surface })
                            .when(rule.off_peak, |el| {
                                el.child(
                                    svg()
                                        .path("checkbox-check")
                                        .size(px(12.))
                                        .text_color(gpui::rgb(0xffffff)),
                                )
                            }),
                    )
                    .child(self.locale.text("pricing.offPeak"))
                    .on_change({
                        let owner = cx.entity().downgrade();
                        move |state, _, _, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                if let Some(rule) = this.rules.iter_mut().find(|r| r.id == id) {
                                    rule.off_peak = state == CheckboxState::Checked;
                                }
                                cx.notify();
                            });
                        }
                    }),
            )
            .when(rule.off_peak, |el| {
                el.child(
                    div()
                        .bg(t.surface2)
                        .rounded(px(6.))
                        .p(px(12.))
                        .flex()
                        .gap(px(8.))
                        .children(rule.inputs[6..].iter().enumerate().map(|(i, input)| {
                            div()
                                .min_w_0()
                                .when(i < 2, |el| el.w(px(104.)).flex_shrink_0())
                                .when(i >= 2, |el| el.flex_1())
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    self.label(fields::RULE_LABELS[i + 6])
                                        .text_size(px(11.))
                                        .line_height(px(13.))
                                        .font_weight(FontWeight::SEMIBOLD),
                                )
                                .child(if i < 2 {
                                    self.field(
                                        input,
                                        format!("pricing-rule-{id}-{}", i + 6),
                                        window,
                                        cx,
                                    )
                                } else {
                                    self.number(
                                        input,
                                        format!("pricing-rule-{id}-{}", i + 6),
                                        fields::RULE_LABELS[i + 6],
                                        false,
                                        window,
                                        cx,
                                    )
                                })
                        })),
                )
            })
            .observe(format!("pricing-rule-{id}"))
            .into_any_element()
    }
    fn history(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme;
        let frame = self
            .history_disclosure
            .sample(self.history_open, window, cx);
        let mut versions = self
            .current_group()
            .map(|g| g.versions.clone())
            .unwrap_or_default();
        versions.sort_by_key(|v| std::cmp::Reverse(v.effective_from));
        div()
            .border_1()
            .border_color(t.border)
            .rounded(px(8.))
            .overflow_hidden()
            .child(
                Button::new("pricing-history")
                    .h(px(43.))
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(10.))
                    .text_size(px(12.))
                    .line_height(px(16.8))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(t.text)
                    .accessibility_label(self.locale.text("pricing.history"))
                    .aria_expanded(self.history_open)
                    .border_b_1()
                    .border_color(if self.history_open {
                        t.divider
                    } else {
                        gpui::rgba(0)
                    })
                    .justify_start()
                    .child(svg().path("history").size(px(14.)).text_color(t.tertiary))
                    .child(div().flex_1().child(self.locale.text("pricing.history")))
                    .child(
                        div()
                            .px(px(6.))
                            .py(px(3.))
                            .min_w(px(22.))
                            .border_1()
                            .border_color(t.border)
                            .rounded_full()
                            .text_size(px(10.))
                            .line_height(px(12.))
                            .font_weight(FontWeight::NORMAL)
                            .text_color(t.tertiary)
                            .child(versions.len().to_string()),
                    )
                    .child(frame.chevron(t.tertiary, 15.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.history_open = !this.history_open;
                        cx.notify();
                    })),
            )
            .child(
                frame.body(
                    "pricing-history-body",
                    div().p(px(12.)).child(
                        div()
                            .border_1()
                            .border_color(t.border)
                            .rounded(px(6.))
                            .overflow_hidden()
                            .children(versions.iter().enumerate().map(|(i, version)| {
                                let at = version.effective_from;
                                let rules: Vec<_> =
                                    version.rules.iter().map(draft::Rule::from).collect();
                                let date = if at == 0 {
                                    self.locale.text("pricing.allHistory")
                                } else {
                                    aipass_desktop_native::pricing::presentation::local_date(
                                        at,
                                        self.locale.language(),
                                    )
                                };
                                div()
                                    .flex()
                                    .items_center()
                                    .when(i > 0, |el| el.border_t_1().border_color(t.divider))
                                    .child(
                                        Button::new(("pricing-history-load", i))
                                            .flex_1()
                                            .min_w_0()
                                            .px(px(12.))
                                            .py(px(8.))
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .gap(px(12.))
                                            .text_size(px(12.))
                                            .line_height(px(15.))
                                            .text_color(t.text)
                                            .disabled(self.busy())
                                            .accessibility_label(format!(
                                                "{} {date}",
                                                self.locale.text("pricing.effectiveFrom")
                                            ))
                                            .child(div().min_w_0().text_ellipsis().child(date))
                                            .child(
                                                div()
                                                    .text_size(px(11.))
                                                    .text_color(t.tertiary)
                                                    .child(
                                                        self.locale
                                                            .text("pricing.ruleCount")
                                                            .replace(
                                                                "{count}",
                                                                &rules.len().to_string(),
                                                            ),
                                                    ),
                                            )
                                            .hover(move |s| s.bg(t.surface2))
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.set_rules(rules.clone(), window, cx)
                                            })),
                                    )
                                    .child(
                                        controls::icon(
                                            ("pricing-history-delete", i),
                                            "trash-2",
                                            self.locale.text("pricing.deleteVersion"),
                                            t,
                                        )
                                        .size(px(28.))
                                        .mr(px(8.))
                                        .disabled(self.busy())
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.panel = Some(Panel::DeleteVersion(at));
                                                cx.notify();
                                            }),
                                        ),
                                    )
                            })),
                    ),
                ),
            )
            .observe("pricing-history")
            .into_any_element()
    }
    pub(super) fn group_footer(&self, cx: &Context<Self>) -> AnyElement {
        let t = self.theme;
        div()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(8.))
            .when(
                self.panel == Some(Panel::Group) && self.current_group().is_some(),
                |el| {
                    el.child(
                        controls::ghost_button("pricing-delete-group", false, t)
                            .gap(px(6.))
                            .disabled(self.busy())
                            .accessibility_label(self.locale.text("pricing.deleteGroup"))
                            .child(svg().path("trash-2").size(px(14.)).text_color(t.text))
                            .child(self.locale.text("pricing.deleteGroup"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.panel = Some(Panel::DeleteGroup);
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1())
                },
            )
            .child(
                self.button("pricing-group-cancel", "common.cancel", false)
                    .bg(gpui::rgba(0))
                    .border_color(gpui::rgba(0))
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.panel == Some(Panel::Group) {
                            this.close(window, cx);
                        } else {
                            this.panel = Some(Panel::Group);
                            this.error.clear();
                            cx.notify();
                        }
                    })),
            )
            .when(self.panel == Some(Panel::Group), |el| {
                el.child(
                    self.button("pricing-group-save", "common.save", true)
                        .disabled(self.busy() || self.draft_group(cx).is_none())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.panel = Some(Panel::Confirm);
                            cx.notify();
                        })),
                )
            })
            .when(self.panel == Some(Panel::Confirm), |el| {
                el.child(
                    self.button("pricing-from-now", "pricing.fromNow", true)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.save_group(PricingApplyScope::FromNow, cx)
                        })),
                )
                .child(
                    self.button("pricing-all-history", "pricing.allHistory", false)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.save_group(PricingApplyScope::AllHistory, cx)
                        })),
                )
            })
            .when(
                matches!(
                    self.panel,
                    Some(Panel::DeleteGroup | Panel::DeleteVersion(_))
                ),
                |el| {
                    el.child(
                        self.button(
                            "pricing-confirm-delete",
                            if self.panel == Some(Panel::DeleteGroup) {
                                "pricing.deleteGroup"
                            } else {
                                "pricing.deleteVersion"
                            },
                            true,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            let operation = match this.panel {
                                Some(Panel::DeleteGroup) => Operation::DeleteGroup(this.group_id),
                                Some(Panel::DeleteVersion(effective_from)) => {
                                    Operation::DeleteVersion {
                                        group_id: this.group_id,
                                        effective_from,
                                    }
                                }
                                _ => return,
                            };
                            this.submit(operation, cx);
                        })),
                    )
                },
            )
            .into_any_element()
    }
}
