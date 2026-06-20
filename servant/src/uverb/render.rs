use std::pin::Pin;

use bytes::Bytes;
use futures_core::Stream;
use http::StatusCode;
use mime::Mime;

use super::arms::{
    ArmHeaders,
    WithFixedStatus,
    WithStatus,
    WithStatusHeaders,
    WithStatusNoBody,
    WithStreamingStatus,
};
use super::unions::{Union2, Union3, Union4};
use crate::content::{AllMime, AllMimeRender, MediaType, MimeRender, negotiate_media_index};
use crate::stream::Framing;

/// What an arm renders as its response body.
pub enum ArmBody {
    /// A fully-buffered response body.
    Full(Bytes),
    /// A streaming response body, already rendered and framed into byte chunks.
    Stream(Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>>),
}

/// What an arm renders to: status, content type, body, and extra headers.
pub type ArmResponse = (StatusCode, Option<Mime>, ArmBody, ArmHeaders);

/// Render the active arm of a union response over content-type list `CTypes`.
/// Implemented for [`WithStatus`] / [`WithStatusHeaders`] (leaves) and the
/// `Union*` enums (dispatch).
pub trait UnionResponse<CTypes> {
    /// Render the active arm, or `Err` if it failed to serialize (→ `500`).
    fn render_union(self, accept: Option<&str>) -> Result<ArmResponse, String>;
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
    fn render_union(self, accept: Option<&str>) -> Result<ArmResponse, String> {
        let (mime, bytes) = render_arm::<CTypes, T>(&self.0, accept)?;
        Ok((Self::status(), mime, ArmBody::Full(bytes), Vec::new()))
    }
}

impl<const S: u16, CTypes> UnionResponse<CTypes> for WithStatusNoBody<S> {
    fn render_union(self, _accept: Option<&str>) -> Result<ArmResponse, String> {
        Ok((
            Self::status(),
            None,
            ArmBody::Full(Bytes::new()),
            Vec::new(),
        ))
    }
}

impl<const S: u16, CType, T, CTypes> UnionResponse<CTypes> for WithFixedStatus<S, CType, T>
where
    CType: MediaType,
    T: MimeRender<CType>,
{
    fn render_union(self, accept: Option<&str>) -> Result<ArmResponse, String> {
        let media = CType::media_types();
        let index = negotiate_media_index(accept, &media).ok_or("not acceptable")?;
        let bytes = <T as MimeRender<CType>>::mime_render(&self.value)?;
        Ok((
            Self::status(),
            Some(media[index].clone()),
            ArmBody::Full(bytes),
            Vec::new(),
        ))
    }
}

impl<const S: u16, Fr, CType, T, CTypes> UnionResponse<CTypes>
    for WithStreamingStatus<S, Fr, CType, T>
where
    Fr: Framing + 'static,
    CType: MediaType,
    T: MimeRender<CType> + Send + 'static,
{
    fn render_union(self, _accept: Option<&str>) -> Result<ArmResponse, String> {
        use futures_util::StreamExt;

        let stream = self.stream.into_inner().map(|item| {
            <T as MimeRender<CType>>::mime_render(&item).map(|bytes| Fr::frame(&bytes))
        });
        Ok((
            Self::status(),
            Some(CType::media_type()),
            ArmBody::Stream(Box::pin(stream)),
            Vec::new(),
        ))
    }
}

impl<const S: u16, T, CTypes> UnionResponse<CTypes> for WithStatusHeaders<S, T>
where
    CTypes: AllMime + AllMimeRender<T>,
{
    fn render_union(self, accept: Option<&str>) -> Result<ArmResponse, String> {
        let (mime, bytes) = render_arm::<CTypes, T>(&self.value, accept)?;
        Ok((Self::status(), mime, ArmBody::Full(bytes), self.headers))
    }
}

macro_rules! union_response {
    ($ty:ident { $( $variant:ident($g:ident) ),+ }) => {
        impl<$( $g ),+, CTypes> UnionResponse<CTypes> for $ty<$( $g ),+>
        where
            $( $g: UnionResponse<CTypes> ),+
        {
            fn render_union(self, accept: Option<&str>) -> Result<ArmResponse, String> {
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
