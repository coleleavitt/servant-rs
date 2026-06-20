use std::marker::PhantomData;

use http::{HeaderName, HeaderValue, StatusCode};

use crate::stream::SourceStream;

/// Extra response headers carried by a union arm (`WithStatusHeaders`).
pub type ArmHeaders = Vec<(HeaderName, HeaderValue)>;

/// Error returned when building response headers from strings.
#[derive(Debug, thiserror::Error)]
pub enum HeaderError {
    /// The header name is not a valid HTTP header name.
    #[error("invalid header name: {0}")]
    Name(#[from] http::header::InvalidHeaderName),
    /// The header value is not a valid HTTP header value.
    #[error("invalid header value: {0}")]
    Value(#[from] http::header::InvalidHeaderValue),
}

/// A response value tagged with the HTTP status it should carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithStatus<const S: u16, T>(pub T);

/// A response arm with status `S` and no response body. This mirrors the
/// body-less `ResponseType` variants available through Servant's `MultiVerb`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WithStatusNoBody<const S: u16>;

/// A response arm with status `S` rendered through exactly `CType`, independent
/// of the endpoint-level response content-type tuple. Include `CType` in the
/// endpoint's `UVerb` content-type list when you want normal `Accept` checking
/// to allow this arm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithFixedStatus<const S: u16, CType, T> {
    /// The response value.
    pub value: T,
    _marker: PhantomData<fn() -> CType>,
}

/// A streaming response arm with status `S`, item framing `Fr`, item content
/// type `CType`, and streamed item type `T`.
pub struct WithStreamingStatus<const S: u16, Fr, CType, T> {
    /// The streamed response value.
    pub stream: SourceStream<T>,
    _marker: PhantomData<fn() -> (Fr, CType)>,
}

/// A union arm that also carries response headers (the `WithHeaders` of
/// Servant's `MultiVerb`): status `S`, a body `T`, and extra headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithStatusHeaders<const S: u16, T> {
    /// The response value.
    pub value: T,
    /// Extra response headers.
    pub headers: ArmHeaders,
}

impl<const S: u16, T> WithStatusHeaders<S, T> {
    /// Wrap a value with status `S` and no extra headers.
    pub fn new(value: T) -> Self {
        WithStatusHeaders {
            value,
            headers: Vec::new(),
        }
    }

    /// Attach a header parsed from strings.
    ///
    /// # Errors
    ///
    /// Returns [`HeaderError`] when `name` or `value` is not a valid HTTP header.
    pub fn header(mut self, name: &str, value: &str) -> Result<Self, HeaderError> {
        let name = HeaderName::try_from(name)?;
        let value = HeaderValue::try_from(value)?;
        self.headers.push((name, value));
        Ok(self)
    }

    /// The status code `S`.
    pub fn status() -> StatusCode {
        StatusCode::from_u16(S).expect("WithStatusHeaders S must be a valid HTTP status code")
    }
}

impl<const S: u16, T> WithStatus<S, T> {
    /// Wrap a value with status `S`.
    pub fn new(value: T) -> Self {
        WithStatus(value)
    }

    /// The status code `S`.
    pub fn status() -> StatusCode {
        StatusCode::from_u16(S).expect("WithStatus S must be a valid HTTP status code")
    }

    /// The wrapped value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<const S: u16> WithStatusNoBody<S> {
    /// Build a body-less arm with status `S`.
    pub fn new() -> Self {
        WithStatusNoBody
    }

    /// The status code `S`.
    pub fn status() -> StatusCode {
        StatusCode::from_u16(S).expect("WithStatusNoBody S must be a valid HTTP status code")
    }
}

impl<const S: u16> Default for WithStatusNoBody<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const S: u16, CType, T> WithFixedStatus<S, CType, T> {
    /// Wrap a value with status `S` and fixed content type `CType`.
    pub fn new(value: T) -> Self {
        WithFixedStatus {
            value,
            _marker: PhantomData,
        }
    }

    /// The status code `S`.
    pub fn status() -> StatusCode {
        StatusCode::from_u16(S).expect("WithFixedStatus S must be a valid HTTP status code")
    }

    /// The wrapped value.
    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<const S: u16, Fr, CType, T> WithStreamingStatus<S, Fr, CType, T> {
    /// Wrap a stream with status `S`, framing `Fr`, and content type `CType`.
    pub fn new(stream: SourceStream<T>) -> Self {
        WithStreamingStatus {
            stream,
            _marker: PhantomData,
        }
    }

    /// The status code `S`.
    pub fn status() -> StatusCode {
        StatusCode::from_u16(S).expect("WithStreamingStatus S must be a valid HTTP status code")
    }

    /// The wrapped stream.
    pub fn into_inner(self) -> SourceStream<T> {
        self.stream
    }
}
