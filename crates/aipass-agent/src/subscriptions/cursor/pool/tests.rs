use super::*;
use aipass_proxy_conversion::providers::connect::*;
use bytes::Bytes;
use http_body_util::{BodyExt, StreamBody};
use hyper::{
    body::{Frame, Incoming},
    service::service_fn,
};
use hyper_util::rt::{TokioExecutor, TokioIo};
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn parked_runs_are_revoked_on_proxy_change_without_a_live_request_adapter() {
    let owner = Id::new_v4();
    let (commands, _receiver) = mpsc::channel(1);
    let (abort, cancellation) = watch::channel(false);
    let entry = Arc::new(Entry {
        owner,
        binding: format!("{:x}", Sha256::digest(b"test-key")),
        session: "parked".into(),
        context: [0; 32],
        outbound: None,
        commands,
        abort,
        state: Mutex::new(State {
            busy: false,
            closed: false,
            parked: true,
            history: [0; 32],
            history_len: 0,
            text: String::new(),
            pending: vec![],
            touched: Instant::now(),
        }),
    });
    let mut pool = Pool::default();
    pool.entries.insert(Id::new_v4(), entry);
    let mut target = ResolvedTarget {
        config: serde_json::from_value(json!({"id":owner,"providerEntryId":Id::new_v4(),"secretId":"primary","label":"cursor","baseUrl":"https://cursor.com","authScheme":"bearer","group":null,"priority":0,"enabled":true})).unwrap(),
        api_key: "test-key".into(), upstream_kind: aipass_proxy::UpstreamKind::CommunitySubscription,
        profile: aipass_proxy::ProviderProfile::Generic, upstream_proxy: None,
        quota: vec![], model_override: None, max_concurrent_requests: None, supports_websockets: false,
    };
    pool.retain_targets(&[&target]);
    assert_eq!(pool.entries.len(), 1);
    assert!(!*cancellation.borrow());
    target.upstream_proxy = Some(UpstreamProxyConfig::default());
    pool.retain_targets(&[&target]);
    assert!(pool.entries.is_empty());
    assert!(
        *cancellation.borrow(),
        "the retained connection must be cancelled"
    );
}

