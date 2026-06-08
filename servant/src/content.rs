//! Content types and content negotiation, mirroring
//! `Servant.API.ContentTypes`.
//!
//! A content type is a zero-sized **marker** (`Json`, `PlainText`,
//! `FormUrlEncoded`, `OctetStream`) implementing [`MediaType`]. Serializing a
//! value into a marker's wire form is [`MimeRender`]; parsing back is
//! [`MimeUnrender`]. These are *open* traits — users add markers and impls.
//!
//! A content-type *list* (a tuple like `(Json,)` or `(Json, PlainText)`) drives
//! negotiation through [`AllMime`], [`AllMimeRender`], and [`AllMimeUnrender`].
//! The negotiation algorithms mirror http-media's `mapAcceptMedia` (response,
//! `Accept`) and `mapContentMedia` (request, `Content-Type`).

use bytes::Bytes;
use mime::Mime;
use serde::Serialize;
use serde::de::DeserializeOwned;

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

// ---------------------------------------------------------------------------
// Content-type lists (tuples)
// ---------------------------------------------------------------------------

/// All media types a content-type list matches on, canonical (primary) first.
pub trait AllMime {
    /// Every media type, in list order (each marker's `media_types`, flattened).
    fn all_media_types() -> Vec<Mime>;
    /// The primary media type (used as the request `Content-Type` for clients
    /// and as a fallback response type).
    fn primary() -> Mime {
        Self::all_media_types()
            .into_iter()
            .next()
            .expect("content-type list is non-empty")
    }
}

/// Render a value `A` in the content types of the list, for `Accept`
/// negotiation.
pub trait AllMimeRender<A>: AllMime {
    /// Render `value` using the codec that owns the `idx`-th media type (the
    /// index returned by [`negotiate_media_index`] against [`AllMime::all_media_types`]).
    fn render_index(value: &A, idx: usize) -> Result<Bytes, String>;

    /// Render `value` in the primary (first) content type, returning its media
    /// type and bytes. Used by clients to encode a request body.
    fn render_primary(value: &A) -> Result<(Mime, Bytes), String> {
        let mime = Self::primary();
        Self::render_index(value, 0).map(|b| (mime, b))
    }

    /// Render `value` once per media type, in list order (used in tests).
    fn render_all(value: &A) -> Result<Vec<(Mime, Bytes)>, String> {
        let media = Self::all_media_types();
        let mut out = Vec::with_capacity(media.len());
        for (i, mt) in media.into_iter().enumerate() {
            out.push((mt, Self::render_index(value, i)?));
        }
        Ok(out)
    }
}

/// Look up the decoder for a request `Content-Type` against the list.
pub trait AllMimeUnrender<A>: AllMime {
    /// If `ct` matches a content type in the list, decode `body` with it.
    fn unrender(ct: &Mime, body: &[u8]) -> Option<Result<A, String>>;
}

macro_rules! impl_content_list {
    ( $( $name:ident ),+ ) => {
        impl<$( $name: MediaType ),+> AllMime for ($( $name, )+) {
            fn all_media_types() -> Vec<Mime> {
                let mut v = Vec::new();
                $( v.extend($name::media_types()); )+
                v
            }
        }

        impl<A, $( $name ),+> AllMimeRender<A> for ($( $name, )+)
        where
            $( $name: MediaType, A: MimeRender<$name> ),+
        {
            fn render_index(value: &A, idx: usize) -> Result<Bytes, String> {
                // Walk markers in order; the marker owning the flattened `idx`
                // does the rendering (so a marker with several media types
                // shares one codec).
                let mut i = idx;
                $(
                    let n = $name::media_types().len();
                    if i < n {
                        return <A as MimeRender<$name>>::mime_render(value);
                    }
                    i -= n;
                )+
                let _ = i;
                Err("content-type index out of range".to_string())
            }
        }

        impl<A, $( $name ),+> AllMimeUnrender<A> for ($( $name, )+)
        where
            $( $name: MediaType, A: MimeUnrender<$name> ),+
        {
            fn unrender(ct: &Mime, body: &[u8]) -> Option<Result<A, String>> {
                $(
                    for mt in $name::media_types() {
                        if media_matches_content(&mt, ct) {
                            return Some(<A as MimeUnrender<$name>>::mime_unrender(body));
                        }
                    }
                )+
                None
            }
        }
    };
}

