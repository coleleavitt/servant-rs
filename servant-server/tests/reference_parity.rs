//! Targeted parity checks for behavior inherited from Haskell Servant's router,
//! delayed extraction pipeline, content negotiation, and shared API
//! interpretations.

use bytes::Bytes;
use http::StatusCode;
use http_body_util::{BodyExt, Full};
use serde::{Deserialize, Serialize};
use servant::hlist;
use servant::prelude::*;
use servant_client::{ClientError, ClientRequest, ClientResponse, RunClient, client};
use servant_docs::HasDocs;
use servant_openapi::{OpenApiInfo, openapi_for};
use servant_server::{RouterService, serve};

async fn call(svc: &RouterService, req: http::Request<Full<Bytes>>) -> (StatusCode, String) {
    let resp = svc.handle(req).await;
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

fn get_req(uri: &str) -> http::Request<Full<Bytes>> {
    http::Request::builder()
        .method("GET")
        .uri(uri)
        .body(Full::new(Bytes::new()))
        .unwrap()
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
            Some((bytes, content_type)) => {
                builder = builder.header(http::header::CONTENT_TYPE, content_type.as_ref());
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
async fn mixed_capture_static_choice_is_left_biased_under_shared_prefix() {
    // Matches `Servant.Server.Internal.Router.choice`: mixed capture/static
    // routers under an already-matched prefix remain a left-biased `Choice`.
    let api = alt(
        path(
            "users",
            capture::<String, _>("name", get::<(PlainText,), String>()),
        ),
        path("users", path("me", get::<(PlainText,), String>())),
    );
    let svc = RouterService::new(serve(
        api,
        (
            |name: String| async move { Ok::<_, ServerError>(format!("capture:{name}")) },
            || async { Ok::<_, ServerError>("static".to_string()) },
        ),
    ));

    assert_eq!(
        call(&svc, get_req("/users/me")).await,
        (StatusCode::OK, "capture:me".into())
    );
    assert_eq!(
        call(&svc, get_req("/users/alice")).await,
        (StatusCode::OK, "capture:alice".into())
    );
}

#[tokio::test]
async fn recoverable_route_failure_falls_through_to_later_choice() {
    // A capture parse failure is recoverable, so the later static route can
    // still handle the request, matching Servant's route-choice semantics.
    let api = alt(
        path(
            "orders",
            capture::<u64, _>("id", get::<(PlainText,), String>()),
        ),
        path("orders", path("latest", get::<(PlainText,), String>())),
    );
    let svc = RouterService::new(serve(
        api,
        (
            |id: u64| async move { Ok::<_, ServerError>(format!("order:{id}")) },
            || async { Ok::<_, ServerError>("latest".to_string()) },
        ),
    ));

    assert_eq!(
        call(&svc, get_req("/orders/latest")).await,
        (StatusCode::OK, "latest".into())
    );
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Payload {
    value: String,
}

#[tokio::test]
async fn extraction_checks_path_before_request_body() {
    // Servant's delayed pipeline validates route/capture failures before body
    // decoding. A bad capture should not be masked by malformed JSON.
    let api = path(
        "decode",
        capture::<u64, _>(
            "id",
            req_body::<(Json,), Payload, _>(post::<(Json,), Payload>()),
        ),
    );
    let svc = RouterService::new(serve(api, |_id: u64, body: Payload| async move {
        Ok::<_, ServerError>(body)
    }));
    let req = http::Request::builder()
        .method("POST")
        .uri("/decode/not-a-number")
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from("{ not json")))
        .unwrap();

    let (status, body) = call(&svc, req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("could not parse capture"));
}

#[tokio::test]
async fn content_negotiation_respects_accept_before_client_decoding() {
    let api = path("message", get::<(Json, PlainText), String>());
    let svc = RouterService::new(serve(api, || async {
        Ok::<_, ServerError>("hello".to_string())
    }));

    let unsupported_accept = http::Request::builder()
        .method("GET")
        .uri("/message")
        .header(http::header::ACCEPT, "application/xml")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let (status, _) = call(&svc, unsupported_accept).await;
    assert_eq!(status, StatusCode::NOT_ACCEPTABLE);

    let transport = InProcess(svc);
    let endpoint = client(path("message", get::<(Json, PlainText), String>()));
    let body = endpoint
        .call(&transport, servant::hlist::HNil)
        .await
        .unwrap();
    assert_eq!(body, "hello");
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct NewWidget {
    name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Widget {
    id: u64,
    name: String,
}

macro_rules! widget_api {
    () => {
        alt(
            path(
                "widgets",
                capture::<u64, _>("id", query_flag("verbose", get::<(Json,), Widget>())),
            ),
            path(
                "widgets",
                req_body::<(Json,), NewWidget, _>(verb::<Post, 201, (Json,), Widget>()),
            ),
        )
    };
}

fn widget_service() -> RouterService {
    RouterService::new(serve(
        widget_api!(),
        (
            |id: u64, verbose: bool| async move {
                Ok::<_, ServerError>(Widget {
                    id,
                    name: if verbose { "verbose-widget" } else { "widget" }.into(),
                })
            },
            |body: NewWidget| async move {
                Ok::<_, ServerError>(Widget {
                    id: 10,
                    name: body.name,
                })
            },
        ),
    ))
}

#[tokio::test]
async fn one_api_description_stays_consistent_across_interpretations() {
    let transport = InProcess(widget_service());
    let (get_widget, create_widget) = client(widget_api!());

    let fetched = get_widget
        .call(&transport, hlist![5u64, true])
        .await
        .unwrap();
    assert_eq!(
        fetched,
        Widget {
            id: 5,
            name: "verbose-widget".into()
        }
    );

    let created = create_widget
        .call(&transport, hlist![NewWidget { name: "new".into() }])
        .await
        .unwrap();
    assert_eq!(
        created,
        Widget {
            id: 10,
            name: "new".into()
        }
    );

    let (get_link, _create_link) = servant::haslink::links(widget_api!());
    assert_eq!(
        get_link.link(hlist![5u64, true]).to_uri(),
        "/widgets/5?verbose"
    );

    let docs = widget_api!().docs();
    assert_eq!(docs.endpoints().len(), 2);
    let markdown = servant_docs::markdown(&docs);
    assert!(markdown.contains("/widgets/:id"));
    assert!(markdown.contains("Status code 201"));

    let openapi = openapi_for(&widget_api!(), OpenApiInfo::new("Widgets", "1.0.0"));
    assert!(openapi["paths"]["/widgets/{id}"]["get"].is_object());
    assert!(openapi["paths"]["/widgets"]["post"].is_object());
}
