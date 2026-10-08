use super::*;

#[tokio::test]
async fn group_model_is_mapped_for_http_and_downstream_websocket_generations() {
    let upstream = mock(0).await;
    let mut route = test_route(upstream.address.clone());
    route.targets[0].config.model = Some("actual-model".into());
    let alias = crate::model_routes::model_id(&route).unwrap();
    let (handle, store, _dir) = start_proxy(vec![route], direct());
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let response = client
        .post(format!("http://{}/v1/responses", handle.bind_addr))
        .bearer_auth(LOCAL_TOKEN)
        .json(&json!({"model":alias,"input":"hello","stream":false}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.json::<Value>().await.unwrap()["status"],
        "completed"
    );
    let mut ws = connect(&handle, "/v1/responses", LOCAL_TOKEN)
        .await
        .unwrap();
    ws.send(Message::text(
        json!({"type":"response.create","model":alias,"input":"second"}).to_string(),
    ))
    .await
    .unwrap();
    let events = receive_response(&mut ws).await;
    assert_eq!(events.last().unwrap()["type"], "response.completed");
    assert_eq!(upstream.calls.lock().unwrap().len(), 2);
    assert!(upstream
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|(native, payload)| *native && payload["model"] == "actual-model"));
    assert!(store
        .summary(|_| 0)
        .unwrap()
        .models
        .iter()
        .all(|model| model.model.as_deref() == Some("actual-model")));
    ws.close(None).await.unwrap();
}
