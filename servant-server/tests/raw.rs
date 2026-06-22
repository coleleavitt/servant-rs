use std::sync::Arc;

use bytes::Bytes;
use http::{HeaderValue, Method, StatusCode};
use http_body_util::{BodyExt, Full};
use servant::prelude::*;
use servant_server::response::full_body;
use servant_server::{
    ConnectionInfo,
    Context,
    RawRequest,
    RouterService,
    serve,
    serve_with_context,
};

async fn collect_response(
    response: http::Response<servant_server::response::ResponseBody>,
) -> (StatusCode, http::HeaderMap, Bytes) {
    let status = response.status();
    let headers = response.headers().clone();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("test response body collects")
        .to_bytes();
    (status, headers, body)
}

#[tokio::test]
async fn raw_tail_query_headers_and_body_are_preserved() {
    // Given: a Raw endpoint under a path prefix with a handler that echoes the
    // request snapshot it receives.
    let api = path("foo", raw());
    let router = serve(api, |request: RawRequest| async move {
        assert_eq!(request.tail(), ["bar".to_string(), "a/b".to_string()]);
        assert_eq!(request.method(), Method::POST);
        assert_eq!(request.raw_query(), Some("flag&encoded=%40"));
        assert_eq!(
            request.query().pairs(),
            &[
                ("flag".to_string(), None),
                ("encoded".to_string(), Some("@".to_string()))
            ]
        );
        assert_eq!(
            request
                .headers()
                .get("x-raw-bin")
                .map(HeaderValue::as_bytes),
            Some(&b"\xffraw"[..])
        );
        assert_eq!(request.body(), &Bytes::from_static(b"\x00raw-body"));
        assert_eq!(request.version(), http::Version::HTTP_11);
        assert_eq!(
            request.remote_addr(),
            Some("127.0.0.1:4545".parse().expect("test socket address"))
        );
        assert!(request.is_secure());
        assert!(request.extensions().get::<ConnectionInfo>().is_some());

        http::Response::builder()
            .status(StatusCode::CREATED)
            .header("x-raw-out", "kept")
            .body(full_body(Bytes::from_static(b"raw-ok")))
            .expect("test raw response builds")
    });
    let service = RouterService::new(router);
    let req = http::Request::builder()
        .method(Method::POST)
        .uri("/foo/bar/a%2Fb?flag&encoded=%40")
        .header(
            "x-raw-bin",
            HeaderValue::from_bytes(b"\xffraw").expect("binary header"),
        )
        .extension(ConnectionInfo {
            remote_addr: Some("127.0.0.1:4545".parse().expect("test socket address")),
            secure: true,
        })
        .body(Full::new(Bytes::from_static(b"\x00raw-body")))
        .expect("test raw request builds");

    // When: the request is served through the real router adapter.
    let (status, headers, body) = collect_response(service.handle(req).await).await;

    // Then: Raw receives the stripped tail and preserves the raw response.
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        headers
            .get("x-raw-out")
            .and_then(|value| value.to_str().ok()),
        Some("kept")
    );
    assert_eq!(body, Bytes::from_static(b"raw-ok"));
}

#[tokio::test]
async fn raw_does_not_shadow_left_biased_typed_sibling() {
    // Given: a typed route to the left of a Raw fallback under the same prefix.
    let api = alt(
        path("foo", path("typed", get::<(PlainText,), String>())),
        path("foo", raw()),
    );
    let service = RouterService::new(serve(
        api,
        (
            || async { Ok::<_, ServerError>("typed".to_string()) },
            |request: RawRequest| async move {
                http::Response::builder()
                    .status(StatusCode::ACCEPTED)
                    .body(full_body(Bytes::from(request.tail().join("/"))))
                    .expect("test raw response builds")
            },
        ),
    ));

    // When: the typed sibling matches exactly.
    let typed = http::Request::builder()
        .method(Method::GET)
        .uri("/foo/typed")
        .body(Full::new(Bytes::new()))
        .expect("test typed request builds");
    let (typed_status, _, typed_body) = collect_response(service.handle(typed).await).await;

    // Then: the typed sibling wins.
    assert_eq!(typed_status, StatusCode::OK);
    assert_eq!(typed_body, Bytes::from_static(b"typed"));

    // When: the higher-priority branch fails recoverably on method.
    let fallback = http::Request::builder()
        .method(Method::POST)
        .uri("/foo/typed/deeper")
        .body(Full::new(Bytes::new()))
        .expect("test fallback request builds");
    let (fallback_status, _, fallback_body) =
        collect_response(service.handle(fallback).await).await;

    // Then: Raw handles only the still-unmatched tail.
    assert_eq!(fallback_status, StatusCode::ACCEPTED);
    assert_eq!(fallback_body, Bytes::from_static(b"typed/deeper"));
}

