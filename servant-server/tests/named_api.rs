//! `#[derive(NamedApi)]`: a struct of endpoint fields becomes an `Alt` tree
//! (`into_api`) plus a name-keyed handler record (`into_handlers`), so routes
//! and handlers are bound by field name rather than by manual nesting.

use bytes::Bytes;
use http::StatusCode;
use http_body_util::{BodyExt, Full};
use serde::{Deserialize, Serialize};
use servant::NamedApi;
use servant::api::{Capture, Path, Verb};
use servant::method::{Get, Post};
use servant::prelude::*;
use servant_server::{RouterService, serve};

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct User {
    id: u64,
}

#[derive(Serialize, Deserialize)]
struct NewUser {
    name: String,
}

// Endpoint type aliases keep the record field types readable.
type GetUser = Path<Capture<u64, Strict, Verb<Get, 200, (Json,), User>>>;
type CreateUser =
    Path<servant::api::ReqBody<(Json,), NewUser, Strict, Verb<Post, 201, (Json,), User>>>;
type Health = Path<Verb<Get, 200, (PlainText,), String>>;

#[derive(NamedApi)]
struct Api {
    get_user: GetUser,
    create_user: CreateUser,
    health: Health,
}

fn the_api() -> Api {
    Api {
        get_user: path("users", capture::<u64, _>("id", get::<(Json,), User>())),
        create_user: path(
            "users",
            req_body::<(Json,), NewUser, _>(verb::<Post, 201, (Json,), User>()),
        ),
        health: path("health", get::<(PlainText,), String>()),
    }
}

#[tokio::test]
async fn named_api_serves_via_into_api_and_into_handlers() {
    let handlers = ApiHandlers {
        get_user: |id: u64| async move { Ok::<_, ServerError>(User { id }) },
        create_user: |b: NewUser| async move {
            let _ = b;
            Ok::<_, ServerError>(User { id: 1 })
        },
        health: || async { Ok::<_, ServerError>("ok".to_string()) },
    };
    let router = serve(the_api().into_api(), handlers.into_handlers());
    let svc = RouterService::new(router);

    let get = |uri: &str| {
        http::Request::builder()
            .method("GET")
            .uri(uri)
            .header("accept", "application/json")
            .body(Full::new(Bytes::new()))
            .unwrap()
    };

    let resp = svc.handle(get("/users/42")).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], br#"{"id":42}"#);

    let resp = svc
        .handle(
            http::Request::builder()
                .method("GET")
                .uri("/health")
                .header("accept", "text/plain")
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], b"ok");
}
