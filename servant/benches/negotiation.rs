//! Microbenchmarks for the hot, allocation-sensitive core paths: content
//! negotiation and scalar URL parsing.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use servant::content::{Json, MediaType, OctetStream, PlainText, negotiate_media_index};
use servant::http_data::FromHttpApiData;

fn bench_negotiation(c: &mut Criterion) {
    let media = vec![
        Json::media_type(),
        PlainText::media_type(),
        OctetStream::media_type(),
    ];
    let mut group = c.benchmark_group("negotiate_media_index");
    for (label, accept) in [
        ("missing", None),
        ("exact", Some("application/json")),
        ("wildcard", Some("*/*")),
        (
            "q_values",
            Some("text/plain;q=0.3, application/json;q=0.9, */*;q=0.1"),
        ),
        ("unsupported", Some("application/xml")),
    ] {
        group.bench_with_input(BenchmarkId::from_parameter(label), &accept, |b, accept| {
            b.iter(|| negotiate_media_index(std::hint::black_box(*accept), &media));
        });
    }
    group.finish();
}

fn bench_scalar_parse(c: &mut Criterion) {
    c.bench_function("from_url_piece/u64", |b| {
        b.iter(|| u64::from_url_piece(std::hint::black_box("1234567890")));
    });
}

criterion_group!(benches, bench_negotiation, bench_scalar_parse);
criterion_main!(benches);
