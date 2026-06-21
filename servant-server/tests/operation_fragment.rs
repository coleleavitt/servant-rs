use http::StatusCode;
use servant::prelude::*;
use servant_server::{TestClient, serve};

#[tokio::test]
async fn fragment_combinator_has_no_server_routing_effect() {
    let api = path(
        "article",
        fragment::<String, _>("article section", get::<(PlainText,), String>()),
    );
    let client = TestClient::new(serve(api, article));

    let without_fragment = client
        .request(http::Method::GET, "/article")
        .accept("text/plain")
        .send()
        .await;
    assert_eq!(without_fragment.status(), StatusCode::OK);
    assert_eq!(without_fragment.text(), "article");

    let with_fragment = client
        .request(http::Method::GET, "/article#intro")
        .accept("text/plain")
        .send()
        .await;
    assert_eq!(with_fragment.status(), StatusCode::OK);
    assert_eq!(with_fragment.text(), "article");
}

async fn article() -> Result<String, ServerError> {
    Ok("article".to_string())
}
