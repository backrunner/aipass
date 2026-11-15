use super::*;
use crate::catalog::Action;

fn draft(raw: &str) -> Draft {
    into_draft(parse(raw.into()).unwrap()).ok().unwrap()
}

#[test]
fn cc_switch_preserves_endpoint_identity_and_explicit_protocol() {
    let official = draft("ccswitch://v1/import?resource=provider&app=claude&name=Official&endpoint=https%3A%2F%2FAPI.ANTHROPIC.COM%2F&apiKey=fixture");
    assert_eq!(official.provider_id, "anthropic");
    assert_eq!(official.auth, AuthScheme::Bearer);
    let relay = draft("ccswitch://v1/import?resource=provider&app=claude&name=Relay&endpoint=https%3A%2F%2Fapi.anthropic.com%3Ftenant%3Dother&apiKey=fixture");
    assert_eq!(relay.provider_id, "custom_http");
    assert_eq!(relay.interface, InterfaceType::AnthropicMessages);
    assert_eq!(relay.auth, AuthScheme::Bearer);
}

#[test]
fn aipass_import_keeps_optional_metadata_and_escaped_fields_until_explicit_save() {
    let mut url = Url::parse("aipass-provider://v1/add").unwrap();
    url.query_pairs_mut().extend_pairs([
        ("title", "Imported"),
        ("apiKey", "fixture-key"),
        ("providerId", "future-provider"),
        ("interfaceType", "openai_compatible"),
        ("authScheme", "bearer"),
        ("credentialKind", "api"),
        ("accountIdentity", "fixture account"),
        ("faviconUrl", "https://fixture.invalid/icon.png"),
        ("endpoint", "https://relay.invalid/?models=a,b"),
        ("domain", "team,one"),
        ("tag", "tag,one"),
        ("headers", "[[\"x=tenant\",\"value,one\"]]"),
        ("modelAliases", "[[\"fast,cheap\",\"model=one\"]]"),
        ("quota", "{\"label\":\"Monthly\",\"remaining\":\"10\"}"),
    ]);
    let imported = draft(url.as_str());
    assert!(imported.original.is_none());
    assert_eq!(imported.provider_id, "custom_openai_compatible");
    let Action::Add(input) = imported.into_action().unwrap() else {
        panic!("add draft")
    };
    assert_eq!(input.api_key, "fixture-key");
    assert_eq!(input.account_identity.as_deref(), Some("fixture account"));
    assert_eq!(
        input.favicon_url.as_deref(),
        Some("https://fixture.invalid/icon.png")
    );
    assert_eq!(input.quota.unwrap().remaining.as_deref(), Some("10"));
    assert_eq!(
        input.model_aliases,
        [("fast,cheap".into(), "model=one".into())]
    );
    assert_eq!(input.headers, [("x=tenant".into(), "value,one".into())]);
    assert_eq!(input.tags, ["tag,one"]);
    assert_eq!(input.domains, ["team,one"]);
}

#[test]
fn duplicates_use_full_active_catalog_and_do_not_discard_tenant_queries() {
    let mut catalog = Catalog::fixture();
    catalog.active[0].endpoints = vec![aipass_provider_registry::ProviderEndpoint::api(
        "https://relay.invalid/v1/?tenant=a",
    )];
    let mut imported = draft("ccswitch://v1/import?resource=provider&app=codex&name=New&endpoint=https%3A%2F%2FRELAY.INVALID%2Fv1%3Ftenant%3Da&apiKey=fixture");
    assert_eq!(
        duplicate(&catalog, &imported).unwrap().id,
        catalog.active[0].id
    );
    imported.endpoint = "https://relay.invalid/v1?tenant=b".into();
    assert!(duplicate(&catalog, &imported).is_none());
    imported.title = catalog.active[1].title.clone();
    assert_eq!(
        duplicate(&catalog, &imported).unwrap().id,
        catalog.active[1].id
    );
}

#[test]
fn invalid_and_unsupported_links_have_safe_localizable_messages() {
    assert!(parse("https://example.invalid".into()).is_none());
    let error =
        into_draft(parse("ccswitch://v1/import?resource=skill&apiKey=do-not-echo".into()).unwrap())
            .err()
            .unwrap();
    assert_eq!(error.key, "deepLink.unsupportedResource");
    assert!(!error.value.contains("do-not-echo"));
    let error = into_draft(
        parse("aipass-provider://v1/add?title=Foo&interfaceType=wrong&apiKey=do-not-echo".into())
            .unwrap(),
    )
    .err()
    .unwrap();
    assert_eq!(error.key, "deepLink.importFailed");
    assert!(!error.value.contains("do-not-echo"));
}
