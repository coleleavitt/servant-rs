use serde::{Deserialize, Serialize};
use servant::prelude::*;
use servant_openapi::{OpenApiInfo, openapi_for};

#[derive(Serialize, Deserialize)]
struct Item {
    id: u64,
    name: String,
}

#[derive(Serialize, Deserialize)]
struct BookFilter {
    author: String,
    year: u16,
}

#[test]
fn deep_query_emits_deep_object_parameter() {
    let api = path(
        "books",
        deep_query::<BookFilter, _>("filter", get::<(Json,), Vec<Item>>()),
    );
    let doc = openapi_for(&api, OpenApiInfo::new("Books", "0.1.0"));
    let params = doc["paths"]["/books"]["get"]["parameters"]
        .as_array()
        .expect("parameters array");
    let filter = params
        .iter()
        .find(|param| param["name"] == "filter")
        .expect("filter parameter");

    assert_eq!(filter["in"], "query");
    assert_eq!(filter["required"], false);
    assert_eq!(filter["style"], "deepObject");
    assert_eq!(filter["explode"], true);
    assert_eq!(filter["schema"]["type"], "object");
    assert_eq!(filter["schema"]["title"], "BookFilter");
}
