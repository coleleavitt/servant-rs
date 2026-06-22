use servant::prelude::*;
use servant_openapi::{OpenApiInfo, openapi_for};

#[test]
fn raw_endpoints_are_omitted_from_openapi() {
    // Given: an API with a Raw endpoint and a normal typed endpoint.
    let api = alt(
        path("api", path("files", raw())),
        path("status", get::<(PlainText,), String>()),
    );

    // When: an OpenAPI document is generated.
    let doc = openapi_for(&api, OpenApiInfo::new("Raw API", "0.1.0"));

    // Then: the Raw path is omitted and the typed path remains.
    assert!(doc["paths"].get("/api/files").is_none(), "{doc}");
    assert!(doc["paths"].get("/status").is_some(), "{doc}");
}
