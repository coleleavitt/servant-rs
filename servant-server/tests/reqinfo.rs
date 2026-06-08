//! Request-info combinators (`IsSecure`/`HttpVersion`/`RemoteHost`),
//! `AuthProtect`, and `WithNamedContext`.

use std::net::SocketAddr;

use bytes::Bytes;
use http::StatusCode;
use http_body_util::{BodyExt, Full};
use servant::prelude::*;
use servant_server::{
    AuthCheck,
    ConnectionInfo,
    Context,
    NamedContext,
    RouterService,
    serve,
    serve_with_context,
};

async fn run(svc: &RouterService, req: http::Request<Full<Bytes>>) -> (StatusCode, String) {
    let resp = svc.handle(req).await;
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

#[tokio::test]
async fn http_version_is_exposed() {
    let api = path("v", http_version(get::<(PlainText,), String>()));
    let svc = RouterService::new(serve(api, |v: http::Version| async move {
        Ok::<_, ServerError>(format!("{v:?}"))
    }));
    let req = http::Request::builder()
        .method("GET")
        .uri("/v")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let (status, body) = run(&svc, req).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("HTTP"), "version: {body}");
}

#[tokio::test]
async fn is_secure_and_remote_host_from_connection_info() {
    let api = path(
        "info",
        is_secure(remote_host(get::<(PlainText,), String>())),
    );
    let svc = RouterService::new(serve(
        api,
        |secure: bool, addr: Option<SocketAddr>| async move {
            Ok::<_, ServerError>(format!("{secure}/{addr:?}"))
        },
    ));

    // No ConnectionInfo -> not secure, no addr.
    let req = http::Request::builder()
        .method("GET")
        .uri("/info")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let (_, body) = run(&svc, req).await;
    assert_eq!(body, "false/None");

    // With ConnectionInfo injected (as a serving layer would).
    let addr: SocketAddr = "10.0.0.1:443".parse().unwrap();
    let mut req = http::Request::builder()
        .method("GET")
        .uri("/info")
        .body(Full::new(Bytes::new()))
        .unwrap();
    req.extensions_mut().insert(ConnectionInfo {
        remote_addr: Some(addr),
        secure: true,
    });
    let (_, body) = run(&svc, req).await;
    assert_eq!(body, "true/Some(10.0.0.1:443)");
}

#[derive(Clone)]
struct User;

#[tokio::test]
async fn auth_protect_uses_context_check() {
    let api = path("me", auth_protect::<User, _>(get::<(PlainText,), String>()));
    let check = AuthCheck::<User>::new(|h| {
        if h.get("x-token").map(|v| v == "letmein").unwrap_or(false) {
            Ok(User)
        } else {
            Err(ServerError::err403().with_body("nope"))
        }
    });
    let svc = RouterService::new(serve_with_context(
        api,
        |_u: User| async { Ok::<_, ServerError>("ok".to_string()) },
        Context::new().with(check),
    ));

    let bad = http::Request::builder()
        .method("GET")
        .uri("/me")
        .body(Full::new(Bytes::new()))
        .unwrap();
    assert_eq!(run(&svc, bad).await.0, StatusCode::FORBIDDEN);

    let good = http::Request::builder()
        .method("GET")
        .uri("/me")
        .header("x-token", "letmein")
        .body(Full::new(Bytes::new()))
        .unwrap();
    assert_eq!(run(&svc, good).await, (StatusCode::OK, "ok".into()));
}

struct AdminScope;

#[tokio::test]
async fn with_named_context_selects_sub_context() {
    // The AuthCheck lives ONLY in the named "admin" sub-context, so the inner
    // AuthProtect resolves it via WithNamedContext.
    let api = path(
        "admin",
        with_named_context::<AdminScope, _>(auth_protect::<User, _>(get::<(PlainText,), String>())),
    );
    let admin_ctx = Context::new().with(AuthCheck::<User>::new(|h| {
        if h.contains_key("x-admin") {
            Ok(User)
        } else {
            Err(ServerError::err401())
        }
    }));
    let ctx = Context::new().with(NamedContext::<AdminScope>::new(admin_ctx));
    let svc = RouterService::new(serve_with_context(
        api,
        |_u: User| async { Ok::<_, ServerError>("admin ok".to_string()) },
        ctx,
    ));

    let no_hdr = http::Request::builder()
        .method("GET")
        .uri("/admin")
        .body(Full::new(Bytes::new()))
        .unwrap();
    assert_eq!(run(&svc, no_hdr).await.0, StatusCode::UNAUTHORIZED);

    let with_hdr = http::Request::builder()
        .method("GET")
        .uri("/admin")
        .header("x-admin", "1")
        .body(Full::new(Bytes::new()))
        .unwrap();
    assert_eq!(
        run(&svc, with_hdr).await,
        (StatusCode::OK, "admin ok".into())
    );
}
