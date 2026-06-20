//! Ergonomic in-process test client for [`RouterService`].
//!
//! The client drives the same request buffering, routing, extraction, rendering,
//! and error handling as the tower/hyper adapter, but keeps tests in-process and
//! free of socket setup.

use bytes::Bytes;
use http::header::{ACCEPT, CONTENT_TYPE};
use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use http_body_util::{BodyExt, Full};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::adapter::RouterService;
use crate::response::ResponseBody;
use crate::router::Router;

/// In-process client for exercising a [`RouterService`] in integration tests.
#[derive(Clone)]
pub struct TestClient {
    service: RouterService,
    default_headers: HeaderMap,
}

impl TestClient {
    /// Build a test client from a router.
    pub fn new(router: Router) -> Self {
        Self::from_service(RouterService::new(router))
    }

    /// Build a test client from an already-configured service.
    pub fn from_service(service: RouterService) -> Self {
        TestClient {
            service,
            default_headers: HeaderMap::new(),
        }
    }

    /// Add a default header sent with every request.
    pub fn header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.default_headers.insert(name, value);
        self
    }

    /// Start building a request.
    pub fn request(&self, method: Method, uri: impl Into<String>) -> TestRequest<'_> {
        TestRequest {
            client: self,
            method,
            uri: uri.into(),
            headers: HeaderMap::new(),
            body: Bytes::new(),
        }
    }

    /// Send a `GET` request.
    pub async fn get(&self, uri: impl Into<String>) -> TestResponse {
        self.request(Method::GET, uri).send().await
    }

    /// Send a `POST` request with an empty body.
    pub async fn post(&self, uri: impl Into<String>) -> TestResponse {
        self.request(Method::POST, uri).send().await
    }
}

/// Builder for one in-process request.
pub struct TestRequest<'a> {
    client: &'a TestClient,
    method: Method,
    uri: String,
    headers: HeaderMap,
    body: Bytes,
}

impl TestRequest<'_> {
    /// Add or replace a request header.
    pub fn header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.insert(name, value);
        self
    }

    /// Set the `Accept` header.
    pub fn accept(self, value: &'static str) -> Self {
        self.header(ACCEPT, HeaderValue::from_static(value))
    }

    /// Set the `Content-Type` header.
    pub fn content_type(self, value: &'static str) -> Self {
        self.header(CONTENT_TYPE, HeaderValue::from_static(value))
    }

    /// Set a raw request body.
    pub fn body(mut self, body: impl Into<Bytes>) -> Self {
        self.body = body.into();
        self
    }

    /// Serialize a JSON request body and set `Content-Type: application/json`.
    pub fn json<T: Serialize>(self, value: &T) -> Self {
        self.content_type("application/json").body(Bytes::from(
            serde_json::to_vec(value).expect("test JSON body should serialize"),
        ))
    }

    /// Send the request through the wrapped [`RouterService`].
    pub async fn send(self) -> TestResponse {
        let mut builder = http::Request::builder()
            .method(self.method)
            .uri(self.uri.as_str());
        let headers = builder.headers_mut().expect("fresh request builder");
        headers.extend(self.client.default_headers.clone());
        headers.extend(self.headers);

        let response = self
            .client
            .service
            .handle(
                builder
                    .body(Full::new(self.body))
                    .expect("test request is valid"),
            )
            .await;
        TestResponse::from_response(response).await
    }
}

/// Buffered response returned by [`TestClient`].
pub struct TestResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
}

impl TestResponse {
    async fn from_response(response: http::Response<ResponseBody>) -> Self {
        let (parts, body) = response.into_parts();
        let body = body
            .collect()
            .await
            .expect("test response body should collect")
            .to_bytes();
        TestResponse {
            status: parts.status,
            headers: parts.headers,
            body,
        }
    }

    /// Response status.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Response headers.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Buffered response body.
    pub fn bytes(&self) -> &Bytes {
        &self.body
    }

    /// Interpret the response body as UTF-8 text.
    pub fn text(&self) -> &str {
        std::str::from_utf8(&self.body).expect("test response body should be UTF-8")
    }

    /// Deserialize the response body as JSON.
    pub fn json<T: DeserializeOwned>(&self) -> T {
        serde_json::from_slice(&self.body).expect("test response body should be JSON")
    }
}
