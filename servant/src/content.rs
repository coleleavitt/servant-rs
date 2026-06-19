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

mod codecs;
mod lists;
mod media;
mod negotiation;

pub use codecs::{MimeRender, MimeUnrender, NoContent};
pub use lists::{AllMime, AllMimeRender, AllMimeUnrender};
pub use media::{EventStream, FormUrlEncoded, Json, MediaType, OctetStream, PlainText};
pub use negotiation::{
    media_type_matches,
    negotiate_accept,
    negotiate_content,
    negotiate_media_index,
};

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
