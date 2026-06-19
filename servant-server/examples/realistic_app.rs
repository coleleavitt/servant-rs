//! A medium-sized API sketch that keeps the core servant-rs promise visible:
//! one typed description drives server routing, typed clients, safe links,
//! markdown docs, and OpenAPI output.
//!
//! Run with: `cargo run -p servant-server --example realistic_app`

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use serde::{Deserialize, Serialize};
use servant::hlist;
use servant::prelude::*;
use servant_client::{ClientError, ClientRequest, ClientResponse, RunClient, client};
use servant_docs::{HasDocs, markdown};
use servant_openapi::{OpenApiInfo, openapi_for};
use servant_server::{BasicAuthCheck, Context, RouterService, layout, serve, serve_with_context};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Account {
    id: u64,
    name: String,
    verbose: bool,
    trace_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct NewTicket {
    title: String,
    tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Ticket {
    id: u64,
    title: String,
    tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AdminUser {
    name: String,
}

// GET /accounts/<id>?verbose  with optional X-Trace-Id -> Account
//   :<|>
// GET /tickets?tag=...&tag=...&open                     -> [Ticket]
//   :<|>
// POST /tickets  (JSON body)                            -> Ticket, 201
macro_rules! public_api {
    () => {
        alt(
            summary(
                "Fetch one account",
                path(
                    "accounts",
                    capture::<u64, _>(
                        "id",
                        query_flag(
                            "verbose",
                            header::<String, _>("X-Trace-Id", get::<(Json,), Account>()),
                        ),
                    ),
                ),
            ),
            alt(
                summary(
                    "Search tickets",
                    path(
                        "tickets",
                        query_params::<String, _>(
                            "tag",
                            query_flag("open", get::<(Json,), Vec<Ticket>>()),
                        ),
                    ),
                ),
                description(
                    "Create a ticket from a JSON request body.",
                    path(
                        "tickets",
                        req_body::<(Json,), NewTicket, _>(verb::<Post, 201, (Json,), Ticket>()),
                    ),
                ),
            ),
        )
    };
}

// A server/docs-only slice that demonstrates auth/context without pretending the
// typed client supports auth combinators yet.
macro_rules! admin_api {
    () => {
        path(
            "admin",
            basic_auth::<AdminUser, _>("admin area", get::<(PlainText,), String>()),
        )
    };
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

#[tokio::main]
async fn main() {
    let get_account = |id: u64, verbose: bool, trace_id: Option<String>| async move {
        Ok::<_, ServerError>(Account {
            id,
            name: format!("account-{id}"),
            verbose,
            trace_id,
        })
    };
    let search_tickets = |tags: Vec<String>, open: bool| async move {
        Ok::<_, ServerError>(vec![Ticket {
            id: if open { 1 } else { 2 },
            title: "triage ergonomics".to_string(),
            tags,
        }])
    };
    let create_ticket = |new: NewTicket| async move {
        Ok::<_, ServerError>(Ticket {
            id: 42,
            title: new.title,
            tags: new.tags,
        })
    };

    let public_router = serve(
        public_api!(),
        (get_account, (search_tickets, create_ticket)),
    );
    let service = RouterService::new(public_router);

    let auth = BasicAuthCheck::new(|data: &servant::auth::BasicAuthData| {
        if data.username == "root" && data.password == "s3cr3t" {
            servant::auth::BasicAuthResult::Authorized(AdminUser {
                name: data.username.clone(),
            })
        } else {
            servant::auth::BasicAuthResult::BadPassword
        }
    });
    let admin_router = serve_with_context(
        admin_api!(),
        |user: AdminUser| async move { Ok::<_, ServerError>(format!("hello {}", user.name)) },
        Context::new().with(auth),
    );

    println!("=== Public router layout ===");
    print!(
        "{}",
        layout(&serve(
            public_api!(),
            (get_account, (search_tickets, create_ticket))
        ))
    );
    println!("\n=== Admin router layout ===");
    print!("{}", layout(&admin_router));

    println!("\n=== Generated markdown ===");
    print!("{}", markdown(&public_api!().docs()));

    println!("\n=== Generated OpenAPI paths ===");
    let openapi = openapi_for(&public_api!(), OpenApiInfo::new("Support API", "1.0.0"));
    println!("{}", openapi["paths"].as_object().unwrap().len());

    println!("\n=== Safe link ===");
    let (account_link, _) = servant::haslink::links(public_api!());
    println!("{}", account_link.link(hlist![7u64, true]).to_uri());

    println!("\n=== Typed client calls ===");
    let transport = InProcess(service);
    let (account_client, (tickets_client, create_client)) = client(public_api!());
    let account = account_client
        .call(
            &transport,
            hlist![7u64, true, Some("demo-trace".to_string())],
        )
        .await
        .unwrap();
    let tickets = tickets_client
        .call(&transport, hlist![vec!["bug".to_string()], true])
        .await
        .unwrap();
    let created = create_client
        .call(
            &transport,
            hlist![NewTicket {
                title: "write compile-fail tests".to_string(),
                tags: vec!["quality".to_string()],
            }],
        )
        .await
        .unwrap();
    println!("GET /accounts/7 -> {account:?}");
    println!("GET /tickets     -> {tickets:?}");
    println!("POST /tickets    -> {created:?}");
}
