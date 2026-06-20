use http::StatusCode;
use serde::{Deserialize, Serialize};
use servant::prelude::*;
use servant_server::{TestClient, serve};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Greeting {
    message: String,
}

#[tokio::test]
async fn test_client_exercises_router_without_manual_request_building() {
    let api = alt(
        path(
            "hello",
            capture::<String, _>("name", get::<(Json,), Greeting>()),
        ),
        path(
            "echo",
            req_body::<(Json,), Greeting, _>(post::<(Json,), Greeting>()),
        ),
    );
    let client = TestClient::new(serve(api, (hello, echo)));

    let hello = client
        .request(http::Method::GET, "/hello/alice")
        .accept("application/json")
        .send()
        .await;
    assert_eq!(hello.status(), StatusCode::OK);
    assert_eq!(
        hello.json::<Greeting>(),
        Greeting {
            message: "hello alice".to_string()
        }
    );

    let echo_body = Greeting {
        message: "round trip".to_string(),
    };
    let echo = client
        .request(http::Method::POST, "/echo")
        .accept("application/json")
        .json(&echo_body)
        .send()
        .await;
    assert_eq!(echo.status(), StatusCode::OK);
    assert_eq!(echo.json::<Greeting>(), echo_body);
}

async fn hello(name: String) -> Result<Greeting, ServerError> {
    Ok(Greeting {
        message: format!("hello {name}"),
    })
}

async fn echo(body: Greeting) -> Result<Greeting, ServerError> {
    Ok(body)
}
