//! Turning a rendered value or a [`ServerError`] into an `http::Response`.
//!
//! The response body is a boxed [`http_body::Body`] so an endpoint can return
//! either a fully-buffered body ([`full_body`]) or a streaming one
//! ([`servant::stream`]).

use bytes::Bytes;
use http::header::CONTENT_TYPE;
use http::{HeaderValue, Response, StatusCode};
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, Full};
use mime::Mime;
use servant::error::ServerError;

/// Boxed error type for streaming response bodies.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// The server's uniform response body: a boxed [`http_body::Body`] of [`Bytes`].
///
/// `Unsync` (only `Send`, not `Sync`) so a streaming body backed by a
/// `Send`-only async stream can be returned; each connection is served on its
/// own task, so `Sync` is not required.
pub type ResponseBody = UnsyncBoxBody<Bytes, BoxError>;

/// Wrap fully-buffered bytes as a [`ResponseBody`].
pub fn full_body(bytes: Bytes) -> ResponseBody {
    Full::new(bytes)
        .map_err(|never: std::convert::Infallible| match never {})
        .boxed_unsync()
}

/// Build a success response with an optional `Content-Type` and extra headers.
pub fn build_response(
    status: StatusCode,
    content_type: Option<Mime>,
    body: ResponseBody,
    extra_headers: Vec<(http::HeaderName, HeaderValue)>,
) -> Response<ResponseBody> {
    let mut resp = Response::new(body);
    *resp.status_mut() = status;
    if let Some(m) = content_type {
        if let Ok(hv) = HeaderValue::from_str(m.as_ref()) {
            resp.headers_mut().insert(CONTENT_TYPE, hv);
        }
    }
    for (name, value) in extra_headers {
        resp.headers_mut().append(name, value);
    }
    resp
}

/// Build a response from a [`ServerError`] (status + headers + body).
pub fn error_response(err: &ServerError) -> Response<ResponseBody> {
    let mut resp = Response::new(full_body(err.body().clone()));
    *resp.status_mut() = err.status();
    *resp.headers_mut() = err.headers().clone();
    resp
}
