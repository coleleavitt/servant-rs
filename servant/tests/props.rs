//! Property tests for the encoding/decoding building blocks (testing rule:
//! property tests for URL encoding/decoding and value round-trips).

use proptest::prelude::*;
use servant::content::{Json, MediaType, PlainText, negotiate_media_index};
use servant::http_data::{FromHttpApiData, ToHttpApiData};
use servant::link::Escaped;

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
