use bytes::Bytes;
use http::StatusCode;
use http_body_util::{BodyExt, Full};
use serde::{Deserialize, Serialize};
use servant::prelude::*;
use servant::query::Query;
use servant_server::{RouterService, serve};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct QueryEcho {
    raw: Option<String>,
    pairs: Vec<(String, Option<String>)>,
}

async fn call(svc: &RouterService, uri: &str) -> (StatusCode, QueryEcho) {
    let req = http::Request::builder()
        .method("GET")
        .uri(uri)
        .body(Full::new(Bytes::new()))
        .expect("test request builds");
    let resp = svc.handle(req).await;
    let status = resp.status();
    let body = resp
        .into_body()
        .collect()
        .await
        .expect("test response body collects")
        .to_bytes();
    let parsed = serde_json::from_slice(&body).expect("query echo decodes");
    (status, parsed)
}

fn service() -> RouterService {
    let api = path(
        "raw-query-string",
        query_string(get::<(Json,), QueryEcho>()),
    );
    let router = serve(api, |query: Query| async move {
        Ok::<_, ServerError>(QueryEcho {
            raw: query.raw().map(str::to_owned),
            pairs: query.pairs().to_vec(),
        })
    });
    RouterService::new(router)
}

#[tokio::test]
async fn query_string_preserves_raw_and_order() {
    let (status, echo) = call(
        &service(),
        "/raw-query-string?name=bob&name=alice&flag&encoded=%40",
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        echo,
        QueryEcho {
            raw: Some("name=bob&name=alice&flag&encoded=%40".to_string()),
            pairs: vec![
                ("name".to_string(), Some("bob".to_string())),
                ("name".to_string(), Some("alice".to_string())),
                ("flag".to_string(), None),
                ("encoded".to_string(), Some("@".to_string())),
            ],
        }
    );
}

#[tokio::test]
async fn query_string_distinguishes_bare_empty_and_invalid_percent() {
    let (status, echo) = call(&service(), "/raw-query-string?flag&empty=&bad=%ZZ&plus=a+b").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(echo.raw.as_deref(), Some("flag&empty=&bad=%ZZ&plus=a+b"));
    assert_eq!(
        echo.pairs,
        vec![
            ("flag".to_string(), None),
            ("empty".to_string(), Some(String::new())),
            ("bad".to_string(), Some("%ZZ".to_string())),
            ("plus".to_string(), Some("a b".to_string())),
        ]
    );
}