#[tokio::test]
async fn one_native_http2_run_survives_tool_result_and_validates_the_returned_history() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(AtomicUsize::new(0));
    let seen = requests.clone();
    let server = tokio::spawn(async move {
        let socket = super::super::test_server::accept(&listener).await.unwrap();
        hyper::server::conn::http2::Builder::new(TokioExecutor::new())
            .serve_connection(
                TokioIo::new(socket),
                service_fn(move |request: hyper::Request<Incoming>| {
                    seen.fetch_add(1, Ordering::SeqCst);
                    async move {
                        assert_eq!(request.uri().path(), "/agent.v1.AgentService/Run");
                        let mut input = request.into_body();
                        let mut frames = Frames::default();
                        let first = input.frame().await.unwrap().unwrap().into_data().unwrap();
                        assert!(!frames.push(&first).unwrap().is_empty());
                        let (tx, rx) =
                            mpsc::channel::<std::result::Result<Frame<Bytes>, std::io::Error>>(8);
                        let text = bytes(1, &string(1, "checking"));
                        let count = bytes(27, &uint(1, 1));
                        let call = bytes(
                            2,
                            &[
                                uint(1, 7),
                                string(15, "exec-7"),
                                bytes(
                                    11,
                                    &[string(5, "search"), string(3, "native-call")].concat(),
                                ),
                            ]
                            .concat(),
                        );
                        tx.send(Ok(Frame::data(Bytes::from(frame(
                            0,
                            &[bytes(1, &[text, count].concat()), call].concat(),
                        )))))
                        .await
                        .unwrap();
                        tokio::spawn(async move {
                            let (mut result, mut ack) = (false, false);
                            while !result || !ack {
                                let data =
                                    input.frame().await.unwrap().unwrap().into_data().unwrap();
                                for (_, payload) in frames.push(&data).unwrap() {
                                    for (n, f) in fields(&payload).unwrap() {
                                        let Field::Bytes(data) = f else {
                                            continue;
                                        };
                                        let values = fields(data).unwrap();
                                        if n == 2 {
                                            assert!(!result, "tool result must be uploaded once");
                                            assert_eq!(field_int(&values, 1), 7);
                                            assert_eq!(field_text(&values, 15), "exec-7");
                                            let mcp =
                                                fields(field_bytes(&values, 11).unwrap()).unwrap();
                                            let success =
                                                fields(field_bytes(&mcp, 1).unwrap()).unwrap();
                                            let content =
                                                fields(field_bytes(&success, 1).unwrap()).unwrap();
                                            let text =
                                                fields(field_bytes(&content, 1).unwrap()).unwrap();
                                            assert_eq!(field_text(&text, 1), "found");
                                            result = true;
                                        } else if n == 5 {
                                            assert!(result, "exec close must follow its result");
                                            let close =
                                                fields(field_bytes(&values, 1).unwrap()).unwrap();
                                            assert_eq!(field_int(&close, 1), 7);
                                            ack = true;
                                        }
                                    }
                                }
                            }
                            let usage =
                                [uint(1, 100), uint(2, 7), uint(3, 80), uint(4, 5)].concat();
                            let final_event = bytes(
                                1,
                                &[bytes(1, &string(1, "done")), bytes(14, &usage)].concat(),
                            );
                            tx.send(Ok(Frame::data(Bytes::from(frame(0, &final_event)))))
                                .await
                                .unwrap();
                            drop(tx);
                            while input.frame().await.is_some() {}
                        });
                        let body = futures_util::stream::unfold(rx, |mut rx| async move {
                            rx.recv().await.map(|v| (v, rx))
                        });
                        Ok::<_, std::convert::Infallible>(hyper::Response::new(StreamBody::new(
                            body,
                        )))
                    }
                }),
            )
            .await
            .unwrap();
    });
    super::super::test_server::probe(&base).await;
    let body = json!({"model":"m","stream":true,"messages":[{"role":"user","content":"find"}],"tools":[{"type":"function","function":{"name":"search","parameters":{"type":"object"}}}]});
    let (commands, receiver) = mpsc::channel(1);
    let (abort, cancel) = watch::channel(false);
    let entry = Arc::new(Entry {
        owner: Id::new_v4(),
        binding: "test".into(),
        session: "test".into(),
        context: context(&body, "m"),
        outbound: None,
        commands,
        abort,
        state: Mutex::new(State {
            busy: true,
            closed: false,
            parked: false,
            history: digest(&body["messages"]),
            history_len: 1,
            text: String::new(),
            pending: vec![],
            touched: Instant::now(),
        }),
    });
    let worker = entry.clone();
    std::thread::spawn(move || {
        connection::start(
            worker,
            receiver,
            cancel,
            SensitiveString::new("test-only"),
            None,
            Some(base),
            "m".into(),
        )
    });
    let mut lease = Lease {
        entry: entry.clone(),
        armed: true,
    };
    tokio::time::timeout(Duration::from_secs(10),async {
        let (tx,mut rx)=mpsc::channel(16);
        entry.commands.send(Turn {body:body.clone(),output:tx}).await.unwrap();
        loop {match rx.recv().await.unwrap().unwrap() {Event::Data(_)=>{},Event::End=>break,Event::Rejected(..)=>panic!("unexpected native refusal")}}
        let id={let state=entry.state.lock().unwrap();assert!(state.parked);state.pending[0].id.clone()};
        let mut next=body.clone();
        next["messages"].as_array_mut().unwrap().extend([json!({"role":"assistant","content":"checking","tool_calls":[{"id":id,"type":"function","function":{"name":"search","arguments":"{}"}}]}),json!({"role":"tool","tool_call_id":id,"content":"found"})]);
        let mut changed=next.clone();changed["messages"][0]["content"]=json!("different");
        assert!(validate(&entry.state.lock().unwrap(),&changed).is_err());
        changed=next.clone();changed["messages"][1]["tool_calls"][0]["function"]["name"]=json!("shell");
        assert!(validate(&entry.state.lock().unwrap(),&changed).is_err());
        validate(&entry.state.lock().unwrap(),&next).unwrap();
        assert_eq!(requests.load(Ordering::SeqCst),1);
        let (tx,mut rx)=mpsc::channel(16);
        entry.commands.send(Turn {body:next,output:tx}).await.unwrap();
        let mut output=Vec::new();
        loop {match rx.recv().await.unwrap().unwrap() {Event::Data(v)=>output.extend(v),Event::End=>break,Event::Rejected(..)=>panic!("unexpected native refusal")}}
        let text=String::from_utf8(output).unwrap();
        assert!(text.contains("done"));assert!(text.contains("\"cache_creation_tokens\":5"));
        assert_eq!(requests.load(Ordering::SeqCst),1);
    }).await.unwrap();
    lease.armed = false;
    revoke_owners(&HashSet::from([entry.owner]));
    server.abort();
}
