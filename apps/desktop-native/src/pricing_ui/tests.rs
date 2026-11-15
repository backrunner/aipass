use super::*;
use aipass_agent::AgentCommandError;
use aipass_desktop_native::catalog::Catalog;
use gpui::{TestAppContext, VisualTestContext};

fn fixture(cx: &mut TestAppContext) -> (Entity<Pricing>, &mut VisualTestContext) {
    cx.update(gpui_base::init);
    let mut view = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let entity = cx.new(|cx| {
            let mut p = Pricing::new(false, false, window, cx);
            let entry = Catalog::fixture().active.remove(0);
            let secret = entry.secret_refs[0].id.clone();
            p.sync(Some(entry), window, cx);
            p.show(&secret, window, cx);
            p
        });
        view = Some(entity.clone());
        gpui_base::Root::new(entity, window, cx)
    });
    (view.unwrap(), cx)
}
fn acknowledge(
    p: &mut Pricing,
    config: PricingConfig,
    window: &mut Window,
    cx: &mut Context<Pricing>,
) {
    let request = p.take_request().unwrap();
    p.complete(
        Completion {
            scope: request.scope,
            result: Ok(config),
        },
        window,
        cx,
    );
}
fn fail(p: &mut Pricing, window: &mut Window, cx: &mut Context<Pricing>) {
    let request = p.take_request().unwrap();
    p.complete(
        Completion {
            scope: request.scope,
            result: Err(AgentCommandError {
                code: None,
                message: "Synthetic failure".into(),
            }),
        },
        window,
        cx,
    );
}

#[gpui::test]
fn failed_load_is_retryable_and_cannot_save_defaults(cx: &mut TestAppContext) {
    let (view, cx) = fixture(cx);
    cx.update(|window, cx| {
        view.update(cx, |p, cx| {
            assert!(p.busy());
            fail(p, window, cx);
            assert!(!p.busy());
            assert!(p.config.is_none());
            assert!(p.assignment(cx).is_none());
            p.submit(Operation::Load, cx);
            acknowledge(p, PricingConfig::default(), window, cx);
            assert!(p.error.is_empty());
            assert_eq!(p.assignment(cx).unwrap().multiplier, 1.);
        })
    });
}

#[gpui::test]
fn group_commit_then_failed_binding_retries_only_the_exact_assignment(cx: &mut TestAppContext) {
    let (view, cx) = fixture(cx);
    cx.update(|window, cx| {
        view.update(cx, |p, cx| {
            acknowledge(p, PricingConfig::default(), window, cx);
            let original = p.original.clone().unwrap();
            p.edit_group(window, cx);
            p.name.update(cx, |s, cx| s.set_value("shared", window, cx));
            p.rules[0].inputs[0].update(cx, |s, cx| s.set_value("claude-", window, cx));
            assert!(p.draft_group(cx).is_none());
            for input in &p.rules[0].inputs[1..5] {
                input.update(cx, |s, cx| s.set_value("0", window, cx));
            }
            p.panel = Some(Panel::Confirm);
            p.save_group(PricingApplyScope::FromNow, cx);
            let request = p.take_request().unwrap();
            p.close(window, cx);
            assert!(!p.closing);
            let Operation::SaveGroup { group, .. } = request.operation else {
                panic!("group write")
            };
            let group_id = group.id;
            let config = PricingConfig {
                groups: vec![group],
                ..Default::default()
            };
            p.complete(
                Completion {
                    scope: request.scope,
                    result: Ok(config.clone()),
                },
                window,
                cx,
            );
            assert!(matches!(p.pending, Some(Operation::Assign(_))));
            fail(p, window, cx);
            assert_eq!(p.config.as_ref().unwrap().groups.len(), 1);
            assert!(p.bind_after_save.is_none());
            let binding = p.retry_binding.clone().unwrap();
            assert_eq!(binding.entry_id, original.entry_id);
            assert_eq!(binding.secret_id, original.secret_id);
            assert_eq!(binding.group_id, Some(group_id));
            p.submit(Operation::Assign(binding), cx);
            assert!(matches!(
                p.request.as_ref().unwrap().operation,
                Operation::Assign(_)
            ));
            acknowledge(p, config, window, cx);
            assert!(p.closing);
            assert!(p.retry_binding.is_none());
        })
    });
}

