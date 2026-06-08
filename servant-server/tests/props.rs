//! Property tests for routing/extraction stability (testing rule: property
//! tests for routing and client/server round trips).

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use proptest::prelude::*;
use servant::prelude::*;
use servant_server::{RouterService, request, serve};

fn id_router() -> RouterService {
    // GET /n/<id> -> id (JSON)
    let api = path("n", capture::<u64, _>("id", get::<(Json,), u64>()));
    RouterService::new(serve(
        api,
        |id: u64| async move { Ok::<_, ServerError>(id) },
    ))
}

// A small fixed API for path/method routing properties:
//   GET  /a         -> "a"     (plain text)
//   GET  /b/<id>    -> id      (json)
//   POST /c (json)  -> u64
fn fixed_router() -> RouterService {
    let api = alt(
        path("a", get::<(PlainText,), String>()),
        alt(
            path("b", capture::<u64, _>("id", get::<(Json,), u64>())),
            path("c", req_body::<(Json,), u64, _>(post::<(Json,), u64>())),
        ),
    );
    RouterService::new(serve(
        api,
        (
            || async { Ok::<_, ServerError>("a".to_string()) },
            (
                |id: u64| async move { Ok::<_, ServerError>(id) },
                |n: u64| async move { Ok::<_, ServerError>(n) },
            ),
        ),
    ))
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
}

proptest! {
    /// Any u64 captured from the path is parsed and returned unchanged: the
    /// router/extractor round-trips arbitrary capture values.
    #[test]
    fn capture_u64_round_trips(id in any::<u64>()) {
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let svc = id_router();
        let body = rt.block_on(async move {
            let req = http::Request::builder()
                .method("GET")
                .uri(format!("/n/{id}"))
                .header("accept", "application/json")
                .body(Full::new(Bytes::new()))
                .unwrap();
            let resp = svc.handle(req).await;
            prop_assert_eq!(resp.status(), http::StatusCode::OK);
            Ok(resp.into_body().collect().await.unwrap().to_bytes())
        })?;
        let expected = id.to_string();
        prop_assert_eq!(body.as_ref(), expected.as_bytes());
    }

    /// Query parsing preserves order and key/value pairs for url-safe inputs.
    #[test]
    fn query_parse_preserves_pairs(
        pairs in proptest::collection::vec(("[a-z]{1,8}", "[a-z0-9]{0,8}"), 0..6)
    ) {
        let q: String = pairs
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");
        let parsed = request::parse_query(if q.is_empty() { None } else { Some(&q) });
        let expected: Vec<(String, Option<String>)> = pairs
            .iter()
            .map(|(k, v)| (k.clone(), Some(v.clone())))
            .collect();
        prop_assert_eq!(parsed, expected);
    }

    /// A single-segment GET routes (200) iff the segment is a registered static
    /// leaf ("a"); every other segment is 404. (`b`/`c` need more path / a body.)
    #[test]
    fn unregistered_paths_are_404(seg in "[a-z]{1,10}") {
        let svc = fixed_router();
        let uri = format!("/{seg}");
        let status = rt().block_on(async move {
            let req = http::Request::builder()
                .method("GET")
                .uri(uri)
                .header("accept", "text/plain")
                .body(Full::new(Bytes::new()))
                .unwrap();
            svc.handle(req).await.status()
        });
        let expected = match seg.as_str() {
            "a" => http::StatusCode::OK,           // GET /a is a static leaf
            "c" => http::StatusCode::METHOD_NOT_ALLOWED, // /c exists but is POST-only
            _ => http::StatusCode::NOT_FOUND,      // incl. "b" (needs /b/<id>)
        };
        prop_assert_eq!(status, expected, "seg={}", seg);
    }

    /// Routing never panics on any target the `http` crate accepts as a URI.
    #[test]
    fn arbitrary_targets_never_panic(target in "/[a-zA-Z0-9/_.~-]{0,40}") {
        let svc = fixed_router();
        let _ = rt().block_on(async move {
            // Skip inputs the edge (`http`) rejects before they reach the router.
            let Ok(req) = http::Request::builder()
                .method("GET")
                .uri(&target)
                .body(Full::new(Bytes::new()))
            else {
                return http::StatusCode::OK;
            };
            svc.handle(req).await.status()
        });
    }
}

#[test]
fn wrong_method_on_static_leaf_is_405() {
    // /a is GET-only; POST /a -> 405 (path matches, method doesn't).
    let svc = fixed_router();
    let status = rt().block_on(async {
        let req = http::Request::builder()
            .method("POST")
            .uri("/a")
            .body(Full::new(Bytes::new()))
            .unwrap();
        svc.handle(req).await.status()
    });
    assert_eq!(status, http::StatusCode::METHOD_NOT_ALLOWED);
}
