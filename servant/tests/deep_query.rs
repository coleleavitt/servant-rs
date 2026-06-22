use servant::query::{DeepQueryEntry, DeepQueryParams, parse_deep_query, render_deep_query_entry};

#[test]
fn deep_query_parser_preserves_duplicates_and_root_value() {
    let params = parse_deep_query(
        "filter",
        &[
            ("filter".to_string(), Some("all".to_string())),
            ("filter[user][name]".to_string(), Some("john".to_string())),
            ("filter[tag]".to_string(), Some("rust".to_string())),
            ("filter[tag]".to_string(), Some("haskell".to_string())),
            ("other[tag]".to_string(), Some("ignored".to_string())),
        ],
    )
    .expect("deep query parses");

    assert_eq!(params.entries().len(), 4);
    assert_eq!(params.entries()[0].path().segments(), &[] as &[String]);
    assert_eq!(params.first_value(&["user", "name"]), Some("john"));
    assert_eq!(params.values(&["tag"]), vec!["rust", "haskell"]);
}

#[test]
fn deep_query_renderer_preserves_bracket_syntax() {
    let entry = DeepQueryEntry::with_value(["author", "name"], "Frank Herbert");

    assert_eq!(
        render_deep_query_entry("filter", &entry),
        "filter[author][name]=Frank%20Herbert"
    );
}

#[test]
fn deep_query_parser_reports_malformed_brackets() {
    let err = parse_deep_query(
        "filter",
        &[("filter[author".to_string(), Some("secret".to_string()))],
    )
    .expect_err("malformed bracket syntax fails");

    assert_eq!(err.to_string(), "missing closing bracket in deep query key");
}

#[test]
fn deep_query_params_can_be_built_explicitly() {
    let params = DeepQueryParams::new(vec![DeepQueryEntry::flag(["present"])]);

    assert_eq!(params.entries()[0].value(), None);
}