impl_content_list!(C0);
impl_content_list!(C0, C1);
impl_content_list!(C0, C1, C2);
impl_content_list!(C0, C1, C2, C3);
impl_content_list!(C0, C1, C2, C3, C4);
impl_content_list!(C0, C1, C2, C3, C4, C5);

// ---------------------------------------------------------------------------
// Negotiation
// ---------------------------------------------------------------------------

/// One entry of a parsed `Accept` header.
#[derive(Debug, Clone)]
struct MediaRange {
    type_: String,
    subtype: String,
    quality: f32,
}

fn parse_accept(header: &str) -> Vec<MediaRange> {
    header
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            let mut segs = part.split(';');
            let media = segs.next()?.trim();
            let (type_, subtype) = media.split_once('/')?;
            let mut quality = 1.0f32;
            for param in segs {
                let param = param.trim();
                if let Some(q) = param
                    .strip_prefix("q=")
                    .or_else(|| param.strip_prefix("Q="))
                {
                    // A malformed or out-of-range q drops the whole range
                    // (matching http-media), rather than defaulting to 1.0.
                    match q.trim().parse::<f32>() {
                        Ok(v) if (0.0..=1.0).contains(&v) => quality = v,
                        _ => return None,
                    }
                }
            }
            Some(MediaRange {
                type_: type_.trim().to_ascii_lowercase(),
                subtype: subtype.trim().to_ascii_lowercase(),
                quality,
            })
        })
        .collect()
}

/// Specificity of a range matching a concrete server media type: exact=3,
/// `type/*`=2, `*/*`=1, no match = `None`.
fn range_specificity(range: &MediaRange, server: &Mime) -> Option<u8> {
    let s_type = server.type_().as_str();
    let s_sub = server.subtype().as_str();
    match (range.type_.as_str(), range.subtype.as_str()) {
        ("*", "*") => Some(1),
        (t, "*") if t.eq_ignore_ascii_case(s_type) => Some(2),
        (t, st) if t.eq_ignore_ascii_case(s_type) && st.eq_ignore_ascii_case(s_sub) => Some(3),
        _ => None,
    }
}

/// Does a server-provided content type match a concrete request `Content-Type`?
///
/// Request-side matching is **strict**: it compares type/subtype only (params
/// ignored) and does NOT honor wildcards — a request `Content-Type: */*` must
/// not match a concrete declared body codec. (Server-declared codecs are always
/// concrete, so no wildcard handling is needed on either side here.)
pub fn media_type_matches(provided: &Mime, content: &Mime) -> bool {
    media_matches_content(provided, content)
}

fn media_matches_content(provided: &Mime, content: &Mime) -> bool {
    provided.type_() == content.type_() && provided.subtype() == content.subtype()
}

