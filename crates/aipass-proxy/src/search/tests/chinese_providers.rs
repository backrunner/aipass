use super::*;

fn response(vendor: SearchVendor) -> Value {
    match vendor {
        SearchVendor::Bocha => json!({"code":200,"data":{"webPages":{"value":[
            {"name":"博查来源","url":"https://example.com/source","summary":"完整摘要","snippet":"短摘要"}
        ]}}}),
        SearchVendor::Yuanbao => json!({"Response":{"Pages":[
            json!({"title":"元宝来源","url":"https://example.com/source","content":"完整摘要","passage":"短摘要"}).to_string()
        ],"RequestId":"synthetic"}}),
        _ => unreachable!(),
    }
}

#[test]
fn chinese_search_configuration_preserves_vault_references() {
    for (vendor, name, base) in [
        (SearchVendor::Bocha, "bocha", "https://api.bocha.cn"),
        (
            SearchVendor::Yuanbao,
            "yuanbao",
            "https://api.wsa.cloud.tencent.com",
        ),
    ] {
        assert_eq!(vendor.base(), base);
        let mut api = source(base.into()).config.clone();
        api.vendor = vendor;
        api.base_url = None;
        let config = SearchConfig {
            apis: vec![api],
            ..Default::default()
        };
        config.validate().unwrap();
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["apis"][0]["vendor"], name);
        assert_eq!(
            serde_json::from_value::<SearchConfig>(json).unwrap(),
            config
        );
        let mut missing = config;
        missing.apis[0].provider_entry_id = None;
        missing.apis[0].secret_id = None;
        assert!(missing.validate().is_err());
    }
}

#[test]
fn chinese_search_decoders_handle_envelopes_and_partial_pages() {
    for vendor in [SearchVendor::Bocha, SearchVendor::Yuanbao] {
        let result = api::decode(vendor, &response(vendor)).unwrap();
        assert_eq!(result["results"][0]["content"], "完整摘要");
        assert_eq!(result["results"][0]["url"], "https://example.com/source");
        assert_eq!(
            result["results"][0]["title"],
            match vendor {
                SearchVendor::Bocha => "博查来源",
                _ => "元宝来源",
            }
        );
        assert!(api::decode(vendor, &json!({})).is_err());
    }
    let mut bocha = response(SearchVendor::Bocha);
    bocha["code"] = json!(401);
    assert!(api::decode(SearchVendor::Bocha, &bocha).is_err());
    bocha["code"] = json!(200);
    bocha["data"]["webPages"]["value"][0]["summary"] = json!("");
    assert_eq!(
        api::decode(SearchVendor::Bocha, &bocha).unwrap()["results"][0]["content"],
        "短摘要"
    );
    let mut yuanbao = response(SearchVendor::Yuanbao);
    yuanbao["Response"]["Error"] =
        json!({"Code":"ResourceUnavailable","Message":"synthetic-secret"});
    assert!(api::decode(SearchVendor::Yuanbao, &yuanbao).is_err());
    let pages = json!({"Response":{"Pages":["invalid", "null",
        json!({"title":"unsafe","url":"javascript:alert(1)"}).to_string(),
        json!({"title":"摘要来源","url":"https://example.com/summary","passage":"短摘要"}).to_string()
    ]}});
    let result = api::decode(SearchVendor::Yuanbao, &pages).unwrap();
    assert_eq!(result["results"].as_array().unwrap().len(), 1);
    assert_eq!(result["results"][0]["content"], "短摘要");
    assert!(api::decode(SearchVendor::Yuanbao, &json!({"Response":{"Pages":[]}})).is_err());
}

#[tokio::test]
async fn chinese_search_mcp_transports_and_application_error_fallback() {
    for vendor in [SearchVendor::Bocha, SearchVendor::Yuanbao] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            for attempt in 0..2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let (headers, body) = request(&mut stream).await;
                let path = if vendor == SearchVendor::Bocha {
                    "/v1/web-search"
                } else {
                    "/SearchPro"
                };
                assert!(headers.starts_with(&format!("POST {path} HTTP/1.1")));
                assert!(headers
                    .to_ascii_lowercase()
                    .contains("authorization: bearer synthetic-search-key"));
                assert_eq!(
                    body,
                    if vendor == SearchVendor::Bocha {
                        json!({"query":"中文搜索","count":6,"summary":true,"freshness":"noLimit"})
                    } else {
                        json!({"Query":"中文搜索"})
                    }
                );
                let body = if attempt == 0 {
                    if vendor == SearchVendor::Bocha {
                        json!({"code":429,"msg":"synthetic-search-key"})
                    } else {
                        json!({"Response":{"Error":{"Code":"RequestLimitExceeded","Message":"synthetic-search-key"}}})
                    }
                } else {
                    response(vendor)
                };
                respond(&mut stream, "200 OK", body).await;
            }
        });
        let route = single_target_route(
            "local-search-token",
            "http://127.0.0.1:1".into(),
            RetryPolicy::default(),
        );
        let mut config = RuntimeConfig::from_routes("", vec![route]);
        for _ in 0..2 {
            let mut api = source(base.clone());
            api.config.vendor = vendor;
            config.search.apis.push(api.config.clone());
            config.search_apis.push(api);
        }
        let (_temp, handle, url) = start(config);
        let reply: Value = client().post(format!("{url}{MCP_PATH}"))
            .bearer_auth("local-search-token")
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"web_search","arguments":{"query":"中文搜索"}}}))
            .send().await.unwrap().json().await.unwrap();
        assert_eq!(reply["result"]["isError"], false, "{reply}");
        assert_eq!(
            reply["result"]["structuredContent"]["results"][0]["content"],
            "完整摘要"
        );
        assert!(handle.logs().unwrap().iter().all(
            |l| !l.message.contains("synthetic-search-key") && !l.message.contains("中文搜索")
        ));
        task.await.unwrap();
    }
}

#[tokio::test]
async fn oversized_yuanbao_query_falls_through_without_a_request() {
    let rejected = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fallback = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut yuanbao = source(format!("http://{}", rejected.local_addr().unwrap()));
    yuanbao.config.vendor = SearchVendor::Yuanbao;
    let next = source(format!("http://{}", fallback.local_addr().unwrap()));
    let task = tokio::spawn(async move {
        let (mut stream, _) = fallback.accept().await.unwrap();
        let (_, body) = request(&mut stream).await;
        assert_eq!(body["query"].as_str().unwrap().len(), 1503);
        respond(&mut stream, "200 OK", json!({"results":[{"url":"https://example.com/fallback","title":"fallback","content":"fact"}]})).await;
    });
    let route = single_target_route(
        "local-search-token",
        "http://127.0.0.1:1".into(),
        RetryPolicy::default(),
    );
    let mut config = RuntimeConfig::from_routes("", vec![route]);
    for api in [yuanbao, next] {
        config.search.apis.push(api.config.clone());
        config.search_apis.push(api);
    }
    let (_temp, _handle, url) = start(config);
    let reply: Value = client().post(format!("{url}{MCP_PATH}"))
        .bearer_auth("local-search-token")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"web_search","arguments":{"query":"中".repeat(501)}}}))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    assert_no_api_request(&rejected).await;
    task.await.unwrap();
}
