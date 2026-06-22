use bytes::Bytes;
use http::StatusCode;
use http_body_util::{BodyExt, Full};
use serde::{Deserialize, Serialize};
use servant::prelude::*;
use servant::query::{DeepQueryParams, FromDeepQuery, Query};
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

async fn call_text(svc: &RouterService, uri: &str) -> (StatusCode, String) {
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
    (
        status,
        String::from_utf8(body.to_vec()).expect("response body is utf-8"),
    )
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DeepEcho {
    age: u32,
    name: String,
    tags: Vec<String>,
    entries: Vec<(Vec<String>, Option<String>)>,
}

impl FromDeepQuery for DeepEcho {
    fn from_deep_query(params: &DeepQueryParams) -> Result<Self, ParseError> {
        let age = params
            .first_value(&["user", "age"])
            .ok_or_else(|| ParseError::new("missing user.age"))
            .and_then(u32::from_query_param)?;
        let name = params
            .first_value(&["user", "name"])
            .ok_or_else(|| ParseError::new("missing user.name"))?
            .to_string();
        let tags = params
            .values(&["tag"])
            .into_iter()
            .map(str::to_owned)
            .collect();
        let entries = params
            .entries()
            .iter()
            .map(|entry| {
                (
                    entry.path().segments().to_vec(),
                    entry.value().map(str::to_owned),
                )
            })
            .collect();

        Ok(DeepEcho {
            age,
            name,
            tags,
            entries,
        })
    }
}

fn deep_service() -> RouterService {
    let api = path(
        "deep-query",
        deep_query::<DeepEcho, _>("filter", get::<(Json,), DeepEcho>()),
    );
    let router = serve(api, |filter: DeepEcho| async move {
        Ok::<_, ServerError>(filter)
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

#[tokio::test]
async fn deep_query_extracts_nested_object() {
    let (status, body) = call_text(
        &deep_service(),
        "/deep-query?filter[user][age]=32&filter[user][name]=john&filter[tag]=rust&filter[tag]=haskell",
    )
    .await;
    let echo: DeepEcho = serde_json::from_str(&body).expect("deep query echo decodes");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(echo.age, 32);
    assert_eq!(echo.name, "john");
    assert_eq!(echo.tags, vec!["rust", "haskell"]);
    assert_eq!(
        echo.entries,
        vec![
            (
                vec!["user".to_string(), "age".to_string()],
                Some("32".to_string())
            ),
            (
                vec!["user".to_string(), "name".to_string()],
                Some("john".to_string())
            ),
            (vec!["tag".to_string()], Some("rust".to_string())),
            (vec!["tag".to_string()], Some("haskell".to_string())),
        ]
    );
}

#[tokio::test]
async fn deep_query_missing_field_returns_400() {
    let (status, body) = call_text(&deep_service(), "/deep-query?filter[user][age]=32").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("deep query parameter `filter`"));
}

#[tokio::test]
async fn deep_query_bad_scalar_returns_400() {
    let (status, body) = call_text(
        &deep_service(),
        "/deep-query?filter[user][age]=not-a-number&filter[user][name]=john",
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("deep query parameter `filter`"));
    assert!(!body.contains("not-a-number"));
}

#[tokio::test]
async fn deep_query_malformed_bracket_returns_400_without_value_echo() {
    let (status, body) = call_text(
        &deep_service(),
        "/deep-query?filter[user][age=hacked-secret&filter[user][name]=john",
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("deep query parameter `filter`"));
    assert!(!body.contains("hacked-secret"));
}
