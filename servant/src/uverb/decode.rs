use http::{HeaderMap, StatusCode};
use mime::Mime;

use super::arms::{
    WithFixedStatus,
    WithStatus,
    WithStatusHeaders,
    WithStatusNoBody,
    WithStreamingStatus,
};
use super::unions::{Union2, Union3, Union4};
use crate::content::{AllMime, AllMimeUnrender, MediaType, MimeUnrender, media_type_matches};
use crate::stream::{Framing, SourceStream};

/// Decode a response into a union arm by matching its status code. Returns
/// `None` when no arm declares the response's status (the client maps that to a
/// `FailureResponse`). `WithStatusHeaders` arms also capture the response headers.
pub trait UnionDecode<CTypes>: Sized {
    /// Try to decode; `None` = no arm matched the status.
    fn decode_union(
        status: StatusCode,
        headers: &HeaderMap,
        ct: Option<&Mime>,
        body: &[u8],
    ) -> Option<Result<Self, String>>;
}

fn decode_arm<CTypes, T>(ct: Option<&Mime>, body: &[u8]) -> Result<T, String>
where
    CTypes: AllMime + AllMimeUnrender<T>,
{
    let ct = ct.cloned().unwrap_or(mime::APPLICATION_OCTET_STREAM);
    match CTypes::unrender(&ct, body) {
        Some(Ok(t)) => Ok(t),
        Some(Err(e)) => Err(e),
        None => Err(format!("unsupported content type: {ct}")),
    }
}

fn ensure_fixed_media<CType>(ct: Option<&Mime>) -> Result<Mime, String>
where
    CType: MediaType,
{
    let ct = ct.cloned().unwrap_or(mime::APPLICATION_OCTET_STREAM);
    let supported = CType::media_types()
        .into_iter()
        .any(|expected| media_type_matches(&expected, &ct));
    if !supported {
        return Err(format!("unsupported content type: {ct}"));
    }
    Ok(ct)
}

fn decode_fixed_arm<CType, T>(ct: Option<&Mime>, body: &[u8]) -> Result<T, String>
where
    CType: MediaType,
    T: MimeUnrender<CType>,
{
    ensure_fixed_media::<CType>(ct)?;
    <T as MimeUnrender<CType>>::mime_unrender(body)
}

fn decode_stream_arm<Fr, CType, T>(
    ct: Option<&Mime>,
    body: &[u8],
) -> Result<SourceStream<T>, String>
where
    Fr: Framing + 'static,
    CType: MediaType + 'static,
    T: MimeUnrender<CType> + Send + 'static,
{
    let mut buf = body.to_vec();
    let mut items = Vec::new();
    while let Some(frame) = Fr::deframe(&mut buf, false) {
        items.push(<T as MimeUnrender<CType>>::mime_unrender(&frame)?);
    }
    while let Some(frame) = Fr::deframe(&mut buf, true) {
        items.push(<T as MimeUnrender<CType>>::mime_unrender(&frame)?);
    }
    if !buf.is_empty() {
        return Err("incomplete streaming response frame".to_string());
    }
    ensure_fixed_media::<CType>(ct)?;
    Ok(SourceStream::new(futures_util::stream::iter(items)))
}

impl<const S: u16, T, CTypes> UnionDecode<CTypes> for WithStatus<S, T>
where
    CTypes: AllMime + AllMimeUnrender<T>,
{
    fn decode_union(
        status: StatusCode,
        _headers: &HeaderMap,
        ct: Option<&Mime>,
        body: &[u8],
    ) -> Option<Result<Self, String>> {
        if status.as_u16() != S {
            return None;
        }
        Some(decode_arm::<CTypes, T>(ct, body).map(WithStatus))
    }
}

impl<const S: u16, CTypes> UnionDecode<CTypes> for WithStatusNoBody<S> {
    fn decode_union(
        status: StatusCode,
        _headers: &HeaderMap,
        _ct: Option<&Mime>,
        _body: &[u8],
    ) -> Option<Result<Self, String>> {
        if status.as_u16() != S {
            return None;
        }
        Some(Ok(WithStatusNoBody))
    }
}

impl<const S: u16, CType, T, CTypes> UnionDecode<CTypes> for WithFixedStatus<S, CType, T>
where
    CType: MediaType,
    T: MimeUnrender<CType>,
{
    fn decode_union(
        status: StatusCode,
        _headers: &HeaderMap,
        ct: Option<&Mime>,
        body: &[u8],
    ) -> Option<Result<Self, String>> {
        if status.as_u16() != S {
            return None;
        }
        Some(decode_fixed_arm::<CType, T>(ct, body).map(WithFixedStatus::new))
    }
}

impl<const S: u16, Fr, CType, T, CTypes> UnionDecode<CTypes>
    for WithStreamingStatus<S, Fr, CType, T>
where
    Fr: Framing + 'static,
    CType: MediaType + 'static,
    T: MimeUnrender<CType> + Send + 'static,
{
    fn decode_union(
        status: StatusCode,
        _headers: &HeaderMap,
        ct: Option<&Mime>,
        body: &[u8],
    ) -> Option<Result<Self, String>> {
        if status.as_u16() != S {
            return None;
        }
        Some(decode_stream_arm::<Fr, CType, T>(ct, body).map(WithStreamingStatus::new))
    }
}

impl<const S: u16, T, CTypes> UnionDecode<CTypes> for WithStatusHeaders<S, T>
where
    CTypes: AllMime + AllMimeUnrender<T>,
{
    fn decode_union(
        status: StatusCode,
        headers: &HeaderMap,
        ct: Option<&Mime>,
        body: &[u8],
    ) -> Option<Result<Self, String>> {
        if status.as_u16() != S {
            return None;
        }
        Some(decode_arm::<CTypes, T>(ct, body).map(|value| {
            WithStatusHeaders {
                value,
                headers: headers
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            }
        }))
    }
}

macro_rules! union_decode {
    ($ty:ident { $( $variant:ident($g:ident) ),+ }) => {
        impl<$( $g ),+, CTypes> UnionDecode<CTypes> for $ty<$( $g ),+>
        where
            $( $g: UnionDecode<CTypes> ),+
        {
            fn decode_union(status: StatusCode, headers: &HeaderMap, ct: Option<&Mime>, body: &[u8]) -> Option<Result<Self, String>> {
                $(
                    if let Some(r) = $g::decode_union(status, headers, ct, body) {
                        return Some(r.map($ty::$variant));
                    }
                )+
                None
            }
        }
    };
}

union_decode!(Union2 { V0(A), V1(B) });
union_decode!(Union3 { V0(A), V1(B), V2(C) });
union_decode!(Union4 { V0(A), V1(B), V2(C), V3(D) });
