use bytes::Bytes;
use http::{Request, StatusCode};
use http_body_util::{BodyExt, Full};
use servant::prelude::*;
use servant_server::{RouterService, serve};

async fn call(svc: &RouterService, req: Request<Full<Bytes>>) -> (StatusCode, String) {
    let resp = svc.handle(req).await;
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

fn get_with_host(uri: &str, host: &str) -> Request<Full<Bytes>> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header(http::header::HOST, host)
        .body(Full::new(Bytes::new()))
        .unwrap()
}

#[tokio::test]
async fn host_alternatives_dispatch_by_header() {
    // Given: sibling routes that differ only by Host.
    let api = alt(
        host(
            "api.example.com",
            path("status", get::<(PlainText,), String>()),
        ),
        host(
            "admin.example.com",
            path("status", get::<(PlainText,), String>()),
        ),
    );
    let svc = RouterService::new(serve(
        api,
        (
            || async { Ok::<_, ServerError>("api".to_string()) },
            || async { Ok::<_, ServerError>("admin".to_string()) },
        ),
    ));

    // When: the request carries the admin Host header.
    let (status, body) = call(&svc, get_with_host("/status", "admin.example.com")).await;

    // Then: the admin sibling handles the request.
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "admin");
}

#[tokio::test]
async fn host_mismatch_falls_through_to_sibling_or_404() {
    // Given: a host-specific route with a generic same-path sibling.
    let fallback_api = alt(
        host(
            "api.example.com",
            path("status", get::<(PlainText,), String>()),
        ),
        path("status", get::<(PlainText,), String>()),
    );
    let fallback_svc = RouterService::new(serve(
        fallback_api,
        (
            || async { Ok::<_, ServerError>("api".to_string()) },
            || async { Ok::<_, ServerError>("fallback".to_string()) },
        ),
    ));

    // When: the Host header does not match the host-specific branch.
    let (status, body) = call(&fallback_svc, get_with_host("/status", "other.example.com")).await;

    // Then: the recoverable mismatch lets the sibling handle it.
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "fallback");

    // Given: only a host-specific route.
    let host_only = RouterService::new(serve(
        host(
            "api.example.com",
            path("status", get::<(PlainText,), String>()),
        ),
        || async { Ok::<_, ServerError>("api".to_string()) },
    ));

    // When: the Host header still does not match.
    let (status, _) = call(&host_only, get_with_host("/status", "other.example.com")).await;

    // Then: no route is selected.
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn host_matching_is_case_insensitive() {
    // Given: a mixed-case Host requirement.
    let svc = RouterService::new(serve(
        host(
            "API.Example.COM",
            path("status", get::<(PlainText,), String>()),
        ),
        || async { Ok::<_, ServerError>("matched".to_string()) },
    ));

    // When: the request uses a differently cased host name.
    let (status, body) = call(&svc, get_with_host("/status", "api.example.com")).await;

    // Then: hostname case does not affect matching.
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "matched");
}

#[tokio::test]
async fn host_matching_respects_explicit_port() {
    // Given: an explicit-port Host requirement with a fallback sibling.
    let api = alt(
        host(
            "api.example.com:8443",
            path("status", get::<(PlainText,), String>()),
        ),
        path("status", get::<(PlainText,), String>()),
    );
    let svc = RouterService::new(serve(
        api,
        (
            || async { Ok::<_, ServerError>("port".to_string()) },
            || async { Ok::<_, ServerError>("fallback".to_string()) },
        ),
    ));

    // When/Then: the same explicit port matches.
    let (status, body) = call(&svc, get_with_host("/status", "API.EXAMPLE.COM:8443")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "port");

    // When/Then: a different explicit port falls through.
    let (status, body) = call(&svc, get_with_host("/status", "api.example.com:443")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "fallback");
}

#[tokio::test]
async fn host_without_port_ignores_request_port() {
    // Given: a portless Host requirement.
    let svc = RouterService::new(serve(
        host(
            "api.example.com",
            path("status", get::<(PlainText,), String>()),
        ),
        || async { Ok::<_, ServerError>("matched".to_string()) },
    ));

    // When: the request supplies any explicit port.
    let (status, body) = call(&svc, get_with_host("/status", "api.example.com:9443")).await;

    // Then: the request port is ignored.
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "matched");
}

#[tokio::test]
async fn host_uses_uri_authority_when_host_header_absent() {
    // Given: a portless Host requirement and an absolute-form request URI.
    let svc = RouterService::new(serve(
        host(
            "api.example.com",
            path("status", get::<(PlainText,), String>()),
        ),
        || async { Ok::<_, ServerError>("matched".to_string()) },
    ));
    let req = Request::builder()
        .method("GET")
        .uri("http://api.example.com:9443/status")
        .body(Full::new(Bytes::new()))
        .unwrap();

    // When: there is no Host header, but the URI carries an authority.
    let (status, body) = call(&svc, req).await;

    // Then: the authority is used as the Host source.
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "matched");
}

#[tokio::test]
async fn malformed_host_header_falls_through() {
    // Given: an explicit-port Host requirement with a generic fallback sibling.
    let api = alt(
        host(
            "api.example.com:8443",
            path("status", get::<(PlainText,), String>()),
        ),
        path("status", get::<(PlainText,), String>()),
    );
    let svc = RouterService::new(serve(
        api,
        (
            || async { Ok::<_, ServerError>("host".to_string()) },
            || async { Ok::<_, ServerError>("fallback".to_string()) },
        ),
    ));

    // When: the Host header has an invalid authority port.
    let (status, body) = call(&svc, get_with_host("/status", "api.example.com:notaport")).await;

    // Then: the malformed host is a recoverable mismatch.
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "fallback");
}
