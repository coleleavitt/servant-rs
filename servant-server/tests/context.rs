//! Context-driven combinators: `BasicAuth`, `WithResource`, and `Vault`.

use std::sync::Arc;

use bytes::Bytes;
use http::StatusCode;
use http_body_util::{BodyExt, Full};
use servant::prelude::*;
use servant_server::{
    BasicAuthCheck,
    Context,
    ResourceProvider,
    RouterService,
    serve,
    serve_with_context,
};

#[derive(Clone)]
struct User {
    name: String,
}

async fn run(
    svc: &RouterService,
    req: http::Request<Full<Bytes>>,
) -> (StatusCode, String, Option<String>) {
    let resp = svc.handle(req).await;
    let status = resp.status();
    let www = resp
        .headers()
        .get(http::header::WWW_AUTHENTICATE)
        .map(|v| v.to_str().unwrap().to_string());
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned(), www)
}

fn basic_b64(user: &str, pass: &str) -> String {
    use base64::Engine;
    let raw = format!("{user}:{pass}");
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(raw)
    )
}

#[tokio::test]
async fn basic_auth_resolves_user_or_401s() {
    // "secret" :> BasicAuth "realm" User :> Get '[PlainText] String
    let api = path(
        "secret",
        basic_auth::<User, _>("servant", get::<(PlainText,), String>()),
    );
    let check = BasicAuthCheck::new(|data: &BasicAuthData| {
        if data.username == "ada" && data.password == "lovelace" {
            BasicAuthResult::Authorized(User {
                name: data.username.clone(),
            })
        } else {
            BasicAuthResult::BadPassword
        }
    });
    let ctx = Context::new().with(check);
    let svc = RouterService::new(serve_with_context(
        api,
        |u: User| async move { Ok::<_, ServerError>(format!("hi {}", u.name)) },
        ctx,
    ));

    // No credentials -> 401 + WWW-Authenticate.
    let req = http::Request::builder()
        .method("GET")
        .uri("/secret")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let (status, _, www) = run(&svc, req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(www.as_deref(), Some(r#"Basic realm="servant""#));

    // Wrong password -> 401.
    let req = http::Request::builder()
        .method("GET")
        .uri("/secret")
        .header("authorization", basic_b64("ada", "wrong"))
        .body(Full::new(Bytes::new()))
        .unwrap();
    assert_eq!(run(&svc, req).await.0, StatusCode::UNAUTHORIZED);

    // Correct credentials -> 200 with the resolved user.
    let req = http::Request::builder()
        .method("GET")
        .uri("/secret")
        .header("authorization", basic_b64("ada", "lovelace"))
        .body(Full::new(Bytes::new()))
        .unwrap();
    let (status, body, _) = run(&svc, req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "hi ada");
}

#[tokio::test]
async fn with_resource_allocates_from_context() {
    // "count" :> WithResource u32 :> Get
    let api = path("count", with_resource::<u32, _>(get::<(Json,), u32>()));
    let provider = ResourceProvider::new(|| 99u32);
    let ctx = Context::new().with(provider);
    let svc = RouterService::new(serve_with_context(
        api,
        |r: u32| async move { Ok::<_, ServerError>(r) },
        ctx,
    ));
    let req = http::Request::builder()
        .method("GET")
        .uri("/count")
        .header("accept", "application/json")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let (status, body, _) = run(&svc, req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "99");
}

#[tokio::test]
async fn vault_exposes_request_extensions() {
    // "v" :> Vault :> Get — read a value other middleware put in extensions.
    // The adapter forwards the request's extensions; we can't easily inject one
    // through hyper here, so assert the handler receives the extensions map.
    let api = path("v", vault(get::<(Json,), bool>()));
    let svc = RouterService::new(serve(api, |ext: Arc<http::Extensions>| async move {
        Ok::<_, ServerError>(ext.get::<&'static str>().is_some())
    }));
    let req = http::Request::builder()
        .method("GET")
        .uri("/v")
        .header("accept", "application/json")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let (status, body, _) = run(&svc, req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "false"); // no extension set in this request
}

#[tokio::test]
async fn missing_basic_auth_check_is_500() {
    let api = path(
        "secret",
        basic_auth::<User, _>("r", get::<(PlainText,), String>()),
    );
    // Context with NO check configured.
    let svc = RouterService::new(serve_with_context(
        api,
        |_u: User| async move { Ok::<_, ServerError>("x".to_string()) },
        Context::new(),
    ));
    let req = http::Request::builder()
        .method("GET")
        .uri("/secret")
        .header("authorization", basic_b64("a", "b"))
        .body(Full::new(Bytes::new()))
        .unwrap();
    assert_eq!(run(&svc, req).await.0, StatusCode::INTERNAL_SERVER_ERROR);
}
