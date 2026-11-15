use super::*;
use gpui::{px, size, TestAppContext, VisualTestContext};
mod preview;

#[gpui::test]
fn successful_tool_write_feedback_survives_installation_rescan(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            s.preview("proxy-ready", w, cx);
            let scope = Scope {
                surface: Surface::Proxy,
                generation: s.generation,
            };
            s.complete(
                Completion {
                    scope,
                    result: Ok(Outcome::ToolApplied(
                        aipass_agent_protocol::ToolConfigApplyResponse {
                            outcome: aipass_agent_protocol::ToolConfigOutcome::Applied,
                            message: None,
                            tool: aipass_agent_protocol::ToolConfigTool::Codex,
                            mode: aipass_agent_protocol::ToolConfigMode::Plaintext,
                            entry_id: Uuid::from_u128(1),
                            entry_title: "fixture".into(),
                            operation_id: Uuid::from_u128(2),
                            target_path: "/tmp/fixture-config".into(),
                            backup_path: "/tmp/fixture-backup".into(),
                            summary: "Fixture applied".into(),
                        },
                    )),
                },
                w,
                cx,
            );
            assert_eq!(s.notice, "Fixture applied");
            let request = s.take_request().unwrap();
            assert!(matches!(request.operation, Operation::Tools));
            s.complete(
                Completion {
                    scope: request.scope,
                    result: Ok(Outcome::Tools(vec![])),
                },
                w,
                cx,
            );
            assert_eq!(s.notice, "Fixture applied");
        })
    });
}

#[test]
fn integration_endpoint_respects_protocol_and_advertises_loopback() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tests/desktop-parity/fixtures/administration.json"
    ))
    .unwrap();
    let mut config: ProxyConfig = serde_json::from_value(fixture["config"].clone()).unwrap();
    let mut route = config.routes[0].clone();
    for protocol in [
        aipass_proxy::Protocol::OpenAiChatCompletions,
        aipass_proxy::Protocol::OpenAiResponses,
    ] {
        route.inbound_protocol = protocol;
        assert_eq!(
            integration::endpoint(&config, &route),
            "http://127.0.0.1:8787/v1"
        );
    }
    route.inbound_protocol = aipass_proxy::Protocol::AnthropicMessages;
    assert_eq!(
        integration::endpoint(&config, &route),
        "http://127.0.0.1:8787"
    );
    config.bind_addr = "0.0.0.0:9999".into();
    assert_eq!(
        integration::endpoint(&config, &route),
        "http://127.0.0.1:9999"
    );
    config.bind_addr = "[::]:9999".into();
    assert_eq!(integration::endpoint(&config, &route), "http://[::1]:9999");
}

fn view(cx: &mut TestAppContext) -> (Entity<Proxy>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_base::init(cx);
        crate::menu::init(cx);
        cx.set_global(crate::preferences::Preferences(Default::default(), None));
    });
    let mut view = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let v = cx.new(|cx| Proxy::new(false, false, cx));
        view = Some(v.clone());
        gpui_base::Root::new(v, window, cx)
    });
    cx.simulate_resize(size(px(960.), px(640.)));
    (view.unwrap(), cx)
}

#[gpui::test]
fn integration_values_have_horizontal_scroll_extent(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            s.preview("proxy-integration", w, cx);
            let config = s.config.as_mut().unwrap();
            config.bind_addr = format!("{}.example.invalid:8787", "fixture".repeat(40));
            config.routes[0].token = "synthetic-long-value-".repeat(100);
        })
    });
    cx.update(|w, cx| w.draw(cx).clear(cx));
    view.read_with(cx, |s, _| {
        s.detail_scroll.set_offset(gpui::point(px(0.), px(-10000.)))
    });
    cx.update(|w, cx| w.draw(cx).clear(cx));
    view.read_with(cx, |s, _| {
        assert!(s.integration_endpoint_scroll.max_offset().x > px(0.));
        assert!(s.integration_token_scroll.max_offset().x > px(0.));
    });
}

