//! Streaming endpoints: framed item streams and Server-Sent Events. The body is
//! produced incrementally; here we collect it and assert the framed wire form.

use bytes::Bytes;
use http::StatusCode;
use http_body_util::{BodyExt, Full};
use servant::prelude::*;
use servant_server::{RouterService, serve};

async fn body_of(
    svc: &RouterService,
    uri: &str,
    accept: &str,
) -> (StatusCode, Option<String>, String) {
    let req = http::Request::builder()
        .method("GET")
        .uri(uri)
        .header("accept", accept)
        .body(Full::new(Bytes::new()))
        .unwrap();
    let resp = svc.handle(req).await;
    let status = resp.status();
    let ct = resp
        .headers()
        .get(http::header::CONTENT_TYPE)
        .map(|v| v.to_str().unwrap().to_string());
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    (status, ct, String::from_utf8_lossy(&body).into_owned())
}

#[tokio::test]
async fn newline_framed_json_stream() {
    // "nums" :> StreamGet NewlineFraming JSON u32
    let api = path("nums", stream_get::<NewlineFraming, Json, u32>());
    let svc = RouterService::new(serve(api, || async {
        Ok::<_, ServerError>(SourceStream::new(futures_util::stream::iter(vec![
            1u32, 2, 3,
        ])))
    }));
    let (status, ct, body) = body_of(&svc, "/nums", "application/json").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ct.as_deref(), Some("application/json"));
    assert_eq!(body, "1\n2\n3\n");
}

#[tokio::test]
async fn netstring_framed_stream() {
    let api = path("nums", stream_get::<NetstringFraming, Json, u32>());
    let svc = RouterService::new(serve(api, || async {
        Ok::<_, ServerError>(SourceStream::new(futures_util::stream::iter(vec![
            10u32, 200,
        ])))
    }));
    let (_, _, body) = body_of(&svc, "/nums", "application/json").await;
    // 10 -> "10" (2 bytes), 200 -> "200" (3 bytes)
    assert_eq!(body, "2:10,3:200,");
}

#[tokio::test]
async fn server_sent_events() {
    // "events" :> ServerSentEvents
    let api = path("events", sse_get());
    let svc = RouterService::new(serve(api, || async {
        Ok::<_, ServerError>(SourceStream::new(futures_util::stream::iter(vec![
            ServerEvent::data("hello"),
            ServerEvent::data("world").with_event("greeting"),
        ])))
    }));
    let (status, ct, body) = body_of(&svc, "/events", "text/event-stream").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ct.as_deref(), Some("text/event-stream"));
    assert_eq!(body, "data: hello\n\nevent: greeting\ndata: world\n\n");
}

#[tokio::test]
async fn stream_negotiation_rejects_wrong_accept() {
    let api = path("nums", stream_get::<NewlineFraming, Json, u32>());
    let svc = RouterService::new(serve(api, || async {
        Ok::<_, ServerError>(SourceStream::new(futures_util::stream::iter(vec![1u32])))
    }));
    let (status, _, _) = body_of(&svc, "/nums", "text/plain").await;
    assert_eq!(status, StatusCode::NOT_ACCEPTABLE);
}
