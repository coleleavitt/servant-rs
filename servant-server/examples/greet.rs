//! One typed API description, four interpretations.
//!
//! Run with: `cargo run -p servant-server --example greet`
//!
//! Defines a small API once and shows it drive: the server routing tree
//! (`layout`), generated documentation (`servant-docs`), a type-safe link
//! (`servant`'s `links`), and a typed client call served in-process.

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use serde::{Deserialize, Serialize};
use servant::hlist::{HNil, hlist1};
use servant::prelude::*;
use servant_client::{ClientError, ClientRequest, ClientResponse, RunClient, client};
use servant_docs::{HasDocs, markdown};
use servant_server::{RouterService, layout, serve};

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Greeting {
    message: String,
}

// "hello" :> Capture "name" String :> Get '[JSON] Greeting
//   :<|>
// "health" :> Get '[PlainText] String
macro_rules! greet_api {
    () => {
        alt(
            path(
                "hello",
                capture::<String, _>("name", get::<(Json,), Greeting>()),
            ),
            path("health", get::<(PlainText,), String>()),
        )
    };
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
        let http_req = b.body(Full::new(Bytes::new())).unwrap();
        let resp = self.0.handle(http_req).await;
        Ok(ClientResponse {
            status: resp.status(),
            headers: resp.headers().clone(),
            body: resp.into_body().collect().await.unwrap().to_bytes(),
        })
    }
}

#[tokio::main]
async fn main() {
    // 1. Server.
    let hello = |name: String| async move {
        Ok::<_, ServerError>(Greeting {
            message: format!("Hello, {name}!"),
        })
    };
    let health = || async { Ok::<_, ServerError>("ok".to_string()) };
    let router = serve(greet_api!(), (hello, health));
    let service = RouterService::new(router);

    println!("=== Router layout ===");
    // (Rebuild for layout since `serve` consumed the first router into a service.)
    let layout_router = serve(greet_api!(), (hello, health));
    print!("{}", layout(&layout_router));

    // 2. Docs.
    println!("\n=== Generated docs ===");
    print!("{}", markdown(&greet_api!().docs()));

    // 3. A type-safe link to the hello endpoint.
    let (hello_link, _) = servant::haslink::links(greet_api!());
    println!("\n=== Safe link ===");
    println!("{}", hello_link.link(hlist1("World".to_string())).to_uri());

    // 4. A typed client call, served in-process.
    let transport = InProcess(service);
    let (hello_client, health_client) = client(greet_api!());
    let greeting = hello_client
        .call(&transport, hlist1("Ada".to_string()))
        .await
        .unwrap();
    let health_resp = health_client.call(&transport, HNil).await.unwrap();
    println!("\n=== Typed client calls ===");
    println!("GET /hello/Ada  -> {greeting:?}");
    println!("GET /health     -> {health_resp:?}");
}
