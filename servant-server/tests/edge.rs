//! Focused edge-case behaviors that mirror specific Servant semantics:
//! lenient parsing, duplicate query keys (first wins), malformed bodies,
//! optional-absent, and lenient query parse surfacing.

use bytes::Bytes;
use http::StatusCode;
use http_body_util::{BodyExt, Full};
use serde::{Deserialize, Serialize};
use servant::modifiers::ParseError;
use servant::prelude::*;
use servant_server::{RouterService, serve};

#[derive(Serialize, Deserialize)]
struct Item {
    n: u32,
}

async fn call(svc: &RouterService, req: http::Request<Full<Bytes>>) -> (StatusCode, String) {
    let resp = svc.handle(req).await;
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

fn get_req(uri: &str) -> http::Request<Full<Bytes>> {
    http::Request::builder()
        .method("GET")
        .uri(uri)
        .body(Full::new(Bytes::new()))
        .unwrap()
}

#[tokio::test]
async fn lenient_capture_surfaces_parse_error_to_handler() {
    // Capture' '[Lenient]: a bad parse is delivered as Err, not a 400.
    let api = path(
        "c",
        capture_lenient::<u32, _>("n", get::<(PlainText,), String>()),
    );
    let svc = RouterService::new(serve(api, |n: Result<u32, ParseError>| async move {
        Ok::<_, ServerError>(match n {
            Ok(v) => format!("ok{v}"),
            Err(_) => "err".to_string(),
        })
    }));

    assert_eq!(
        call(&svc, get_req("/c/5")).await,
        (StatusCode::OK, "ok5".into())
    );
    assert_eq!(
        call(&svc, get_req("/c/abc")).await,
        (StatusCode::OK, "err".into())
    );
}

#[tokio::test]
async fn duplicate_query_key_first_wins_for_scalar() {
    // QueryParam takes the first matching value (Haskell `lookup`).
    let api = path(
        "q",
        query_param::<String, _>("x", get::<(PlainText,), String>()),
    );
    let svc = RouterService::new(serve(api, |x: Option<String>| async move {
        Ok::<_, ServerError>(x.unwrap_or_else(|| "none".into()))
    }));

    assert_eq!(
        call(&svc, get_req("/q?x=a&x=b")).await,
        (StatusCode::OK, "a".into())
    );
    // optional absent -> None
    assert_eq!(
        call(&svc, get_req("/q")).await,
        (StatusCode::OK, "none".into())
    );
}

#[tokio::test]
async fn query_params_collects_all_values_in_order() {
    let api = path(
        "q",
        query_params::<String, _>("x", get::<(Json,), Vec<String>>()),
    );
    let svc = RouterService::new(serve(api, |xs: Vec<String>| async move {
        Ok::<_, ServerError>(xs)
    }));
    assert_eq!(
        call(&svc, get_req("/q?x=a&x=b&x=c")).await,
        (StatusCode::OK, r#"["a","b","c"]"#.into())
    );
}

#[tokio::test]
async fn malformed_json_body_is_400() {
    let api = path("b", req_body::<(Json,), Item, _>(post::<(Json,), Item>()));
    let svc = RouterService::new(serve(api, |item: Item| async move {
        Ok::<_, ServerError>(item)
    }));
    let req = http::Request::builder()
        .method("POST")
        .uri("/b")
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from("{ this is not json")))
        .unwrap();
    let (status, _) = call(&svc, req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn handler_returned_error_becomes_that_response() {
    // A handler error is a real response (not Fail/FailFatal): custom status+body.
    let api = path("teapot", get::<(PlainText,), String>());
    let svc = RouterService::new(serve(api, || async {
        Err::<String, _>(ServerError::err418().with_body("no coffee"))
    }));
    let (status, body) = call(&svc, get_req("/teapot")).await;
    assert_eq!(status, StatusCode::IM_A_TEAPOT);
    assert_eq!(body, "no coffee");
}

#[tokio::test]
async fn quality_zero_accept_is_not_acceptable() {
    // Accept: application/json;q=0 means "not acceptable".
    let api = path("j", get::<(Json,), u32>());
    let svc = RouterService::new(serve(api, || async { Ok::<_, ServerError>(1u32) }));
    let req = http::Request::builder()
        .method("GET")
        .uri("/j")
        .header("accept", "application/json;q=0")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let (status, _) = call(&svc, req).await;
    assert_eq!(status, StatusCode::NOT_ACCEPTABLE);
}

#[tokio::test]
async fn query_flag_is_value_sensitive() {
    // ?flag / =true / =1 / = -> true; =false / =0 -> false; absent -> false.
    let api = path("f", query_flag("on", get::<(PlainText,), String>()));
    let svc = RouterService::new(serve(api, |on: bool| async move {
        Ok::<_, ServerError>(on.to_string())
    }));
    for (q, want) in [
        ("/f?on", "true"),
        ("/f?on=true", "true"),
        ("/f?on=1", "true"),
        ("/f?on=", "true"),
        ("/f?on=false", "false"),
        ("/f?on=0", "false"),
        ("/f", "false"),
    ] {
        assert_eq!(
            call(&svc, get_req(q)).await,
            (StatusCode::OK, want.into()),
            "{q}"
        );
    }
}

#[tokio::test]
async fn bare_query_key_is_absent_not_empty() {
    // Optional u32: `?x` (bare) -> absent (None); `?x=5` -> 5; `?x=` -> parse "" fails -> 400.
    let api = path(
        "q",
        query_param::<u32, _>("x", get::<(PlainText,), String>()),
    );
    let svc = RouterService::new(serve(api, |x: Option<u32>| async move {
        Ok::<_, ServerError>(x.map(|v| v.to_string()).unwrap_or_else(|| "none".into()))
    }));
    assert_eq!(
        call(&svc, get_req("/q?x")).await,
        (StatusCode::OK, "none".into())
    );
    assert_eq!(
        call(&svc, get_req("/q?x=5")).await,
        (StatusCode::OK, "5".into())
    );
    assert_eq!(
        call(&svc, get_req("/q?x=")).await.0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn query_params_accepts_bracketed_form_and_skips_bare_keys() {
    let api = path("q", query_params::<u32, _>("x", get::<(Json,), Vec<u32>>()));
    let svc = RouterService::new(serve(api, |xs: Vec<u32>| async move {
        Ok::<_, ServerError>(xs)
    }));
    assert_eq!(
        call(&svc, get_req("/q?x[]=1&x[]=2")).await,
        (StatusCode::OK, "[1,2]".into())
    );
    // a bare `x` (no value) is dropped; values still collected
    assert_eq!(
        call(&svc, get_req("/q?x&x=3")).await,
        (StatusCode::OK, "[3]".into())
    );
}

#[tokio::test]
async fn error_bodies_do_not_echo_query_values() {
    // A required query param that fails to parse must not leak its raw value.
    let api = path(
        "q",
        query_param::<u32, _>("token", get::<(Json,), u32>()).required(),
    );
    let svc = RouterService::new(serve(api, |t: u32| async move { Ok::<_, ServerError>(t) }));
    let (status, body) = call(&svc, get_req("/q?token=sk-secret-123")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(!body.contains("sk-secret-123"), "leaked value: {body}");
}