#[gpui::test]
fn inline_bind_keyboard_save_retains_failed_draft_and_escape_restores_focus(
    cx: &mut TestAppContext,
) {
    let (view, cx) = view(cx);
    cx.update(|w, cx| view.update(cx, |s, cx| s.preview("proxy-empty-bind", w, cx)));
    cx.update(|w, cx| w.draw(cx).clear(cx));
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            use gpui::Focusable;
            assert!(s
                .fields
                .input("proxy-bind")
                .read(cx)
                .focus_handle(cx)
                .is_focused(w));
            s.fields
                .input("proxy-bind")
                .update(cx, |s, cx| s.set_value("127.0.0.1:9999", w, cx));
        })
    });
    cx.simulate_keystrokes("enter");
    cx.update(|w, cx| view.update(cx, |s, cx| {
        let Request {scope, operation} = s.take_request().expect("Enter submits bind address");
        assert!(matches!(operation, Operation::Apply(r) if matches!(&*r, AgentRequest::ServerConfigSet {config} if config.bind_addr == "127.0.0.1:9999")));
        assert!(s.saving_bind && s.editing_bind);
        s.cancel_bind(w, cx);
        assert!(s.editing_bind, "pending saves cannot be discarded");
        s.complete(Completion {scope, result: Err(aipass_agent::AgentCommandError {code: None, message: "address rejected".into()})}, w, cx);
        assert!(s.editing_bind && !s.saving_bind);
        assert_eq!(s.fields.value("proxy-bind", cx), "127.0.0.1:9999");
    }));
    cx.update(|w, cx| w.draw(cx).clear(cx));
    cx.simulate_keystrokes("escape");
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            assert!(!s.editing_bind);
            assert!(!s.fields.contains("proxy-bind"));
            assert!(s.bind_focus.is_focused(w));
            s.begin_bind(w, cx);
            assert_eq!(s.fields.value("proxy-bind", cx), "127.0.0.1:8787");
        })
    });
    cx.update(|w, cx| w.draw(cx).clear(cx));
    cx.simulate_keystrokes("enter");
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            let scope = s.take_request().unwrap().scope;
            s.complete(
                Completion {
                    scope,
                    result: Ok(Outcome::Applied),
                },
                w,
                cx,
            );
            assert!(!s.editing_bind && !s.saving_bind);
            assert!(!s.fields.contains("proxy-bind"));
            assert!(s.bind_focus.is_focused(w));
        })
    });
}
#[gpui::test]
fn populated_fixture_and_route_edit_keep_token_and_member_identity(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            s.preview("proxy-ready", window, cx);
            let config = s.config.clone().unwrap();
            let original = config.routes[0].clone();
            s.edit_route(Some(original.clone()), window, cx);
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|_, cx| {
        view.update(cx, |s, cx| {
            let original = s.config.as_ref().unwrap().routes[0].clone();
            s.save_route(cx);
            let request = s.take_request().unwrap();
            let Operation::Apply(request) = request.operation else {
                panic!("expected route save")
            };
            let AgentRequest::ServerConfigSet { config } = *request else {
                panic!("expected config")
            };
            let route = &config.routes[0];
            assert_eq!(route.token, original.token);
            assert_eq!(route.id, original.id);
            assert_eq!(route.targets[0].id, original.targets[0].id);
        })
    });
}

#[gpui::test]
fn bind_save_queued_behind_failed_refresh_still_closes_after_acceptance(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            s.preview("proxy-empty-bind", w, cx);
            s.refresh(cx);
        })
    });
    cx.update(|w, cx| w.draw(cx).clear(cx));
    cx.simulate_keystrokes("enter");
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            let scope = s.take_request().unwrap().scope;
            assert!(s.saving_bind && s.deferred_operation.is_some());
            s.complete(
                Completion {
                    scope,
                    result: Err(aipass_agent::AgentCommandError {
                        code: None,
                        message: "refresh unavailable".into(),
                    }),
                },
                w,
                cx,
            );
            assert!(s.saving_bind && s.editing_bind);
            let request = s
                .take_request()
                .expect("queued save survives refresh failure");
            assert!(matches!(request.operation, Operation::Apply(_)));
            s.complete(
                Completion {
                    scope: request.scope,
                    result: Ok(Outcome::Applied),
                },
                w,
                cx,
            );
            assert!(!s.saving_bind && !s.editing_bind);
        })
    });
}
#[gpui::test]
fn navigation_retires_late_log_and_preview_replies(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            s.preview("proxy-ready", window, cx);
            s.request(Operation::Logs, false, cx);
            let scope = s.take_request().unwrap().scope;
            s.activate(false, cx);
            s.complete(
                Completion {
                    scope,
                    result: Err(aipass_agent::AgentCommandError {
                        code: None,
                        message: "stale failure".into(),
                    }),
                },
                window,
                cx,
            );
            assert!(s.error.is_empty());
            assert!(s.preview.is_none());
            assert!(!s.pending);
            assert!(s.logs.is_empty());
        })
    });
}
#[gpui::test]
fn reopening_search_retires_old_picker_subscriptions_and_drafts(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            s.preview("proxy-ready", window, cx);
            s.open_modal(Modal::Search, window, cx);
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            assert!(!s.choices.is_empty());
            s.close_modal(window, cx);
            s.open_modal(Modal::Search, window, cx);
            assert!(s.choices.is_empty());
            assert!(s.choice_subscriptions.is_empty());
        })
    });
}

