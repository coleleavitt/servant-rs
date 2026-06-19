use bytes::Bytes;
use mime::Mime;

use super::lists::AllMimeUnrender;

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

pub(crate) fn media_matches_content(provided: &Mime, content: &Mime) -> bool {
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
