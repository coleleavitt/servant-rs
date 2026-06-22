use servant::prelude::*;
use servant_client::{ClientRequest, HasClient};

#[test]
fn host_combinator_sets_host_header() {
    // Given: a generated-client endpoint under a Host combinator.
    let api = host(
        "api.example.com:8443",
        path("status", get::<(PlainText,), String>()),
    );
    let mut req = ClientRequest::new();

    // When: the client request is built.
    api.build_request(servant::hlist::HNil, &mut req)
        .expect("request builds");

    // Then: Host is emitted as an HTTP header and the URL target is unchanged.
    assert_eq!(req.target(), "/status");
    assert_eq!(req.method, http::Method::GET);
    assert_eq!(
        req.headers
            .get(http::header::HOST)
            .and_then(|value| value.to_str().ok()),
        Some("api.example.com:8443")
    );
}
