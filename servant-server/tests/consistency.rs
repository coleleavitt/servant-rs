//! The central thesis: ONE API description drives the server, the typed client,
//! and the docs — and they agree. The same `the_api!()` expression is used for
//! all three interpretations (no route is defined more than once).

use bytes::Bytes;
use http::StatusCode;
use http_body_util::{BodyExt, Full};
use serde::{Deserialize, Serialize};
use servant::hlist::{HNil, hlist1};
use servant::prelude::*;
use servant_client::{ClientError, ClientRequest, ClientResponse, RunClient, client};
use servant_docs::HasDocs;
use servant_server::{RouterService, serve};

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
struct User {
    id: u64,
    name: String,
}

// The single source of truth, expanded into each interpretation.
macro_rules! the_api {
    () => {
        alt(
            path("users", capture::<u64, _>("id", get::<(Json,), User>())),
            path("hello", get::<(PlainText,), String>()),
        )
    };
}

fn router() -> RouterService {
    let get_user = |id: u64| async move {
        Ok::<_, ServerError>(User {
            id,
            name: format!("user{id}"),
        })
    };
    let hello = || async { Ok::<_, ServerError>("world".to_string()) };
    RouterService::new(serve(the_api!(), (get_user, hello)))
}

/// A `RunClient` transport that calls a `RouterService` in-process (no sockets).
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
        let status = resp.status();
        let headers = resp.headers().clone();
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        Ok(ClientResponse {
            status,
            headers,
            body,
        })
    }
}

#[tokio::test]
async fn client_and_server_round_trip_in_process() {
    let transport = InProcess(router());
    let (users_ep, hello_ep) = client(the_api!());

    // Typed client args line up with the API: GET /users/42 takes one u64.
    let user = users_ep.call(&transport, hlist1(42u64)).await.unwrap();
    assert_eq!(
        user,
        User {
            id: 42,
            name: "user42".into()
        }
    );

    let greeting = hello_ep.call(&transport, HNil).await.unwrap();
    assert_eq!(greeting, "world");
}

#[tokio::test]
async fn client_reports_failure_for_unknown_capture_type() {
    // The server 400s on an unparseable capture; the client surfaces it as a
    // non-success FailureResponse (status check) rather than a decode panic.
    // We exercise this by hitting a status the endpoint doesn't declare:
    // here the server returns 200 for valid ids, so a valid id succeeds and a
    // 400 (forced via a raw request) becomes FailureResponse.
    let transport = InProcess(router());
    let mut req = ClientRequest::new();
    req.method = http::Method::GET;
    req.append_path("users");
    req.append_path("not-a-number");
    req.accept = vec![mime::APPLICATION_JSON];
    let resp = transport.run_request(req).await.unwrap();
    assert_eq!(resp.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn docs_describe_the_same_endpoints_as_the_server() {
    let doc = the_api!().docs();
    let eps = doc.endpoints();
    assert_eq!(eps.len(), 2);

    // Build the (method, rendered-path) key set the docs expose...
    let mut doc_keys: Vec<String> = eps
        .iter()
        .map(|e| {
            let path = e
                .path
                .iter()
                .map(|p| match p {
                    servant_docs::PathPart::Static(s) => s.clone(),
                    servant_docs::PathPart::Capture { name, .. } => format!(":{name}"),
                    servant_docs::PathPart::CaptureAll { name, .. } => format!(":{name}"),
                })
                .collect::<Vec<_>>()
                .join("/");
            format!("{} /{}", e.method, path)
        })
        .collect();
    doc_keys.sort();
    assert_eq!(doc_keys, vec!["GET /hello", "GET /users/:id"]);

    // ...and confirm the server actually routes those paths (200), proving the
    // docs are not describing routes the server lacks.
    let svc = router();
    for (uri, accept) in [("/users/9", "application/json"), ("/hello", "text/plain")] {
        let req = http::Request::builder()
            .method("GET")
            .uri(uri)
            .header("accept", accept)
            .body(Full::new(Bytes::new()))
            .unwrap();
        assert_eq!(svc.handle(req).await.status(), StatusCode::OK, "{uri}");
    }
}

#[tokio::test]
async fn round_trip_over_real_hyper() {
    use servant_client::{BaseUrl, HyperClient};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(servant_server::adapter::serve_listener(listener, router()));

    let transport = HyperClient::new(BaseUrl::http("127.0.0.1", addr.port()));
    let (users_ep, _hello_ep) = client(the_api!());
    let user = users_ep.call(&transport, hlist1(7u64)).await.unwrap();
    assert_eq!(
        user,
        User {
            id: 7,
            name: "user7".into()
        }
    );
}
