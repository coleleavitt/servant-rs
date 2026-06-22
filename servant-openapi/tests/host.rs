use servant::prelude::*;
use servant_openapi::{OpenApiInfo, openapi_for};

#[test]
fn host_requirement_is_explicit_openapi_extension() {
    // Given: an endpoint scoped to an explicit Host authority.
    let api = host(
        "api.example.com:8443",
        path("status", get::<(Json,), String>()),
    );

    // When: an OpenAPI document is generated.
    let doc = openapi_for(&api, OpenApiInfo::new("Host API", "0.1.0"));
    let operation = &doc["paths"]["/status"]["get"];

    // Then: Host is represented explicitly as a servant-rs extension.
    assert_eq!(operation["x-servant-host"]["name"], "api.example.com:8443");
    assert_eq!(operation["x-servant-host"]["hostnameCaseInsensitive"], true);
    assert_eq!(
        operation["x-servant-host"]["portPolicy"],
        "explicit-port-must-match"
    );
    assert_eq!(operation["x-servant-host"]["port"], 8443);
}

#[test]
fn host_alternatives_do_not_overwrite_same_path_openapi_metadata() {
    // Given: same-path GET endpoints scoped by different Host authorities.
    let api = alt(
        host("api.example.com", path("status", get::<(Json,), String>())),
        host(
            "admin.example.com",
            path("status", get::<(Json,), String>()),
        ),
    );

    // When: OpenAPI collapses them into the one Operation slot OAS allows.
    let doc = openapi_for(&api, OpenApiInfo::new("Host API", "0.1.0"));
    let operation = &doc["paths"]["/status"]["get"];

    // Then: the left operation remains stable and all Host alternatives are
    // recorded explicitly instead of the later Host replacing the earlier one.
    assert_eq!(operation["x-servant-host"]["name"], "api.example.com");

    let alternatives = operation["x-servant-host-alternatives"]
        .as_array()
        .expect("host alternatives");
    let names: Vec<_> = alternatives
        .iter()
        .map(|alternative| alternative["host"]["name"].as_str().expect("host name"))
        .collect();
    assert_eq!(names, vec!["api.example.com", "admin.example.com"]);
    assert_eq!(
        alternatives[1]["operation"]["x-servant-host"]["name"],
        "admin.example.com"
    );
}
