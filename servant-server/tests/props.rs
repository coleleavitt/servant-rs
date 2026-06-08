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
}
