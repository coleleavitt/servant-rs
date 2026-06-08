//! UVerb union responses: the server sends the active arm's status + body, and
//! the typed client decodes the union by matching the response status.

use bytes::Bytes;
use http::StatusCode;
use http_body_util::{BodyExt, Full};
use serde::{Deserialize, Serialize};
use servant::prelude::*;
use servant_client::{ClientError, ClientRequest, ClientResponse, RunClient, client};
use servant_server::{RouterService, serve};

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
struct User {
    id: u64,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
struct NotFound {
    message: String,
}

// GET /users/<id> -> 200 User | 404 NotFound  (JSON union)
type Resp = Union2<WithStatus<200, User>, WithStatus<404, NotFound>>;

macro_rules! the_api {
    () => {
        path(
            "users",
            capture::<u64, _>("id", uverb::<Get, (Json,), Resp>()),
        )
    };
}

fn router() -> RouterService {
    RouterService::new(serve(the_api!(), |id: u64| async move {
        if id == 1 {
            Ok::<_, ServerError>(Union2::V0(WithStatus::new(User { id })))
        } else {
            Ok(Union2::V1(WithStatus::new(NotFound {
                message: "no such user".into(),
            })))
        }
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
async fn server_sends_active_arm_status() {
    let svc = router();
    // id=1 -> 200 User
    let ok = svc
        .handle(
            http::Request::builder()
                .method("GET")
                .uri("/users/1")
                .header("accept", "application/json")
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await;
    assert_eq!(ok.status(), StatusCode::OK);
    let body = ok.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], br#"{"id":1}"#);

    // id=2 -> 404 NotFound
    let nf = svc
        .handle(
            http::Request::builder()
                .method("GET")
                .uri("/users/2")
                .header("accept", "application/json")
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await;
    assert_eq!(nf.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn client_decodes_union_by_status() {
    let t = InProcess(router());
    let endpoint = client(the_api!());

    let found = endpoint.call(&t, servant::hlist![1u64]).await.unwrap();
    assert_eq!(found, Union2::V0(WithStatus::new(User { id: 1 })));

    let missing = endpoint.call(&t, servant::hlist![2u64]).await.unwrap();
    assert_eq!(
        missing,
        Union2::V1(WithStatus::new(NotFound {
            message: "no such user".into()
        }))
    );
}