#[tokio::test]
async fn nested_static_raw_prefix_owns_only_remaining_tail() {
    // Given: a Raw endpoint mounted below two static path prefixes.
    let api = path("api", path("files", raw()));
    let service = RouterService::new(serve(api, |request: RawRequest| async move {
        assert_eq!(request.tail(), ["a".to_string(), "b".to_string()]);
        assert_eq!(request.method(), Method::PUT);
        assert_eq!(request.raw_query(), Some("x=1&encoded=%2F"));
        assert_eq!(
            request
                .headers()
                .get("x-nested-raw")
                .and_then(|value| value.to_str().ok()),
            Some("kept")
        );
        assert_eq!(request.body(), &Bytes::from_static(b"nested body"));

        http::Response::builder()
            .status(StatusCode::CREATED)
            .header("x-nested-raw-out", "ok")
            .body(full_body(Bytes::from_static(b"nested raw ok")))
            .expect("test nested raw response builds")
    }));
    let req = http::Request::builder()
        .method(Method::PUT)
        .uri("/api/files/a/b?x=1&encoded=%2F")
        .header("x-nested-raw", "kept")
        .body(Full::new(Bytes::from_static(b"nested body")))
        .expect("test nested raw request builds");

    // When: the nested prefix matches.
    let (status, headers, body) = collect_response(service.handle(req).await).await;

    // Then: only the unconsumed tail is passed to Raw and the raw response wins.
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        headers
            .get("x-nested-raw-out")
            .and_then(|value| value.to_str().ok()),
        Some("ok")
    );
    assert_eq!(body, Bytes::from_static(b"nested raw ok"));
}

#[tokio::test]
async fn raw_m_reads_context_and_renders_server_errors() {
    // Given: a RawM endpoint with context access.
    let api = path("rawm", raw_m());
    let context = Context::new().with(String::from("ctx-value"));
    let service = RouterService::new(serve_with_context(
        api,
        |request: RawRequest, context: Arc<Context>| async move {
            if request.raw_query() == Some("fail") {
                return Err(ServerError::err418().with_body("rawm failed"));
            }
            let value = context
                .get::<String>()
                .map(String::as_str)
                .unwrap_or("missing");
            Ok(http::Response::builder()
                .status(StatusCode::OK)
                .body(full_body(Bytes::from(value.to_string())))
                .expect("test rawm response builds"))
        },
        context,
    ));

    // When: RawM succeeds.
    let ok = http::Request::builder()
        .method(Method::PATCH)
        .uri("/rawm/a/b")
        .body(Full::new(Bytes::new()))
        .expect("test rawm request builds");
    let (ok_status, _, ok_body) = collect_response(service.handle(ok).await).await;

    // Then: it can read the shared context and accept a non-GET method.
    assert_eq!(ok_status, StatusCode::OK);
    assert_eq!(ok_body, Bytes::from_static(b"ctx-value"));

    // When: RawM returns a ServerError.
    let fail = http::Request::builder()
        .method(Method::DELETE)
        .uri("/rawm?fail")
        .body(Full::new(Bytes::new()))
        .expect("test rawm failure request builds");
    let (fail_status, _, fail_body) = collect_response(service.handle(fail).await).await;

    // Then: the existing error renderer is used.
    assert_eq!(fail_status, StatusCode::IM_A_TEAPOT);
    assert_eq!(fail_body, Bytes::from_static(b"rawm failed"));
}

#[tokio::test]
async fn nested_static_raw_m_prefix_reads_context_and_renders_server_errors() {
    // Given: a RawM endpoint mounted below two static path prefixes.
    let api = path("api", path("rawm", raw_m()));
    let context = Context::new().with(String::from("nested-context"));
    let service = RouterService::new(serve_with_context(
        api,
        |request: RawRequest, context: Arc<Context>| async move {
            if request.raw_query() == Some("fail=true") {
                return Err(ServerError::err418().with_body("nested rawm failed"));
            }
            let value = context
                .get::<String>()
                .map(String::as_str)
                .unwrap_or("missing");
            Ok(http::Response::builder()
                .status(StatusCode::OK)
                .body(full_body(Bytes::from(format!(
                    "{value}:{}",
                    request.tail().join("/")
                ))))
                .expect("test nested rawm response builds"))
        },
        context,
    ));

    // When: RawM succeeds below the nested prefix.
    let ok = http::Request::builder()
        .method(Method::PATCH)
        .uri("/api/rawm/a%2Fb/z?unusual&x=%2F")
        .body(Full::new(Bytes::from_static(b"rawm body")))
        .expect("test nested rawm request builds");
    let (ok_status, _, ok_body) = collect_response(service.handle(ok).await).await;

    // Then: it can read context and sees the decoded tail after the prefix.
    assert_eq!(ok_status, StatusCode::OK);
    assert_eq!(ok_body, Bytes::from_static(b"nested-context:a/b/z"));

    // When: RawM returns a ServerError below the nested prefix.
    let fail = http::Request::builder()
        .method(Method::DELETE)
        .uri("/api/rawm?fail=true")
        .body(Full::new(Bytes::new()))
        .expect("test nested rawm failure request builds");
    let (fail_status, _, fail_body) = collect_response(service.handle(fail).await).await;

    // Then: the existing ServerError renderer is still used.
    assert_eq!(fail_status, StatusCode::IM_A_TEAPOT);
    assert_eq!(fail_body, Bytes::from_static(b"nested rawm failed"));
}
