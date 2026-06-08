//! Response headers (`VerbWithHeaders` / `Headers<A>`): the server attaches
//! them and the typed client recovers both the body and the headers.

use bytes::Bytes;
use http::StatusCode;
use http_body_util::{BodyExt, Full};
use servant::hlist::HNil;
use servant::prelude::*;
use servant_client::{ClientError, ClientRequest, ClientResponse, RunClient, client};
use servant_server::{RouterService, serve};

macro_rules! the_api {
    () => {
        // GET /thing -> Headers { u32, X-Total-Count }
        path("thing", get_with_headers::<(Json,), u32>())
    };
}

fn router() -> RouterService {
    RouterService::new(serve(the_api!(), || async {
        Ok::<_, ServerError>(Headers::new(7u32).try_header("x-total-count", "42"))
    }))
}

struct InProcess(RouterService);
impl RunClient for InProcess {
    async fn run_request(&self, req: ClientRequest) -> Result<ClientResponse, ClientError> {
        let mut b = http::Request::builder()
            .method(req.method.clone())
            .uri(req.target());
        if let Some(a) = req.accept.first() {
            b = b.header(http::header::ACCEPT, a.as_ref());
        }
        let resp = self
            .0
            .handle(b.body(Full::new(Bytes::new())).unwrap())
            .await;
        Ok(ClientResponse {
            status: resp.status(),
            headers: resp.headers().clone(),
            body: resp.into_body().collect().await.unwrap().to_bytes(),
        })
    }
}

#[tokio::test]
async fn server_sets_response_header() {
    let svc = router();
    let req = http::Request::builder()
        .method("GET")
        .uri("/thing")
        .header("accept", "application/json")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let resp = svc.handle(req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("x-total-count")
            .unwrap()
            .to_str()
            .unwrap(),
        "42"
    );
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], b"7");
}

#[tokio::test]
async fn client_recovers_value_and_headers() {
    let t = InProcess(router());
    let endpoint = client(the_api!());
    let result = endpoint.call(&t, HNil).await.unwrap();
    assert_eq!(*result.value(), 7u32);
    let has_count = result
        .headers()
        .iter()
        .any(|(k, v)| k.as_str() == "x-total-count" && v == "42");
    assert!(has_count, "headers: {:?}", result.headers());
}
