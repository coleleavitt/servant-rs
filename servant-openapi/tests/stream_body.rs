use servant::prelude::*;
use servant_openapi::{OpenApiInfo, openapi_for};

#[test]
fn stream_body_request_body_is_explicit_openapi_extension() {
    let api = path(
        "sum",
        stream_body::<NetstringFraming, Json, u64, _>(verb::<Post, 200, (PlainText,), String>()),
    );
    let doc = openapi_for(&api, OpenApiInfo::new("Streaming", "0.1.0"));
    let body = &doc["paths"]["/sum"]["post"]["requestBody"];

    assert_eq!(body["required"], true);
    assert_eq!(body["x-servant-streaming-request-body"], true);
    assert!(
        body["content"]["application/json"]["schema"].is_object(),
        "requestBody json schema missing: {doc:#}"
    );
}
