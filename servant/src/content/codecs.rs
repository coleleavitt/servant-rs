use bytes::Bytes;
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::media::{FormUrlEncoded, Json, MediaType, OctetStream, PlainText};

/// Serialize a value into the wire bytes of content type `C`.
///
/// Fallible because, unlike Haskell's total `aeson` encoder, `serde_json` /
/// `serde_urlencoded` can fail at runtime (e.g. a map with non-string keys, or
/// a non-flat struct for forms). A failure becomes a `500` on the server and a
/// client encode error, never a panic.
pub trait MimeRender<C: MediaType> {
    /// Render `self` as `C`, or return a human-readable error message.
    fn mime_render(&self) -> Result<Bytes, String>;
}

/// Parse a value from the wire bytes of content type `C`.
pub trait MimeUnrender<C: MediaType>: Sized {
    /// Parse bytes interpreted as `C`. The `Err` is a human-readable message.
    fn mime_unrender(bytes: &[u8]) -> Result<Self, String>;
}

// --- JSON (blanket over serde) ---

impl<A: Serialize> MimeRender<Json> for A {
    fn mime_render(&self) -> Result<Bytes, String> {
        serde_json::to_vec(self)
            .map(Bytes::from)
            .map_err(|e| e.to_string())
    }
}

impl<A: DeserializeOwned> MimeUnrender<Json> for A {
    fn mime_unrender(bytes: &[u8]) -> Result<Self, String> {
        serde_json::from_slice(bytes).map_err(|e| e.to_string())
    }
}

// --- FormUrlEncoded (blanket over serde) ---

impl<A: Serialize> MimeRender<FormUrlEncoded> for A {
    fn mime_render(&self) -> Result<Bytes, String> {
        serde_urlencoded::to_string(self)
            .map(|s| Bytes::from(s.into_bytes()))
            .map_err(|e| e.to_string())
    }
}

impl<A: DeserializeOwned> MimeUnrender<FormUrlEncoded> for A {
    fn mime_unrender(bytes: &[u8]) -> Result<Self, String> {
        serde_urlencoded::from_bytes(bytes).map_err(|e| e.to_string())
    }
}

// --- PlainText ---

impl MimeRender<PlainText> for String {
    fn mime_render(&self) -> Result<Bytes, String> {
        Ok(Bytes::from(self.clone().into_bytes()))
    }
}
impl MimeRender<PlainText> for str {
    fn mime_render(&self) -> Result<Bytes, String> {
        Ok(Bytes::from(self.to_owned().into_bytes()))
    }
}
impl MimeUnrender<PlainText> for String {
    fn mime_unrender(bytes: &[u8]) -> Result<Self, String> {
        String::from_utf8(bytes.to_vec()).map_err(|e| e.to_string())
    }
}

// --- OctetStream ---

impl MimeRender<OctetStream> for Bytes {
    fn mime_render(&self) -> Result<Bytes, String> {
        Ok(self.clone())
    }
}
impl MimeRender<OctetStream> for Vec<u8> {
    fn mime_render(&self) -> Result<Bytes, String> {
        Ok(Bytes::from(self.clone()))
    }
}
impl MimeUnrender<OctetStream> for Bytes {
    fn mime_unrender(bytes: &[u8]) -> Result<Self, String> {
        Ok(Bytes::copy_from_slice(bytes))
    }
}
impl MimeUnrender<OctetStream> for Vec<u8> {
    fn mime_unrender(bytes: &[u8]) -> Result<Self, String> {
        Ok(bytes.to_vec())
    }
}

/// The body-less response value. Renders to empty bytes for every content type
/// (so `Get '[JSON] NoContent` still negotiates `Accept`) and decodes from any
/// content type for clients.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoContent;

impl<C: MediaType> MimeRender<C> for NoContent {
    fn mime_render(&self) -> Result<Bytes, String> {
        Ok(Bytes::new())
    }
}
impl<C: MediaType> MimeUnrender<C> for NoContent {
    fn mime_unrender(_bytes: &[u8]) -> Result<Self, String> {
        Ok(NoContent)
    }
}
