use mime::Mime;

/// A media type marker. `media_types` may list several equivalent media types;
/// `media_type` is the canonical (first) one used to set `Content-Type`.
pub trait MediaType {
    /// The canonical media type.
    fn media_type() -> Mime;
    /// All media types this marker will match on (canonical first).
    fn media_types() -> Vec<Mime> {
        vec![Self::media_type()]
    }
}

/// `application/json`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Json;
/// `text/plain; charset=utf-8`.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlainText;
/// `application/x-www-form-urlencoded`.
#[derive(Debug, Clone, Copy, Default)]
pub struct FormUrlEncoded;
/// `application/octet-stream`.
#[derive(Debug, Clone, Copy, Default)]
pub struct OctetStream;
/// `text/event-stream` (Server-Sent Events).
#[derive(Debug, Clone, Copy, Default)]
pub struct EventStream;

impl MediaType for Json {
    fn media_type() -> Mime {
        mime::APPLICATION_JSON
    }
}
impl MediaType for PlainText {
    fn media_type() -> Mime {
        mime::TEXT_PLAIN_UTF_8
    }
}
impl MediaType for FormUrlEncoded {
    fn media_type() -> Mime {
        mime::APPLICATION_WWW_FORM_URLENCODED
    }
}
impl MediaType for OctetStream {
    fn media_type() -> Mime {
        mime::APPLICATION_OCTET_STREAM
    }
}
impl MediaType for EventStream {
    fn media_type() -> Mime {
        "text/event-stream".parse().expect("valid media type")
    }
}
