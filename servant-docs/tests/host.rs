use servant::prelude::*;
use servant_docs::{HasDocs, markdown};

#[test]
fn host_requirement_is_recorded_in_docs() {
    // Given: an endpoint scoped to one host.
    let api = host("api.example.com", path("status", get::<(Json,), String>()));

    // When: docs are generated from the shared API description.
    let doc = api.docs();
    let endpoint = &doc.endpoints()[0];

    // Then: the docs model and Markdown record the host requirement.
    let host = endpoint.host.as_ref().expect("host requirement");
    assert_eq!(host.name, "api.example.com");
    assert_eq!(
        host.port_policy,
        servant::host::HostPortPolicy::IgnoreRequestPort
    );

    let md = markdown(&doc);
    assert!(md.contains("### Host:"), "md:\n{md}");
    assert!(
        md.contains("Requires `Host: api.example.com`"),
        "host note missing:\n{md}"
    );
}
