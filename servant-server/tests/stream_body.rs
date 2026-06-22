use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bytes::Bytes;
use futures_util::StreamExt;
use http::StatusCode;
use http_body_util::Full;
use servant::auth::{BasicAuthData, BasicAuthResult};
use servant::prelude::*;
use servant::stream::StreamBodyError;
use servant_server::extract::RequestBodyMode;
use servant_server::{
    BasicAuthCheck,
    Context,
    RouterService,
    ServerChain,
    serve,
    serve_with_context,
};
use tokio::sync::watch;

#[path = "support/stream_body.rs"]
mod stream_body_support;

use stream_body_support::{ChannelBody, collect_text, stream_sum_service};

struct User;

include!("support/stream_body_host_tests.rs");

#[tokio::test]
async fn stream_body_delayed_chunks_sum_without_prebuffer() {
    // Given: a streaming request body whose second frame is withheld until the
    // handler proves it observed the first decoded item.
    let (seen_tx, mut seen_rx) = watch::channel(0usize);
    let service = stream_sum_service(seen_tx);
    let (body_tx, body, probe) = ChannelBody::new();
    let request = http::Request::builder()
        .method("POST")
        .uri("/sum")
        .header("content-type", "application/json")
        .header("accept", "text/plain")
        .body(body)
        .expect("test request builds");

    // When: the first framed item is sent through the real RouterService path.
    let task = tokio::spawn(async move { service.handle(request).await });
    body_tx
        .send(Ok(Bytes::from_static(b"1:2,")))
        .await
        .expect("first body chunk sends");
    tokio::time::timeout(std::time::Duration::from_millis(200), seen_rx.changed())
        .await
        .expect("handler should see the first item before the whole body is available")
        .expect("watch channel remains open");

    // Then: the handler was handed a live stream before the request body ended.
    assert_eq!(*seen_rx.borrow(), 1);
    assert!(probe.polls() > 0);

    body_tx
        .send(Ok(Bytes::from_static(b"1:3,")))
        .await
        .expect("second body chunk sends");
    drop(body_tx);
    let (status, body) = collect_text(task.await.expect("request task completes")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "5");
    assert_eq!(probe.drops(), 1);
}

#[tokio::test]
async fn stream_body_basic_auth_401_before_body_handoff() {
    // Given: auth is nested after StreamBody, and the request has no credentials.
    let handler_calls = Arc::new(AtomicUsize::new(0));
    let api = path(
        "sum",
        stream_body::<NetstringFraming, Json, u64, _>(basic_auth::<User, _>(
            "servant",
            verb::<Post, 200, (PlainText,), String>(),
        )),
    );
    let context = Context::new().with(BasicAuthCheck::new(|_data: &BasicAuthData| {
        BasicAuthResult::Authorized(User)
    }));
    let service = RouterService::new(serve_with_context(
        api,
        {
            let handler_calls = handler_calls.clone();
            move |_body: SourceStream<Result<u64, StreamBodyError>>, _user: User| {
                handler_calls.fetch_add(1, Ordering::SeqCst);
                async { Ok::<_, ServerError>("unreachable".to_string()) }
            }
        },
        context,
    ));
    let (_body_tx, body, probe) = ChannelBody::new();
    let request = http::Request::builder()
        .method("POST")
        .uri("/sum")
        .header("content-type", "application/json")
        .header("accept", "text/plain")
        .body(body)
        .expect("test request builds");

    // When: the request is served.
    let response = service.handle(request).await;

    // Then: auth rejects before the handler receives or polls the body.
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(handler_calls.load(Ordering::SeqCst), 0);
    assert_eq!(probe.polls(), 0);
}

#[tokio::test]
async fn stream_body_wrong_content_type_returns_415_before_handoff() {
    // Given: a StreamBody endpoint and a request with the wrong Content-Type.
    let handler_calls = Arc::new(AtomicUsize::new(0));
    let api = path(
        "sum",
        stream_body::<NetstringFraming, Json, u64, _>(verb::<Post, 200, (PlainText,), String>()),
    );
    let service = RouterService::new(serve(api, {
        let handler_calls = handler_calls.clone();
        move |_body: SourceStream<Result<u64, StreamBodyError>>| {
            handler_calls.fetch_add(1, Ordering::SeqCst);
            async { Ok::<_, ServerError>("unreachable".to_string()) }
        }
    }));
    let (_body_tx, body, probe) = ChannelBody::new();
    let request = http::Request::builder()
        .method("POST")
        .uri("/sum")
        .header("content-type", "text/plain")
        .header("accept", "text/plain")
        .body(body)
        .expect("test request builds");

    // When: the request is served.
    let response = service.handle(request).await;

    // Then: 415 is returned before handler handoff or body polling.
    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(handler_calls.load(Ordering::SeqCst), 0);
    assert_eq!(probe.polls(), 0);
}

