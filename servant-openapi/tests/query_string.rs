use serde::{Deserialize, Serialize};
use servant::prelude::*;
use servant_openapi::{OpenApiInfo, openapi_for};

#[derive(Serialize, Deserialize)]
struct Item {
    id: u64,
    name: String,
}

#[test]
fn query_string_is_explicit_openapi_extension_not_fake_parameter() {
    let api = path(
        "search",
        query_string(query_param::<String, _>("fixed", get::<(Json,), Item>())),
    );
    let doc = openapi_for(&api, OpenApiInfo::new("Q", "0.1.0"));
    let op = &doc["paths"]["/search"]["get"];

    assert_eq!(op["x-servant-query-string"]["decodedOrderedPairs"], true);
    assert_eq!(op["x-servant-query-string"]["rawQuery"], "available");

    let params = op["parameters"].as_array().expect("parameters");
    assert_eq!(params.len(), 1, "QueryString must not fake a parameter");
    assert_eq!(params[0]["name"], "fixed");
}
