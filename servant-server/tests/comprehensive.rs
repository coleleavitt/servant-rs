//! A richer multi-combinator API exercised consistently across all four
//! interpretations (server, client, docs, links) — the Rust analogue of a
//! subset of Servant's `ComprehensiveAPI`. Combinators covered here:
//! `Path`, `Capture`, `CaptureAll`, `QueryParam` (optional), `QueryParams`,
//! `QueryFlag`, `Header` (optional), `ReqBody`, `Verb` (200/201),
//! `NoContentVerb` (204), `Description`/`Summary`, and nested `Alt`.

use bytes::Bytes;
use http::StatusCode;
use http_body_util::{BodyExt, Full};
use serde::{Deserialize, Serialize};
use servant::hlist;
use servant::hlist::hlist1;
use servant::prelude::*;
use servant_client::{ClientError, ClientRequest, ClientResponse, RunClient, client};
use servant_docs::HasDocs;
use servant_server::{RouterService, serve};

#[path = "support/comprehensive_parity.rs"]
mod comprehensive_parity;

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
struct Item {
    id: u64,
    label: String,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
struct NewItem {
    label: String,
}

// One description; expanded into server, client, docs, and links.
macro_rules! the_api {
    () => {
        alt(
            // GET /items?tag=&tag=&active&limit=  -> [String] (JSON)
            path(
                "items",
                query_params::<String, _>(
                    "tag",
                    query_flag(
                        "active",
                        query_param::<u32, _>("limit", get::<(Json,), Vec<String>>()),
                    ),
                ),
            ),
            alt(
                // GET /files/<rest...> -> String (text/plain)
                summary(
                    "Fetch a file by path",
                    path(
                        "files",
                        capture_all::<String, _>("rest", get::<(PlainText,), String>()),
                    ),
                ),
                alt(
                    // POST /items (JSON NewItem) -> Item (JSON), 201
                    path(
                        "items",
                        req_body::<(Json,), NewItem, _>(verb::<Post, 201, (Json,), Item>()),
                    ),
                    // DELETE /items/<id> -> 204
                    path("items", capture::<u64, _>("id", no_content::<Delete>())),
                ),
            ),
        )
    };
}

fn router() -> RouterService {
    let items_get = |tags: Vec<String>, active: bool, limit: Option<u32>| async move {
        let mut out = tags;
        if active {
            out.push("active".into());
        }
        if let Some(n) = limit {
            out.truncate(n as usize);
        }
        Ok::<_, ServerError>(out)
    };
    let files_get = |rest: Vec<String>| async move { Ok::<_, ServerError>(rest.join("/")) };
    let items_post = |body: NewItem| async move {
        Ok::<_, ServerError>(Item {
            id: 100,
            label: body.label,
        })
    };
    let items_delete = |_id: u64| async move { Ok::<_, ServerError>(NoContent) };

    RouterService::new(serve(
        the_api!(),
        (items_get, (files_get, (items_post, items_delete))),
    ))
}

struct InProcess(RouterService);

impl RunClient for InProcess {
    async fn run_request(&self, req: ClientRequest) -> Result<ClientResponse, ClientError> {
        let mut builder = http::Request::builder()
            .method(req.method.clone())
            .uri(req.target());
        if !req.accept.is_empty() {
            let accept = req
                .accept
                .iter()
                .map(|m| m.as_ref())
                .collect::<Vec<&str>>()
                .join(", ");
            builder = builder.header(http::header::ACCEPT, accept);
        }
        for (k, v) in req.headers.iter() {
            builder = builder.header(k, v);
        }
        let body = match &req.body {
            Some((bytes, mt)) => {
                builder = builder.header(http::header::CONTENT_TYPE, mt.as_ref());
                Full::new(bytes.clone())
            }
            None => Full::new(Bytes::new()),
        };
        let http_req = builder
            .body(body)
            .map_err(|e| ClientError::ConnectionError(Box::new(e)))?;
        let resp = self.0.handle(http_req).await;
        Ok(ClientResponse {
            status: resp.status(),
            headers: resp.headers().clone(),
            body: resp.into_body().collect().await.unwrap().to_bytes(),
        })
    }
}

#[tokio::test]
async fn all_combinators_round_trip() {
    let t = InProcess(router());
    let (items_get, (files_get, (items_post, items_delete))) = client(the_api!());

    // QueryParams + QueryFlag + optional QueryParam
    let got = items_get
        .call(
            &t,
            hlist![vec!["x".to_string(), "y".to_string()], true, Some(2u32)],
        )
        .await
        .unwrap();
    assert_eq!(got, vec!["x".to_string(), "y".to_string()]); // truncated to limit 2

    // QueryFlag off, no limit
    let got2 = items_get
        .call(&t, hlist![vec!["only".to_string()], false, None])
        .await
        .unwrap();
    assert_eq!(got2, vec!["only".to_string()]);

    // CaptureAll
    let file = files_get
        .call(
            &t,
            hlist1(vec!["a".to_string(), "b".to_string(), "c".to_string()]),
        )
        .await
        .unwrap();
    assert_eq!(file, "a/b/c");

    // ReqBody + 201
    let item = items_post
        .call(&t, hlist1(NewItem { label: "hi".into() }))
        .await
        .unwrap();
    assert_eq!(
        item,
        Item {
            id: 100,
            label: "hi".into()
        }
    );

    // NoContent / 204
    let nc = items_delete.call(&t, hlist1(9u64)).await.unwrap();
    assert_eq!(nc, NoContent);
}

#[tokio::test]
async fn docs_cover_all_endpoints() {
    let doc = the_api!().docs();
    assert_eq!(doc.endpoints().len(), 4);
    let md = servant_docs::markdown(&doc);
    assert!(md.contains("/items"));
    assert!(md.contains("/files/:rest"));
    assert!(md.contains("Status code 201"));
    assert!(md.contains("Status code 204"));
    assert!(md.contains("Fetch a file by path"));
}

#[tokio::test]
async fn links_route_to_real_endpoints() {
    use servant::haslink::links;
    let (items_link, (files_link, (_post_link, delete_link))) = links(the_api!());

    // items?tag=a&tag=b&active  (limit omitted)
    let l = items_link.link(hlist![
        vec!["a".to_string(), "b".to_string()],
        true,
        None::<u32>
    ]);
    assert_eq!(l.to_uri(), "/items?tag[]=a&tag[]=b&active");

    let lf = files_link.link(hlist1(vec!["docs".to_string(), "x.txt".to_string()]));
    assert_eq!(lf.to_uri(), "/files/docs/x.txt");

    // A safe link's path actually routes on the server (here: DELETE /items/5).
    let ld = delete_link.link(hlist1(5u64));
    assert_eq!(ld.to_uri(), "/items/5");
    let svc = router();
    let req = http::Request::builder()
        .method("DELETE")
        .uri(ld.to_uri())
        .body(Full::new(Bytes::new()))
        .unwrap();
    assert_eq!(svc.handle(req).await.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn comprehensive_parity_covers_new_combinators() {
    comprehensive_parity::covers_new_combinators().await;
}

#[tokio::test]
async fn comprehensive_parity_docs_links_client_and_openapi_agree() {
    comprehensive_parity::docs_links_client_and_openapi_agree().await;
}
