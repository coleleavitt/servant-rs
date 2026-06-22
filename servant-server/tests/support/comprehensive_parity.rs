#[macro_use]
#[path = "comprehensive_parity/api.rs"]
mod api;
#[path = "comprehensive_parity/fixture.rs"]
mod fixture;

use bytes::Bytes;
use http::StatusCode;
use http_body_util::Full;
use servant::prelude::*;
use servant_docs::HasDocs;
use servant_openapi::{OpenApiInfo, openapi_for};
use servant_server::{RouterService, serve, serve_with_context};

fn router() -> RouterService {
    RouterService::new(serve_with_context(
        full_api!(),
        full_handlers!(),
        fixture::context(),
    ))
}

pub async fn covers_new_combinators() {
    let transport = fixture::InProcess(router());
    let (create, (stream, (headers, (raw, rawm)))) = servant_client::client(client_api!());
    let created = create
        .call(&transport, create_args!())
        .await
        .expect("generated client create call succeeds");
    assert_eq!(created.id, 42);
    assert_eq!(created.label, "new");
    assert_eq!(
        created.observed,
        "alpha/beta|seed=yes&filter[author]=Ada%20Lovelace&filter[year]=1843&tag=rust&tag=servant&active&limit=2|Ada Lovelace|1843|rust,servant|true|noted",
    );

    let source = SourceStream::new(futures_util::stream::iter(vec![Ok(4u64), Ok(6u64)]));
    assert_eq!(
        stream
            .call(&transport, servant::hlist::hlist1(source))
            .await
            .expect("streaming request client succeeds"),
        "10",
    );
    let headers_resp = headers
        .call(&transport, servant::hlist::HNil)
        .await
        .expect("generated response-header client succeeds");
    assert_eq!(*headers_resp.value(), 7);
    assert_eq!(
        headers_resp
            .headers()
            .iter()
            .find(|(name, _)| name.as_str() == "x-total-count")
            .and_then(|(_, value)| value.to_str().ok()),
        Some("42"),
    );
    assert_eq!(
        raw.call_raw(&transport, http::Method::PATCH)
            .await
            .expect("raw client succeeds")
            .body,
        Bytes::from_static(b"raw:"),
    );
    assert_eq!(
        rawm.call_raw(&transport, http::Method::DELETE)
            .await
            .expect("rawm client succeeds")
            .body,
        Bytes::from_static(b"rawm-context:DELETE"),
    );
    assert_server_only_routes().await;
}

async fn assert_server_only_routes() {
    let svc = router();
    assert_eq!(
        fixture::text(&svc, "GET", "/resource", None).await,
        (StatusCode::OK, "77".into())
    );
    assert_eq!(
        fixture::text(&svc, "GET", "/auth", Some(("x-token", "ok")))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        svc.handle(
            http::Request::builder()
                .method("DELETE")
                .uri("/gone")
                .body(Full::new(Bytes::new()))
                .expect("request builds"),
        )
        .await
        .status(),
        StatusCode::NO_CONTENT,
    );
}

pub async fn docs_links_client_and_openapi_agree() {
    let layout = servant_server::layout(&serve(full_api!(), full_handlers!()));
    assert!(layout.contains("parity/"), "{layout}");
    assert!(layout.contains("<raw>"), "{layout}");

    let docs = full_api!().docs();
    assert_eq!(docs.endpoints().len(), 11);
    let markdown = servant_docs::markdown(&docs);
    for fragment in [
        "OperationId: createParity",
        "### Query String:",
        "### Deep Query Parameters:",
        "### Fragment:",
        "### Raw Endpoint:",
        "Decoded incrementally as a streaming request body.",
        "Status code 204",
    ] {
        assert!(
            markdown.contains(fragment),
            "missing `{fragment}` in\n{markdown}"
        );
    }

    let link = servant::haslink::links(create_api!()).link(link_args!());
    assert_eq!(
        link.to_uri(),
        "/parity/42/alpha/beta?seed=yes&filter[author]=Ada%20Lovelace&filter[year]=1843&tag[]=rust&tag[]=servant&active&limit=2#details",
    );
    let raw_link = servant::haslink::links(path("raw", raw())).link(servant::hlist::HNil);
    assert_eq!(raw_link.to_uri(), "/raw");
    let raw_m_link = servant::haslink::links(path("rawm", raw_m())).link(servant::hlist::HNil);
    assert_eq!(raw_m_link.to_uri(), "/rawm");

    let openapi = openapi_for(&full_api!(), OpenApiInfo::new("Parity", "1.0.0"));
    let op = &openapi["paths"]["/parity/{id}/{tail}"]["post"];
    assert_eq!(op["operationId"], "createParity");
    assert_eq!(op["x-servant-host"]["name"], "api.example.com");
    assert_eq!(
        op["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/NewParity"
    );
    assert_eq!(
        op["responses"]["201"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/ParityItem"
    );
    assert_eq!(
        openapi["paths"]["/stream-sum"]["post"]["requestBody"]["x-servant-streaming-request-body"],
        true
    );
    assert_eq!(
        openapi["paths"]["/headers"]["get"]["responses"]["200"]["content"]["application/json"]["schema"]
            ["type"],
        "integer"
    );
    assert!(openapi["components"]["schemas"]["NewParity"].is_object());
    assert!(openapi["paths"].get("/raw").is_none(), "{openapi:#}");
}
