//! A small CRUD-style TODO service using an in-memory store.
//!
//! The example mirrors the shape of a realistic web app without adding a
//! database dependency: one API description drives routing, typed clients, safe
//! links, markdown docs, and OpenAPI output.
//!
//! Run with: `cargo run -p servant-server --example todos_crud`

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use serde::{Deserialize, Serialize};
use servant::hlist;
use servant::method::{Delete, Put};
use servant::prelude::*;
use servant_client::{ClientError, ClientRequest, ClientResponse, RunClient, client};
use servant_docs::{HasDocs, markdown};
use servant_openapi::{OpenApiInfo, openapi_for};
use servant_server::{RouterService, TestClient, layout, serve};

#[path = "todos_crud/store.rs"]
mod store;

use store::{create_todo, delete_todo, get_todo, list_todos, new_store, update_todo};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Todo {
    id: u64,
    title: String,
    completed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct NewTodo {
    title: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct UpdateTodo {
    title: Option<String>,
    completed: Option<bool>,
}

// GET    /todos       -> [Todo]
// POST   /todos       -> Todo, 201
// GET    /todos/{id}  -> Todo
// PUT    /todos/{id}  -> Todo
// DELETE /todos/{id}  -> 204 No Content
macro_rules! todo_api {
    () => {
        alt(
            summary("List todos", path("todos", get::<(Json,), Vec<Todo>>())),
            alt(
                summary(
                    "Create todo",
                    path(
                        "todos",
                        req_body::<(Json,), NewTodo, _>(verb::<Post, 201, (Json,), Todo>()),
                    ),
                ),
                alt(
                    summary(
                        "Fetch todo",
                        path("todos", capture::<u64, _>("id", get::<(Json,), Todo>())),
                    ),
                    alt(
                        summary(
                            "Update todo",
                            path(
                                "todos",
                                capture::<u64, _>(
                                    "id",
                                    req_body::<(Json,), UpdateTodo, _>(verb::<
                                        Put,
                                        200,
                                        (Json,),
                                        Todo,
                                    >(
                                    )),
                                ),
                            ),
                        ),
                        summary(
                            "Delete todo",
                            path("todos", capture::<u64, _>("id", no_content::<Delete>())),
                        ),
                    ),
                ),
            ),
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
    let store = new_store();
    let router = serve(
        todo_api!(),
        (
            list_todos(store.clone()),
            (
                create_todo(store.clone()),
                (
                    get_todo(store.clone()),
                    (update_todo(store.clone()), delete_todo(store.clone())),
                ),
            ),
        ),
    );
    let service = RouterService::new(router);

    println!("=== Router layout ===");
    print!(
        "{}",
        layout(&serve(
            todo_api!(),
            (
                list_todos(store.clone()),
                (
                    create_todo(store.clone()),
                    (
                        get_todo(store.clone()),
                        (update_todo(store.clone()), delete_todo(store.clone())),
                    ),
                ),
            ),
        ))
    );

    println!("\n=== Markdown docs ===");
    print!("{}", markdown(&todo_api!().docs()));

    println!("\n=== OpenAPI paths ===");
    let openapi = openapi_for(&todo_api!(), OpenApiInfo::new("TODO API", "1.0.0"));
    println!("{}", openapi["paths"]);

    println!("\n=== Safe link ===");
    let (_, (_, (todo_link, _))) = servant::haslink::links(todo_api!());
    println!("{}", todo_link.link(hlist![1u64]).to_uri());

    println!("\n=== Typed client round-trip ===");
    let transport = InProcess(service.clone());
    let (list_client, (create_client, (get_client, (update_client, delete_client)))) =
        client(todo_api!());
    let created = create_client
        .call(
            &transport,
            hlist![NewTodo {
                title: "ship servant-rs".to_string(),
            }],
        )
        .await
        .unwrap();
    let updated = update_client
        .call(
            &transport,
            hlist![
                created.id,
                UpdateTodo {
                    title: None,
                    completed: Some(true),
                }
            ],
        )
        .await
        .unwrap();
    let fetched = get_client
        .call(&transport, hlist![created.id])
        .await
        .unwrap();
    let listed = list_client.call(&transport, hlist![]).await.unwrap();
    delete_client
        .call(&transport, hlist![created.id])
        .await
        .unwrap();
    println!("created: {created:?}");
    println!("updated: {updated:?}");
    println!("fetched: {fetched:?}");
    println!("listed:  {listed:?}");

    println!("\n=== TestClient smoke request ===");
    let test_client = TestClient::from_service(service);
    let missing = test_client.get("/todos/999").await;
    println!("GET /todos/999 -> {} {}", missing.status(), missing.text());
}
