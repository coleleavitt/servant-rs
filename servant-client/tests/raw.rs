use bytes::Bytes;
use servant::prelude::*;
use servant_client::{
    ClientError,
    ClientRequest,
    ClientResponse,
    RawClientEndpoint,
    RunClient,
    client,
};

#[derive(Clone)]
struct EchoTransport;

impl RunClient for EchoTransport {
    async fn run_request(
        &self,
        req: ClientRequest,
    ) -> Result<ClientResponse, servant_client::ClientError> {
        assert_eq!(req.method, http::Method::POST);
        assert_eq!(req.target(), "/files");
        let mut headers = http::HeaderMap::new();
        headers.insert("x-client-raw", http::HeaderValue::from_static("kept"));
        Ok(ClientResponse {
            status: http::StatusCode::ACCEPTED,
            headers,
            body: Bytes::from_static(b"raw response"),
        })
    }
}

#[derive(Clone)]
struct MethodTargetTransport {
    expected_method: http::Method,
}

impl RunClient for MethodTargetTransport {
    async fn run_request(&self, req: ClientRequest) -> Result<ClientResponse, ClientError> {
        assert_eq!(req.method, self.expected_method);
        assert_eq!(req.target(), "/api/files");
        Ok(ClientResponse {
            status: http::StatusCode::CREATED,
            headers: http::HeaderMap::new(),
            body: Bytes::from(format!("{} {}", req.method, req.target())),
        })
    }
}

#[tokio::test]
async fn raw_client_returns_raw_response_for_selected_method() {
    // Given: a generated client for a Raw endpoint.
    let endpoint: RawClientEndpoint<_> = client(path("files", raw()));

    // When: the caller selects POST at call time.
    let response = endpoint
        .call_raw(&EchoTransport, http::Method::POST)
        .await
        .expect("raw client call succeeds");

    // Then: the raw response is returned without status/content decoding.
    assert_eq!(response.status, http::StatusCode::ACCEPTED);
    assert_eq!(
        response
            .headers
            .get("x-client-raw")
            .and_then(|value| value.to_str().ok()),
        Some("kept")
    );
    assert_eq!(response.body, Bytes::from_static(b"raw response"));
}

#[tokio::test]
async fn nested_static_raw_client_returns_raw_response_for_selected_method() {
    // Given: a generated Raw client below two static path prefixes.
    let endpoint: RawClientEndpoint<_> = client(path("api", path("files", raw())));

    // When: the caller selects GET at call time.
    let get_response = endpoint
        .call_raw(
            &MethodTargetTransport {
                expected_method: http::Method::GET,
            },
            http::Method::GET,
        )
        .await
        .expect("nested raw GET client call succeeds");

    // Then: the raw response is returned without typed decoding.
    assert_eq!(get_response.status, http::StatusCode::CREATED);
    assert_eq!(get_response.body, Bytes::from_static(b"GET /api/files"));

    // When: the caller selects POST at call time.
    let post_response = endpoint
        .call_raw(
            &MethodTargetTransport {
                expected_method: http::Method::POST,
            },
            http::Method::POST,
        )
        .await
        .expect("nested raw POST client call succeeds");

    // Then: the same generated endpoint targets the same prefix with a new method.
    assert_eq!(post_response.status, http::StatusCode::CREATED);
    assert_eq!(post_response.body, Bytes::from_static(b"POST /api/files"));
}
