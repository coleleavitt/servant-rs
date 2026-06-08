//! End-to-end server tests: one API description, routed and served, exercising
//! captures, content negotiation, the `Fail`/`FailFatal` distinction, and the
//! best-error priority selection.

use bytes::Bytes;
use http::{Request, StatusCode};
use http_body_util::{BodyExt, Full};
use serde::{Deserialize, Serialize};
use servant::prelude::*;
use servant_server::{RouterService, serve};

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
struct User {
    id: u64,
    name: String,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
struct NewUser {
    name: String,
}

fn test_service() -> RouterService {
    // GET  /users/<id>           -> User (JSON)
    // POST /users  (JSON body)   -> User (JSON), 201
    // GET  /hello                -> String (text/plain)
    // GET  /search?q=<required>  -> String (JSON)
    let api = alt(
        path("users", capture::<u64, _>("id", get::<(Json,), User>())),
        alt(
            path(
                "users",
                req_body::<(Json,), NewUser, _>(verb::<Post, 201, (Json,), User>()),
            ),
            alt(
                path("hello", get::<(PlainText,), String>()),
                path(
                    "search",
                    query_param::<String, _>("q", get::<(Json,), String>()).required(),
                ),
            ),
        ),
    );

    let get_user = |id: u64| async move {
        Ok::<_, ServerError>(User {
            id,
            name: format!("user{id}"),
        })
    };
    let create_user = |body: NewUser| async move {
        Ok::<_, ServerError>(User {
            id: 1,
            name: body.name,
        })
    };
    let hello = || async { Ok::<_, ServerError>("world".to_string()) };
    let search = |q: String| async move { Ok::<_, ServerError>(format!("you searched {q}")) };

    let router = serve(api, (get_user, (create_user, (hello, search))));
    RouterService::new(router)
}

async fn run(req: Request<Full<Bytes>>) -> (StatusCode, String, Option<String>) {
    let svc = test_service();
    let resp = svc.handle(req).await;
    let status = resp.status();
    let ct = resp
        .headers()
        .get(http::header::CONTENT_TYPE)
        .map(|v| v.to_str().unwrap().to_string());
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned(), ct)
}

fn get_req(uri: &str) -> Request<Full<Bytes>> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .body(Full::new(Bytes::new()))
        .unwrap()
}

#[tokio::test]
async fn get_with_capture_returns_json() {
    let (status, body, ct) = run(get_req("/users/42")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#"{"id":42,"name":"user42"}"#);
    assert_eq!(ct.as_deref(), Some("application/json"));
}

#[tokio::test]
async fn unparseable_capture_is_bad_request() {
    // /users/abc: GET-capture route fails to parse (400, recoverable);
    // POST route 404s (path not empty). 400 beats 404 by priority.
    let (status, _, _) = run(get_req("/users/abc")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn post_with_json_body_creates() {
    let req = Request::builder()
        .method("POST")
        .uri("/users")
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(r#"{"name":"alice"}"#)))
        .unwrap();
    let svc = test_service();
    let resp = svc.handle(req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], br#"{"id":1,"name":"alice"}"#);
}

#[tokio::test]
async fn wrong_method_is_405() {
    // POST /users/42 -> GET-capture route says 405; sibling 404s. 405 > 404.
    let req = Request::builder()
        .method("POST")
        .uri("/users/42")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let (status, _, _) = run(req).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn unsupported_accept_is_406() {
    let req = Request::builder()
        .method("GET")
        .uri("/hello")
        .header("accept", "application/json")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let (status, _, _) = run(req).await;
    assert_eq!(status, StatusCode::NOT_ACCEPTABLE);
}

#[tokio::test]
async fn unsupported_content_type_is_415() {
    let req = Request::builder()
        .method("POST")
        .uri("/users")
        .header("content-type", "text/plain")
        .body(Full::new(Bytes::from("hi")))
        .unwrap();
    let (status, _, _) = run(req).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn missing_required_query_is_400() {
    let (status, _, _) = run(get_req("/search")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn required_query_present_succeeds() {
    let (status, body, _) = run(get_req("/search?q=rust")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#""you searched rust""#);
}

#[tokio::test]
async fn unknown_path_is_404() {
    let (status, _, _) = run(get_req("/nope")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn hello_plain_text() {
    let (status, body, ct) = run(get_req("/hello")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "world");
    assert_eq!(ct.as_deref(), Some("text/plain; charset=utf-8"));
}

#[tokio::test]
async fn head_is_served_by_get_without_body() {
    let req = Request::builder()
        .method("HEAD")
        .uri("/hello")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let svc = test_service();
    let resp = svc.handle(req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert!(body.is_empty());
}

#[tokio::test]
async fn trailing_slash_matches() {
    let (status, _, _) = run(get_req("/hello/")).await;
    assert_eq!(status, StatusCode::OK);
}
