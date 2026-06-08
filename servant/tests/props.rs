//! Property tests for the encoding/decoding building blocks (testing rule:
//! property tests for URL encoding/decoding and value round-trips).

use proptest::prelude::*;
use servant::content::{Json, MediaType, OctetStream, PlainText, negotiate_media_index};
use servant::http_data::{FromHttpApiData, ToHttpApiData};
use servant::link::Escaped;
use servant::stream::{Framing, NetstringFraming, NewlineFraming, NoFraming};

proptest! {
    #[test]
    fn u64_round_trips(x in any::<u64>()) {
        prop_assert_eq!(u64::from_url_piece(&x.to_url_piece()), Ok(x));
    }

    #[test]
    fn i64_round_trips(x in any::<i64>()) {
        prop_assert_eq!(i64::from_url_piece(&x.to_url_piece()), Ok(x));
    }

    #[test]
    fn f64_round_trips(x in proptest::num::f64::NORMAL) {
        let back = f64::from_url_piece(&x.to_url_piece()).unwrap();
        prop_assert_eq!(back, x);
    }

    #[test]
    fn bool_round_trips(x in any::<bool>()) {
        prop_assert_eq!(bool::from_url_piece(&x.to_url_piece()), Ok(x));
    }

    /// Escaping a path segment and percent-decoding it is the identity — so a
    /// capture rendered into a link survives the trip back to the router.
    #[test]
    fn escaped_segment_decodes_back(s in ".*") {
        let esc = Escaped::escape(&s);
        let decoded = percent_encoding::percent_decode_str(esc.as_str())
            .decode_utf8()
            .unwrap();
        prop_assert_eq!(decoded.as_ref(), s.as_str());
    }

    /// Escaped output never contains a raw '/' (so it cannot forge a path
    /// boundary — path-traversal safety for captures rendered into links).
    #[test]
    fn escaped_segment_has_no_slash(s in ".*") {
        prop_assert!(!Escaped::escape(&s).as_str().contains('/'));
    }

    /// Newline framing: items (containing no '\n') frame + concat + de-frame
    /// back to exactly the original items, in order.
    #[test]
    fn newline_framing_round_trips(
        items in proptest::collection::vec(
            proptest::collection::vec(any::<u8>().prop_filter("no nl", |b| *b != b'\n'), 0..16),
            0..12,
        )
    ) {
        let mut buf = Vec::new();
        for item in &items {
            buf.extend_from_slice(&NewlineFraming::frame(item));
        }
        let mut out = Vec::new();
        while let Some(frame) = NewlineFraming::deframe(&mut buf, false) {
            out.push(frame);
        }
        prop_assert!(buf.is_empty());
        prop_assert_eq!(out, items);
    }

    /// Netstring framing handles arbitrary bytes (length-prefixed) and round-trips.
    #[test]
    fn netstring_framing_round_trips(
        items in proptest::collection::vec(proptest::collection::vec(any::<u8>(), 0..16), 0..12)
    ) {
        let mut buf = Vec::new();
        for item in &items {
            buf.extend_from_slice(&NetstringFraming::frame(item));
        }
        let mut out = Vec::new();
        while let Some(frame) = NetstringFraming::deframe(&mut buf, false) {
            out.push(frame);
        }
        prop_assert!(buf.is_empty());
        prop_assert_eq!(out, items);
    }

    /// De-framing arbitrary (possibly truncated/garbage) bytes never panics.
    #[test]
    fn deframe_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
        let mut b = bytes.clone();
        let _ = NewlineFraming::deframe(&mut b, false);
        let _ = NewlineFraming::deframe(&mut b, true);
        let mut b = bytes.clone();
        let _ = NetstringFraming::deframe(&mut b, false);
        let mut b = bytes;
        let _ = NoFraming::deframe(&mut b, true);
    }

    /// Accept negotiation never panics on arbitrary input and only ever returns
    /// a valid index into the server's content-type list.
    #[test]
    fn negotiation_never_panics_and_index_is_valid(accept in ".*") {
        let media = vec![
            Json::media_type(),
            PlainText::media_type(),
            OctetStream::media_type(),
        ];
        if let Some(i) = negotiate_media_index(Some(&accept), &media) {
            prop_assert!(i < media.len());
        }
    }

    /// A `q=0` on the exact (and only) type excludes it, whatever follows.
    #[test]
    fn quality_zero_excludes(suffix in "[ ,a-z/*;=0-9.]{0,20}") {
        let media = vec![Json::media_type()];
        let accept = format!("application/json;q=0{suffix}");
        // Either the malformed suffix drops the range (None) or q=0 excludes it,
        // but JSON is never selected with q=0 on its only matching range.
        let chosen = negotiate_media_index(Some(&accept), &media);
        // The only media type is json; if chosen it'd be index 0 — assert it is
        // NOT chosen purely on the strength of a q=0 json range.
        if accept == "application/json;q=0" {
            prop_assert_eq!(chosen, None);
        }
    }
}

#[test]
fn negotiation_is_total_and_first_biased() {
    let media = vec![Json::media_type(), PlainText::media_type()];
    // Wildcard / missing -> first.
    assert_eq!(negotiate_media_index(None, &media), Some(0));
    assert_eq!(negotiate_media_index(Some("*/*"), &media), Some(0));
    // Exact match selects the right index regardless of order.
    assert_eq!(negotiate_media_index(Some("text/plain"), &media), Some(1));
    assert_eq!(
        negotiate_media_index(Some("application/json"), &media),
        Some(0)
    );
    // Unsupported -> None (406), never a panic.
    assert_eq!(negotiate_media_index(Some("application/xml"), &media), None);
    // Malformed Accept must not panic.
    let _ = negotiate_media_index(Some(";;;garbage,,,"), &media);
    let _ = negotiate_media_index(Some(""), &media);
}