/// Choose the index of the best media type for the request's `Accept` header.
///
/// Missing/empty `Accept` is treated as `*/*` (first option wins). Returns
/// `None` when nothing is acceptable (the caller maps that to 406). Selection
/// maximizes `(client quality, range specificity)`, breaking ties toward the
/// earlier (server-listed) option.
pub fn negotiate_media_index(accept: Option<&str>, media: &[Mime]) -> Option<usize> {
    if media.is_empty() {
        return None;
    }
    let ranges = match accept {
        // Absent or empty Accept means "anything" (`*/*`).
        None => vec![star_range()],
        Some(h) if h.trim().is_empty() => vec![star_range()],
        Some(h) => {
            let parsed = parse_accept(h);
            // Present but containing no valid range (e.g. a malformed q) is a
            // client error: nothing is acceptable (406) — do NOT fall back to */*.
            if parsed.is_empty() {
                return None;
            }
            parsed
        }
    };

    // For each server media type, its quality is that of the MOST SPECIFIC
    // matching Accept range (ties between equal-specificity ranges → higher
    // quality). This makes `application/json;q=0` exclude JSON even when `*/*`
    // (less specific) also matches. Across types: higher quality wins, then
    // higher specificity, then the earlier (server-listed) type.
    let mut best: Option<(usize, f32, u8)> = None; // (idx, quality, specificity)
    for (idx, mt) in media.iter().enumerate() {
        let mut best_for_option: Option<(u8, f32)> = None; // (specificity, quality)
        for r in &ranges {
            if let Some(spec) = range_specificity(r, mt) {
                let cand = (spec, r.quality);
                if best_for_option.is_none_or(|cur| cand > cur) {
                    best_for_option = Some(cand);
                }
            }
        }
        if let Some((spec, q)) = best_for_option {
            if q > 0.0 {
                let better = match best {
                    None => true,
                    Some((_, bq, bspec)) => (q, spec) > (bq, bspec),
                };
                if better {
                    best = Some((idx, q, spec));
                }
            }
        }
    }

    best.map(|(idx, _, _)| idx)
}

/// Pick the best `(media_type, bytes)` for the request's `Accept` header.
///
/// Missing/empty `Accept` is treated as `*/*` (first option wins). Returns
/// `None` when nothing is acceptable (the caller maps that to 406).
pub fn negotiate_accept(accept: Option<&str>, options: &[(Mime, Bytes)]) -> Option<(Mime, Bytes)> {
    let media: Vec<Mime> = options.iter().map(|(m, _)| m.clone()).collect();
    negotiate_media_index(accept, &media).map(|i| options[i].clone())
}

fn star_range() -> MediaRange {
    MediaRange {
        type_: "*".into(),
        subtype: "*".into(),
        quality: 1.0,
    }
}

