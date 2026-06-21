use servant::api::{get, query_param, query_string};
use servant::content::Json;
use servant::query::Query;
use servant_client::{ClientRequest, HasClient};

#[test]
fn query_string_replaces_existing_query() {
    let api = query_param::<String, _>("existing", query_string(get::<(Json,), String>()));
    let mut req = ClientRequest::new();

    api.build_request(
        servant::hlist![
            Some("must-disappear".to_string()),
            Query::new(vec![
                ("name".to_string(), Some("bob".to_string())),
                ("flag".to_string(), None),
                ("empty".to_string(), Some(String::new())),
            ])
        ],
        &mut req,
    )
    .expect("client request builds");

    assert_eq!(req.target(), "/?name=bob&flag&empty=");
    assert!(!req.target().contains("existing=must-disappear"));
}
