//! Union responses, mirroring `Servant.API.UVerb`.
//!
//! A `UVerb` endpoint can return one of several response types, each with its
//! own status code. The handler returns a union value ([`Union2`]/[`Union3`]/
//! [`Union4`]) whose arms are [`WithStatus`]-tagged values; the server renders
//! the active arm (negotiating its body over the endpoint's content types) with
//! that arm's status, and the client decodes by matching the response status.

use bytes::Bytes;
use http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use mime::Mime;

use crate::content::{AllMime, AllMimeRender, AllMimeUnrender, negotiate_media_index};

/// Extra response headers carried by a union arm (`WithStatusHeaders`).
pub type ArmHeaders = Vec<(HeaderName, HeaderValue)>;

/// A response value tagged with the HTTP status it should carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithStatus<const S: u16, T>(pub T);

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
    /// Attach a header parsed from strings (malformed is ignored).
    pub fn try_header(mut self, name: &str, value: &str) -> Self {
        if let (Ok(n), Ok(v)) = (HeaderName::try_from(name), HeaderValue::try_from(value)) {
            self.headers.push((n, v));
        }
        self
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

/// A two-arm response union.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Union2<A, B> {
    /// First arm.
    V0(A),
    /// Second arm.
    V1(B),
}

/// A three-arm response union.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Union3<A, B, C> {
    /// First arm.
    V0(A),
    /// Second arm.
    V1(B),
    /// Third arm.
    V2(C),
}

/// A four-arm response union.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Union4<A, B, C, D> {
    /// First arm.
    V0(A),
    /// Second arm.
    V1(B),
    /// Third arm.
    V2(C),
    /// Fourth arm.
    V3(D),
}

// ---------------------------------------------------------------------------
// Server-side rendering and client-side decoding of a union
// ---------------------------------------------------------------------------

/// What an arm renders to: status, content type, body, and extra headers.
pub type ArmResponse = (StatusCode, Option<Mime>, Bytes, ArmHeaders);

/// Render the active arm of a union response over content-type list `CTypes`.
/// Implemented for [`WithStatus`] / [`WithStatusHeaders`] (leaves) and the
/// `Union*` enums (dispatch).
pub trait UnionResponse<CTypes> {
    /// Render the active arm, or `Err` if it failed to serialize (→ `500`).
    fn render_union(&self, accept: Option<&str>) -> Result<ArmResponse, String>;
}

fn render_arm<CTypes, T>(value: &T, accept: Option<&str>) -> Result<(Option<Mime>, Bytes), String>
where
    CTypes: AllMime + AllMimeRender<T>,
{
    let media = CTypes::all_media_types();
    let idx = negotiate_media_index(accept, &media).ok_or("not acceptable")?;
    let bytes = CTypes::render_index(value, idx)?;
    Ok((Some(media[idx].clone()), bytes))
}

impl<const S: u16, T, CTypes> UnionResponse<CTypes> for WithStatus<S, T>
where
    CTypes: AllMime + AllMimeRender<T>,
{
    fn render_union(&self, accept: Option<&str>) -> Result<ArmResponse, String> {
        let (mime, bytes) = render_arm::<CTypes, T>(&self.0, accept)?;
        Ok((Self::status(), mime, bytes, Vec::new()))
    }
}

impl<const S: u16, T, CTypes> UnionResponse<CTypes> for WithStatusHeaders<S, T>
where
    CTypes: AllMime + AllMimeRender<T>,
{
    fn render_union(&self, accept: Option<&str>) -> Result<ArmResponse, String> {
        let (mime, bytes) = render_arm::<CTypes, T>(&self.value, accept)?;
        Ok((Self::status(), mime, bytes, self.headers.clone()))
    }
}

macro_rules! union_response {
    ($ty:ident { $( $variant:ident($g:ident) ),+ }) => {
        impl<$( $g ),+, CTypes> UnionResponse<CTypes> for $ty<$( $g ),+>
        where
            $( $g: UnionResponse<CTypes> ),+
        {
            fn render_union(&self, accept: Option<&str>) -> Result<ArmResponse, String> {
                match self {
                    $( $ty::$variant(x) => x.render_union(accept) ),+
                }
            }
        }
    };
}
union_response!(Union2 { V0(A), V1(B) });
union_response!(Union3 { V0(A), V1(B), V2(C) });
union_response!(Union4 { V0(A), V1(B), V2(C), V3(D) });

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::Json;

    #[test]
    fn renders_active_arm_status_and_body() {
        type Resp = Union2<WithStatus<200, u32>, WithStatus<404, String>>;
        let ok: Resp = Union2::V0(WithStatus::new(7u32));
        let (status, mt, body, _h) =
            UnionResponse::<(Json,)>::render_union(&ok, Some("application/json")).unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(mt.unwrap(), mime::APPLICATION_JSON);
        assert_eq!(&body[..], b"7");

        let nf: Resp = Union2::V1(WithStatus::new("nope".to_string()));
        let (status, _, body, _h) =
            UnionResponse::<(Json,)>::render_union(&nf, Some("application/json")).unwrap();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(&body[..], br#""nope""#);
    }

    #[test]
    fn decodes_by_status() {
        type Resp = Union2<WithStatus<200, u32>, WithStatus<404, String>>;
        let empty = HeaderMap::new();
        let r: Resp = <Resp as UnionDecode<(Json,)>>::decode_union(
            StatusCode::NOT_FOUND,
            &empty,
            Some(&mime::APPLICATION_JSON),
            br#""nope""#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(r, Union2::V1(WithStatus::new("nope".to_string())));
        // A status no arm declares -> None (FailureResponse).
        assert!(
            <Resp as UnionDecode<(Json,)>>::decode_union(
                StatusCode::INTERNAL_SERVER_ERROR,
                &empty,
                Some(&mime::APPLICATION_JSON),
                b"",
            )
            .is_none()
        );
    }

    #[test]
    fn arm_with_headers_round_trips_status_headers_and_body() {
        type Resp = Union2<WithStatusHeaders<201, u32>, WithStatus<404, String>>;
        let created: Resp =
            Union2::V0(WithStatusHeaders::new(9u32).try_header("location", "/things/9"));
        let (status, _, body, headers) =
            UnionResponse::<(Json,)>::render_union(&created, Some("application/json")).unwrap();
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(&body[..], b"9");
        assert_eq!(headers[0].0.as_str(), "location");
    }
}
