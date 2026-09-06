use super::*;
use serde_json::json;

fn store() -> (UsageStore, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    (
        UsageStore::open(dir.path().join("usage.sqlite")).unwrap(),
        dir,
    )
}

#[tokio::test]
async fn summaries_skip_payloads_and_support_file_backed_requests() {
    use std::io::Write;
    let payload = serde_json::to_vec(&json!({
        "input":"PRIVATE_PROMPT", "previous_response_id":"PRIVATE_RESPONSE_ID",
        "tools":[{"type":"namespace","name":"PRIVATE_NAMESPACE","tools":[
            {"type":"custom","name":"exec","description":"PRIVATE_DESCRIPTION","format":{"grammar":"PRIVATE_GRAMMAR"}},
            {"type":"function","name":"PRIVATE_TOOL_NAME","parameters":{"PRIVATE_SCHEMA":"secret"}}
        ]},{"type":"shell"},{"type":"PRIVATE_TOOL_TYPE"}]
    })).unwrap();
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(&payload).unwrap();
    let bodies = [
        ReplayableRequestBody::File {
            file,
            len: payload.len() as u64,
        },
        ReplayableRequestBody::Memory(Bytes::from(payload.clone())),
    ];
    let (store, _dir) = store();
    for body in bodies {
        let summary = body.parse_metadata::<RequestSummary>().await.unwrap();
        RequestSummary::log(Some(&summary), &store, Uuid::new_v4(), "test");
        // Reading metadata must leave the actual replayable request intact.
        assert_eq!(
            body.json().await.unwrap(),
            serde_json::from_slice::<Value>(&payload).unwrap()
        );
    }
    for entry in store.logs().unwrap() {
        assert!(entry.message.contains(
            "tools=5 function=1 custom=1 shell=1 namespace=1 other=1 exec=1 previous_response=true"
        ));
        assert!(!entry.message.contains("PRIVATE"));
    }
}

#[test]
fn summaries_distinguish_missing_empty_and_unavailable_tools() {
    let (store, _dir) = store();
    for value in [
        json!({}),
        json!({"tools":[]}),
        json!({"tools":"PRIVATE_INVALID"}),
    ] {
        let summary = RequestSummary::deserialize(&value).ok();
        RequestSummary::log(summary.as_ref(), &store, Uuid::new_v4(), "test");
    }
    let logs = store.logs().unwrap();
    assert!(logs[0].message.contains("tools_present=false tools=0"));
    assert!(logs[1].message.contains("tools_present=true tools=0"));
    assert!(logs[2].message.contains("available=false"));
    assert!(!logs[2].message.contains("PRIVATE"));
}

#[test]
fn response_trace_detects_ordering_without_logging_deltas() {
    let mut trace = ResponseTrace::default();
    trace.observe(
        &json!({"type":"response.created","sequence_number":0,"response":{"id":"PRIVATE_ID"}}),
    );
    for sequence in 1..1001 {
        trace.observe(&json!({"type":"response.output_text.delta","sequence_number":sequence,"output_index":0,"delta":"PRIVATE_TEXT"}));
    }
    trace.observe(&json!({"type":"response.output_item.added","sequence_number":1001,"output_index":1,"item":{"type":"custom_tool_call","name":"PRIVATE_TOOL"}}));
    trace.observe(&json!({"type":"response.custom_tool_call_input.delta","sequence_number":1002,"output_index":1,"delta":"PRIVATE_COMMAND"}));
    trace.observe(&json!({"type":"response.completed","sequence_number":1002}));
    let (store, _dir) = store();
    trace.log(&store, Uuid::new_v4(), "test");
    let logs = store.logs().unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].level, "warn");
    assert!(logs[0].message.contains("tool_items=1 text_deltas=1000 tool_deltas=1 terminal=1 delta_without_item=1000 sequence_regressions=1"));
    assert!(!logs[0].message.contains("PRIVATE"));
}

#[test]
fn split_sse_events_are_observed_once_and_valid_sequence_is_healthy() {
    let events = [
        json!({"type":"response.created"}),
        json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message"}}),
        json!({"type":"response.output_text.delta","output_index":0,"delta":"secret"}),
        json!({"type":"response.output_item.done","output_index":0}),
        json!({"type":"response.completed"}),
    ];
    let sse = events
        .iter()
        .enumerate()
        .map(|(seq, value)| {
            let mut value = value.clone();
            value["sequence_number"] = json!(seq);
            format!("data: {value}\n\n")
        })
        .collect::<String>();
    let mut trace = ResponseTrace::default();
    let mut buffer = Vec::new();
    let mut usage = TokenUsage::default();
    for chunk in sse.as_bytes().chunks(7) {
        observe_sse_usage_traced(
            ProxyProtocol::OpenAiResponses,
            &mut buffer,
            chunk,
            &mut usage,
            Some(&mut trace),
        );
    }
    assert_eq!(trace.events, 5);
    assert_eq!(trace.delta_without_item, 0);
    assert_eq!(trace.sequence_regressions, 0);
    assert_eq!(trace.terminal, 1);
}

#[test]
fn response_tracking_is_bounded_and_unknown_events_are_not_logged() {
    let mut trace = ResponseTrace::default();
    for index in 0..1000 {
        trace.observe(&json!({"type":"response.output_item.added","output_index":index}));
    }
    assert_eq!(trace.active_items.len(), 256);
    assert!(trace.tracking_limited);
    trace.observe(&json!({"type":"PRIVATE_EVENT_TYPE"}));
    let (store, _dir) = store();
    trace.log(&store, Uuid::new_v4(), "test");
    let log = store.logs().unwrap().pop().unwrap();
    assert!(log
        .message
        .contains("tracking_limited=true last_event=other"));
    assert!(!log.message.contains("PRIVATE"));
}

#[test]
fn websocket_error_logs_only_fixed_reason_status_and_os_code() {
    let (store, _dir) = store();
    let diagnostic = WsDiagnostic {
        store: &store,
        request_id: Uuid::new_v4(),
        route_id: Uuid::new_v4(),
        provider_id: Uuid::new_v4(),
    };
    let io = std::io::Error::from_raw_os_error(49);
    let nested = tokio_tungstenite::tungstenite::Error::Io(io);
    diagnostic.log("connect_failed", None, Some(&nested));
    diagnostic.log(
        "connect_failed",
        None,
        Some(&std::io::Error::other(
            "https://PRIVATE_URL/?key=PRIVATE_KEY",
        )),
    );
    let logs = store.logs().unwrap();
    assert!(logs[0].message.contains("os_error=Some(49)"));
    assert!(logs[1].message.contains("os_error=None"));
    assert!(!logs.iter().any(|entry| entry.message.contains("PRIVATE")));
}
