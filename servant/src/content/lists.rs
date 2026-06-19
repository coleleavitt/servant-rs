use bytes::Bytes;
use mime::Mime;

use super::codecs::{MimeRender, MimeUnrender};
use super::media::MediaType;
use super::negotiation::media_matches_content;

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