#[gpui::test]
fn switching_provider_or_lock_discards_read_and_write_completions(cx: &mut TestAppContext) {
    let (view, cx) = fixture(cx);
    for writing in [false, true] {
        cx.update(|window, cx| {
            view.update(cx, |p, cx| {
                p.sync(Some(Catalog::fixture().active.remove(0)), window, cx);
                let secret = p.entry.as_ref().unwrap().secret_refs[0].id.clone();
                p.show(&secret, window, cx);
                if writing {
                    acknowledge(p, PricingConfig::default(), window, cx);
                    p.submit(Operation::Assign(p.assignment(cx).unwrap()), cx);
                }
                let request = p.take_request().unwrap();
                if writing {
                    p.clear(window, cx);
                } else {
                    p.sync(Some(Catalog::fixture().active.remove(1)), window, cx);
                }
                p.complete(
                    Completion {
                        scope: request.scope,
                        result: Ok(PricingConfig::default()),
                    },
                    window,
                    cx,
                );
                assert!(p.panel.is_none());
                assert!(p.config.is_none());
                assert!(p.secret.is_none());
            })
        });
    }
}

#[gpui::test]
fn dirty_assignment_blocks_rule_edit_and_zero_multiplier_is_valid(cx: &mut TestAppContext) {
    let (view, cx) = fixture(cx);
    cx.update(|window, cx| {
        view.update(cx, |p, cx| {
            acknowledge(p, PricingConfig::default(), window, cx);
            p.multiplier
                .update(cx, |s, cx| s.set_value("0", window, cx));
            assert_eq!(p.assignment(cx).unwrap().multiplier, 0.);
            p.edit_group(window, cx);
            assert!(p.panel == Some(Panel::Assignment));
            for invalid in ["", "NaN", "-1"] {
                p.multiplier
                    .update(cx, |s, cx| s.set_value(invalid, window, cx));
                assert!(p.assignment(cx).is_none());
            }
        })
    });
}

#[gpui::test]
fn source_picker_keyboard_stays_in_dialog_and_changes_only_draft(cx: &mut TestAppContext) {
    let (view, cx) = fixture(cx);
    cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(640.)));
    cx.update(|window, cx| {
        view.update(cx, |p, cx| {
            let group = draft::group(
                Uuid::new_v4(),
                "shared",
                &[draft::Rule {
                    model: "claude-".into(),
                    prices: ["0", "0", "0", "0", ""].map(str::to_owned),
                    ..Default::default()
                }],
            )
            .unwrap();
            acknowledge(
                p,
                PricingConfig {
                    groups: vec![group],
                    ..Default::default()
                },
                window,
                cx,
            );
            p.picker_focus.trigger.focus(window, cx);
        })
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        let focus = view.read(cx).picker_focus.trigger.clone();
        focus.focus(window, cx);
    });
    for key in ["enter", "end", "enter"] {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let keystroke = gpui::Keystroke::parse(key).unwrap();
        cx.simulate_event(gpui::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(gpui::KeyUpEvent { keystroke });
    }
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(view.read_with(cx, |p, _| p.selected_group.is_some()
        && !p.picker_open
        && p.request.is_none()));
}

#[gpui::test]
fn multiplier_arrow_keys_step_and_clamp_without_submitting(cx: &mut TestAppContext) {
    use gpui::Focusable;
    let (view, cx) = fixture(cx);
    cx.update(|window, cx| {
        view.update(cx, |p, cx| {
            acknowledge(p, PricingConfig::default(), window, cx)
        })
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        let focus = view.read(cx).multiplier.read(cx).focus_handle(cx);
        focus.focus(window, cx);
    });
    for (key, value) in [("up", 2.), ("down", 1.), ("down", 0.), ("down", 0.)] {
        let keystroke = gpui::Keystroke::parse(key).unwrap();
        cx.simulate_event(gpui::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(gpui::KeyUpEvent { keystroke });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(
            view.read_with(cx, |p, cx| p.assignment(cx).unwrap().multiplier),
            value
        );
        assert!(view.read_with(cx, |p, _| p.request.is_none()));
    }
}
