use servant::api::{deep_query, get, path};
use servant::content::Json;
use servant::query::{DeepQueryEntry, DeepQueryParams, ToDeepQuery};
use servant_client::{ClientRequest, HasClient};

struct BookFilter {
    author: String,
    year: u16,
    tags: Vec<String>,
}

impl ToDeepQuery for BookFilter {
    fn to_deep_query(&self) -> DeepQueryParams {
        let mut entries = vec![
            DeepQueryEntry::with_value(["author"], self.author.clone()),
            DeepQueryEntry::with_value(["year"], self.year.to_string()),
        ];
        entries.extend(
            self.tags
                .iter()
                .map(|tag| DeepQueryEntry::with_value(["tag"], tag.clone())),
        );
        DeepQueryParams::new(entries)
    }
}

#[test]
fn deep_query_builds_nested_query_target() {
    let api = path(
        "books",
        deep_query::<BookFilter, _>("filter", get::<(Json,), String>()),
    );
    let mut req = ClientRequest::new();

    api.build_request(
        servant::hlist![BookFilter {
            author: "Frank Herbert".to_string(),
            year: 1965,
            tags: vec!["sci fi".to_string(), "classic".to_string()],
        }],
        &mut req,
    )
    .expect("client request builds");

    assert_eq!(
        req.target(),
        "/books?filter[author]=Frank%20Herbert&filter[year]=1965&filter[tag]=sci%20fi&filter[tag]=classic"
    );
}