/// Decode a request body using the content-type list `L`, mirroring
/// `handleCTypeH`. A missing `Content-Type` defaults to `application/octet-stream`
/// (server-request-side default only). Returns `None` when no content type in
/// the list matches (the caller maps that to 415).
pub fn negotiate_content<L: AllMimeUnrender<A>, A>(
    content_type: Option<&str>,
    body: &[u8],
) -> Option<Result<A, String>> {
    let ct: Mime = match content_type {
        Some(s) => s.trim().parse().ok()?,
        None => mime::APPLICATION_OCTET_STREAM,
    };
    L::unrender(&ct, body)
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Thing {
        x: u32,
    }

    #[test]
    fn json_round_trip() {
        let t = Thing { x: 7 };
        let bytes = <Thing as MimeRender<Json>>::mime_render(&t).unwrap();
        assert_eq!(bytes.as_ref(), br#"{"x":7}"#);
        let back: Thing = <Thing as MimeUnrender<Json>>::mime_unrender(&bytes).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn list_media_types_in_order() {
        assert_eq!(
            <(Json, PlainText) as AllMime>::all_media_types(),
            vec![mime::APPLICATION_JSON, mime::TEXT_PLAIN_UTF_8]
        );
        assert_eq!(
            <(Json, PlainText) as AllMime>::primary(),
            mime::APPLICATION_JSON
        );
    }

    #[test]
    fn accept_picks_specific_over_wildcard() {
        let v = "hi".to_string();
        let opts = <(Json, PlainText) as AllMimeRender<String>>::render_all(&v).unwrap();
        // */* and application/json both q=1 -> json (more specific) wins
        let (mt, _) = negotiate_accept(Some("*/*, application/json"), &opts).unwrap();
        assert_eq!(mt, mime::APPLICATION_JSON);
    }

    #[test]
    fn accept_respects_quality() {
        let v = "hi".to_string();
        let opts = <(Json, PlainText) as AllMimeRender<String>>::render_all(&v).unwrap();
        let (mt, _) =
            negotiate_accept(Some("application/json;q=0.3, text/plain;q=0.9"), &opts).unwrap();
        assert_eq!(mt.essence_str(), "text/plain");
    }

    #[test]
    fn accept_missing_defaults_to_first() {
        let opts =
            <(PlainText, Json) as AllMimeRender<String>>::render_all(&"hi".to_string()).unwrap();
        let (mt, _) = negotiate_accept(None, &opts).unwrap();
        assert_eq!(mt.essence_str(), "text/plain");
    }

    #[test]
    fn accept_unsupported_is_none() {
        let opts = <(Json,) as AllMimeRender<Thing>>::render_all(&Thing { x: 1 }).unwrap();
        assert!(negotiate_accept(Some("application/xml"), &opts).is_none());
    }

    #[test]
    fn content_type_match_and_mismatch() {
        let ok: Option<Result<Thing, _>> = negotiate_content::<(Json,), Thing>(
            Some("application/json; charset=utf-8"),
            br#"{"x":3}"#,
        );
        assert_eq!(ok.unwrap().unwrap(), Thing { x: 3 });

        let bad: Option<Result<Thing, _>> =
            negotiate_content::<(Json,), Thing>(Some("application/xml"), b"<x/>");
        assert!(bad.is_none()); // -> 415
    }

    #[test]
    fn missing_content_type_defaults_octet_stream() {
        // octet-stream list accepts the missing-CT default
        let ok: Option<Result<Vec<u8>, _>> =
            negotiate_content::<(OctetStream,), Vec<u8>>(None, b"raw");
        assert_eq!(ok.unwrap().unwrap(), b"raw".to_vec());
        // json-only list rejects a missing CT (defaults to octet-stream, no match)
        let bad: Option<Result<Thing, _>> =
            negotiate_content::<(Json,), Thing>(None, br#"{"x":1}"#);
        assert!(bad.is_none());
    }

    #[test]
    fn nocontent_renders_empty_but_negotiates() {
        let opts = <(Json,) as AllMimeRender<NoContent>>::render_all(&NoContent).unwrap();
        let (mt, body) = negotiate_accept(Some("application/json"), &opts).unwrap();
        assert_eq!(mt, mime::APPLICATION_JSON);
        assert!(body.is_empty());
        assert!(negotiate_accept(Some("application/xml"), &opts).is_none());
    }

    #[test]
    fn q_zero_on_specific_range_excludes_even_with_wildcard() {
        // `application/json;q=0` excludes JSON even though `*/*` (less specific)
        // matches it; text/plain remains acceptable via `*/*`.
        let opts =
            <(Json, PlainText) as AllMimeRender<String>>::render_all(&"hi".to_string()).unwrap();
        let (mt, _) = negotiate_accept(Some("application/json;q=0, */*"), &opts).unwrap();
        assert_eq!(mt.essence_str(), "text/plain");
        // JSON-only list with q=0 -> nothing acceptable -> 406.
        let json = <(Json,) as AllMimeRender<String>>::render_all(&"hi".to_string()).unwrap();
        assert!(negotiate_accept(Some("application/json;q=0"), &json).is_none());
    }

    #[test]
    fn malformed_or_out_of_range_q_drops_the_range() {
        let opts = <(Json,) as AllMimeRender<String>>::render_all(&"hi".to_string()).unwrap();
        // A bad q drops the range entirely (not treated as q=1.0), so nothing matches.
        assert!(negotiate_accept(Some("application/json;q=abc"), &opts).is_none());
        assert!(negotiate_accept(Some("application/json;q=7"), &opts).is_none());
    }

    #[test]
    fn request_content_type_wildcard_is_rejected() {
        // A request `Content-Type: */*` must not match a concrete body codec.
        let r: Option<Result<Thing, _>> =
            negotiate_content::<(Json,), Thing>(Some("*/*"), br#"{"x":1}"#);
        assert!(r.is_none()); // -> 415
    }
}
