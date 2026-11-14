use super::*;
use crate::parity::Observe;
use gpui::{div, px, relative, AnyElement, FontWeight};
use gpui_base::Button;

impl Credentials {
    pub(super) fn pricing_badge(
        &self,
        secret: &SecretRef,
        index: usize,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let config = self.pricing_config.as_ref()?;
        let entry = self.entry.as_ref()?;
        let a = config
            .assignments
            .iter()
            .find(|a| a.entry_id == entry.id && a.secret_id == secret.id)?;
        if a.group_id.is_none() && a.multiplier == 1. {
            return None;
        }
        let group = a
            .group_id
            .and_then(|id| config.groups.iter().find(|g| g.id == id))
            .map(|g| g.name.clone())
            .unwrap_or_default();
        let t = self.theme;
        let id = secret.id.clone();
        Some(
            Button::new(("credential-pricing-badge", index))
                .min_w_0()
                .max_w(relative(0.48))
                .flex_shrink_0()
                .mr(px(4.))
                .px(px(8.))
                .py(px(3.))
                .rounded_full()
                .border_1()
                .border_color(t.accent.opacity(0.18))
                .bg(t.accent_soft)
                .text_color(t.accent)
                .text_size(px(11.))
                .line_height(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .flex()
                .items_center()
                .gap(px(4.))
                .overflow_hidden()
                .cursor_pointer()
                .disabled(self.busy())
                .accessibility_label(format!(
                    "{}: {group} ×{}",
                    self.locale.text("pricing.credentialSettings"),
                    a.multiplier
                ))
                .when(!group.is_empty(), |el| {
                    el.child(div().min_w_0().text_ellipsis().child(group))
                })
                .when(a.multiplier != 1., |el| {
                    el.child(div().flex_shrink_0().child(format!("×{}", a.multiplier)))
                })
                .hover(move |s| s.border_color(t.accent))
                .focus_visible(move |s| s.shadow(t.outline(t.focus_ring, 2., 2.)))
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(Event::Pricing(id.clone()))))
                .observe(format!("credential-pricing-badge-{index}"))
                .into_any_element(),
        )
    }
}