#[tokio::test]
async fn stream_body_malformed_netstring_surfaces_item_error() {
    // Given: malformed netstring bytes after content-type acceptance.
    let api = path(
        "sum",
        stream_body::<NetstringFraming, Json, u64, _>(verb::<Post, 200, (PlainText,), String>()),
    );
    let service = RouterService::new(serve(
        api,
        |body: SourceStream<Result<u64, StreamBodyError>>| async move {
            let mut stream = body.into_inner();
            match stream.next().await {
                Some(Err(error)) => Ok::<_, ServerError>(format!("stream-error:{error}")),
                other => Ok::<_, ServerError>(format!("unexpected:{other:?}")),
            }
        },
    ));
    let request = http::Request::builder()
        .method("POST")
        .uri("/sum")
        .header("content-type", "application/json")
        .header("accept", "text/plain")
        .body(Full::new(Bytes::from_static(b"x:")))
        .expect("test request builds");

    // When: the handler polls the body stream.
    let (status, body) = collect_text(service.handle(request).await).await;

    // Then: the malformed frame is an item error after handoff, not a 400/415.
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("stream-error:"));
    assert!(body.contains("malformed"));
}

#[tokio::test]
async fn stream_body_oversized_frame_surfaces_item_error() {
    // Given: a netstring length above the default 8 MiB decoded-frame cap.
    let api = path(
        "sum",
        stream_body::<NetstringFraming, Json, u64, _>(verb::<Post, 200, (PlainText,), String>()),
    );
    let service = RouterService::new(serve(
        api,
        |body: SourceStream<Result<u64, StreamBodyError>>| async move {
            let mut stream = body.into_inner();
            match stream.next().await {
                Some(Err(error)) => Ok::<_, ServerError>(format!("stream-error:{error}")),
                other => Ok::<_, ServerError>(format!("unexpected:{other:?}")),
            }
        },
    ));
    let request = http::Request::builder()
        .method("POST")
        .uri("/sum")
        .header("content-type", "application/json")
        .header("accept", "text/plain")
        .body(Full::new(Bytes::from_static(b"8388609:")))
        .expect("test request builds");

    // When: the handler polls the body stream.
    let (status, body) = collect_text(service.handle(request).await).await;

    // Then: the oversized frame is an item error after handoff.
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("stream-error:"));
    assert!(body.contains("exceeded"));
}

#[tokio::test]
async fn stream_body_empty_stream_finishes_without_item() {
    // Given: a StreamBody endpoint and an empty accepted request stream.
    let api = path(
        "sum",
        stream_body::<NetstringFraming, Json, u64, _>(verb::<Post, 200, (PlainText,), String>()),
    );
    let service = RouterService::new(serve(
        api,
        |body: SourceStream<Result<u64, StreamBodyError>>| async move {
            let mut stream = body.into_inner();
            match stream.next().await {
                Some(Ok(value)) => Ok::<_, ServerError>(format!("unexpected:{value}")),
                Some(Err(error)) => Ok::<_, ServerError>(format!("stream-error:{error}")),
                None => Ok::<_, ServerError>("empty".to_string()),
            }
        },
    ));
    let request = http::Request::builder()
        .method("POST")
        .uri("/sum")
        .header("content-type", "application/json")
        .header("accept", "text/plain")
        .body(Full::new(Bytes::new()))
        .expect("test request builds");

    // When: the handler polls the body stream.
    let (status, body) = collect_text(service.handle(request).await).await;

    // Then: the stream terminates normally without an item error.
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "empty");
}

#[tokio::test]
async fn stream_body_alternatives_do_not_double_consume_body() {
    // Given: a same-path left alternative that fails on method before the right
    // StreamBody route is selected.
    let api = alt(
        path("sum", get::<(PlainText,), String>()),
        path(
            "sum",
            stream_body::<NetstringFraming, Json, u64, _>(verb::<Post, 200, (PlainText,), String>()),
        ),
    );
    let service = RouterService::new(serve(
        api,
        (
            || async { Ok::<_, ServerError>("wrong branch".to_string()) },
            |body: SourceStream<Result<u64, StreamBodyError>>| async move {
                let mut stream = body.into_inner();
                match stream.next().await {
                    Some(Ok(value)) => Ok::<_, ServerError>(format!("stream:{value}")),
                    Some(Err(error)) => Ok::<_, ServerError>(format!("stream-error:{error}")),
                    None => Ok::<_, ServerError>("empty".to_string()),
                }
            },
        ),
    ));
    let (body_tx, body, probe) = ChannelBody::new();
    let request = http::Request::builder()
        .method("POST")
        .uri("/sum")
        .header("content-type", "application/json")
        .header("accept", "text/plain")
        .body(body)
        .expect("test request builds");

    // When: the right streaming alternative receives the body.
    body_tx
        .send(Ok(Bytes::from_static(b"2:42,")))
        .await
        .expect("body chunk sends");
    drop(body_tx);
    let (status, body) = collect_text(service.handle(request).await).await;

    // Then: the one-shot body is consumed once by the selected alternative.
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "stream:42");
    assert!(probe.polls() > 0);
    assert_eq!(probe.drops(), 1);
}