#[gpui::test]
fn user_operation_waits_for_background_refresh_without_being_dropped(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| view.update(cx, |s, cx| {
        s.preview("proxy-ready", window, cx);
        s.notice = "keep notice while polling".into();
        s.refresh(cx);
        assert!(!s.busy());
        assert_eq!(s.notice, "keep notice while polling");
        let scope = s.take_request().unwrap().scope;
        s.apply(AgentRequest::ServerStop, cx);
        assert!(s.busy());
        assert!(s.take_request().is_none());
        s.complete(Completion {scope, result: Ok(Outcome::Proxy {
            config: Box::new(s.config.clone().unwrap()), status: s.status.clone().unwrap(),
            usage: s.usage.clone().unwrap(), series:s.series.clone(),
        })}, window,cx);
        assert!(matches!(s.take_request().unwrap().operation, Operation::Apply(r) if matches!(*r,AgentRequest::ServerStop)));
        assert!(s.take_request().is_none());
    }));
}

#[gpui::test]
fn collapsed_advanced_form_saves_edited_retry_and_member_ids(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            s.preview("proxy-edit", window, cx);
            s.route.as_mut().unwrap().retry.silent_retry = true;
            s.advanced = true;
        })
    });
    cx.update(|w, cx| w.draw(cx).clear(cx));
    cx.update(|window, cx| {
        view.update(cx, |s, cx| {
            s.fields
                .input("silent-retries")
                .update(cx, |v, cx| v.set_value("7", window, cx));
            s.advanced = false;
            let ids = s
                .route
                .as_ref()
                .unwrap()
                .targets
                .iter()
                .map(|t| t.id)
                .collect::<Vec<_>>();
            s.save_route(cx);
            let Operation::Apply(r) = s.take_request().unwrap().operation else {
                panic!("save")
            };
            let AgentRequest::ServerConfigSet { config } = *r else {
                panic!("config")
            };
            assert_eq!(config.routes[0].retry.max_silent_retries, 7);
            assert_eq!(
                config.routes[0]
                    .targets
                    .iter()
                    .map(|t| t.id)
                    .collect::<Vec<_>>(),
                ids
            );
        })
    });
}

#[gpui::test]
fn logs_keep_reader_anchor_when_bounded_history_trims_and_follow_tail_when_requested(
    cx: &mut TestAppContext,
) {
    let (view, cx) = view(cx);
    cx.update(|_, cx| {
        view.update(cx, |s, _| {
            let line = |i| ProxyLogEntry {
                timestamp: i,
                level: "info".into(),
                message: format!("event=fixture seq={i}"),
            };
            s.update_logs((0..100).map(line).collect());
            assert!(s.log_list.is_following_tail());
            s.log_list.scroll_to(gpui::ListOffset {
                item_ix: 40,
                offset_in_item: px(5.),
            });
            s.update_logs((10..110).map(line).collect());
            assert_eq!(s.log_list.logical_scroll_top().item_ix, 30);
            assert_eq!(s.log_list.logical_scroll_top().offset_in_item, px(5.));
            assert!(!s.log_list.is_following_tail());
            assert_eq!(s.log_list.item_count(), 100);
            s.log_list.set_follow_mode(gpui::FollowMode::Tail);
            s.update_logs((20..120).map(line).collect());
            assert!(s.log_list.is_following_tail());
        })
    });
}
#[gpui::test]
fn chart_keyboard_focus_shows_current_bucket_and_blur_clears_it(cx: &mut TestAppContext) {
    let (view, cx) = view(cx);
    cx.update(|w, cx| view.update(cx, |s, cx| s.preview("proxy-ready", w, cx)));
    cx.update(|w, cx| w.draw(cx).clear(cx));
    cx.run_until_parked();
    cx.update(|w, cx| view.update(cx, |s, cx| s.chart_focus.handles[0].focus(w, cx)));
    cx.update(|w, cx| w.draw(cx).clear(cx));
    cx.run_until_parked();
    cx.update(|w, cx| {
        view.update(cx, |s, cx| {
            assert!(
                s.hovered_bucket.is_some(),
                "series={:?}, focus={:?}",
                s.series
                    .iter()
                    .map(|p| (&p.date, p.request_count))
                    .collect::<Vec<_>>(),
                s.chart_focus
                    .handles
                    .iter()
                    .map(|h| h.is_focused(w))
                    .collect::<Vec<_>>()
            );
            s.close_focus.focus(w, cx);
        })
    });
    cx.update(|w, cx| w.draw(cx).clear(cx));
    cx.run_until_parked();
    cx.update(|_, cx| view.update(cx, |s, _| assert!(s.hovered_bucket.is_none())));
}
