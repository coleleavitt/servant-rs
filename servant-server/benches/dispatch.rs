//! End-to-end request-dispatch microbenchmarks: build the router once, then
//! measure routing + extraction + handler + render for a few request shapes.

use bytes::Bytes;
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use http_body_util::Full;
use serde::{Deserialize, Serialize};
use servant::prelude::*;
use servant_server::{RouterService, serve};

#[derive(Serialize, Deserialize)]
struct User {
    id: u64,
    name: String,
}

fn router() -> RouterService {
    let api = alt(
        path("users", capture::<u64, _>("id", get::<(Json,), User>())),
        alt(
            path("hello", get::<(PlainText,), String>()),
            path(
                "search",
                query_param::<String, _>("q", get::<(Json,), String>()),
            ),
        ),
    );
    RouterService::new(serve(
        api,
        (
            |id: u64| async move {
                Ok::<_, ServerError>(User {
                    id,
                    name: "u".into(),
                })
            },
            (
                || async { Ok::<_, ServerError>("hello".to_string()) },
                |q: Option<String>| async move { Ok::<_, ServerError>(q.unwrap_or_default()) },
            ),
        ),
    ))
}

fn req(uri: &str, accept: &str) -> http::Request<Full<Bytes>> {
    http::Request::builder()
        .method("GET")
        .uri(uri)
        .header("accept", accept)
        .body(Full::new(Bytes::new()))
        .unwrap()
}

fn bench_dispatch(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let svc = router();

    let mut group = c.benchmark_group("dispatch");
    for (label, uri, accept) in [
        ("capture_json", "/users/42", "application/json"),
        ("static_plain", "/hello", "text/plain"),
        ("query_param", "/search?q=rust", "application/json"),
        ("not_found", "/nope", "application/json"),
    ] {
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &(uri, accept),
            |b, (uri, accept)| {
                b.iter(|| rt.block_on(async { svc.handle(req(uri, accept)).await }));
            },
        );
    }
    group.finish();
}

fn bench_build(c: &mut Criterion) {
    c.bench_function("router_build", |b| b.iter(router));
}

criterion_group!(benches, bench_dispatch, bench_build);
criterion_main!(benches);
