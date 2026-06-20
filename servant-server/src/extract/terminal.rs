use bytes::Bytes;
use http::StatusCode;
use mime::Mime;
use servant::api::{Endpoint, NoContentVerb, StreamVerb, UVerb, Verb, VerbWithHeaders};
use servant::content::{
    AllMime,
    AllMimeRender,
    MediaType,
    MimeRender,
    NoContent,
    negotiate_media_index,
};
use servant::error::ServerError;
use servant::hlist::HNil;
use servant::method::MethodMarker;
use servant::stream::{Framing, SourceStream};
use servant::uverb::{ArmBody, UnionResponse};

use super::chain::{Rendered, ServerChain};
use super::state::ExtractState;
use crate::result::RouteResult;

// --- Terminal: Verb ---

impl<M, const STATUS: u16, CTypes, A> ServerChain for Verb<M, STATUS, CTypes, A>
where
    M: MethodMarker,
    CTypes: AllMime + AllMimeRender<A>,
{
    fn validate_captures(
        &self,
        _: &[String],
        _: &mut usize,
        _: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        RouteResult::Route(())
    }
    fn request_content_types(&self) -> Option<Vec<Mime>> {
        None
    }
    fn accept_check(&self, accept: Option<&str>) -> RouteResult<()> {
        let media = CTypes::all_media_types();
        match negotiate_media_index(accept, &media) {
            Some(_) => RouteResult::Route(()),
            None => RouteResult::Fail(ServerError::err406()),
        }
    }
    fn render(&self, accept: Option<&str>, value: A) -> Rendered {
        let status =
            StatusCode::from_u16(STATUS).expect("Verb STATUS must be a valid HTTP status code");
        render_body::<CTypes, A>(accept, &value, status)
    }
    fn extract(&self, _st: &mut ExtractState<'_>) -> RouteResult<HNil> {
        RouteResult::Route(HNil)
    }
}

/// Negotiate + render a value over content-type list `L`, with no extra headers.
/// Renders only the negotiated representation (a serialization failure becomes a
/// clean `500`, and a failure in an *unused* format can't 500 a serviceable
/// request).
fn render_body<L: AllMime + AllMimeRender<A>, A>(
    accept: Option<&str>,
    value: &A,
    status: StatusCode,
) -> Rendered {
    let media = L::all_media_types();
    let idx = negotiate_media_index(accept, &media)
        .expect("accept_check guarantees a negotiable content type");
    match L::render_index(value, idx) {
        Ok(bytes) => (
            status,
            Some(media[idx].clone()),
            crate::response::full_body(bytes),
            Vec::new(),
        ),
        Err(_msg) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            None,
            crate::response::full_body(Bytes::from_static(b"internal serialization error")),
            Vec::new(),
        ),
    }
}

// --- Terminal: VerbWithHeaders ---

impl<M, const STATUS: u16, CTypes, A> ServerChain for VerbWithHeaders<M, STATUS, CTypes, A>
where
    M: MethodMarker,
    CTypes: AllMime + AllMimeRender<A>,
{
    fn validate_captures(
        &self,
        _: &[String],
        _: &mut usize,
        _: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        RouteResult::Route(())
    }
    fn request_content_types(&self) -> Option<Vec<Mime>> {
        None
    }
    fn accept_check(&self, accept: Option<&str>) -> RouteResult<()> {
        let media = CTypes::all_media_types();
        match negotiate_media_index(accept, &media) {
            Some(_) => RouteResult::Route(()),
            None => RouteResult::Fail(ServerError::err406()),
        }
    }
    fn render(&self, accept: Option<&str>, value: servant::response::Headers<A>) -> Rendered {
        let status = StatusCode::from_u16(STATUS)
            .expect("VerbWithHeaders STATUS must be a valid HTTP status code");
        let (inner, headers) = value.into_parts();
        let (status, ct, body, _) = render_body::<CTypes, A>(accept, &inner, status);
        (status, ct, body, headers)
    }
    fn extract(&self, _st: &mut ExtractState<'_>) -> RouteResult<HNil> {
        RouteResult::Route(HNil)
    }
}

// --- Terminal: UVerb (union response) ---

impl<M, CTypes, Resp> ServerChain for UVerb<M, CTypes, Resp>
where
    M: MethodMarker,
    CTypes: AllMime,
    Resp: UnionResponse<CTypes>,
    Self: Endpoint<Output = Resp, Args = HNil>,
{
    fn validate_captures(
        &self,
        _: &[String],
        _: &mut usize,
        _: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        RouteResult::Route(())
    }
    fn request_content_types(&self) -> Option<Vec<Mime>> {
        None
    }
    fn accept_check(&self, accept: Option<&str>) -> RouteResult<()> {
        let media = CTypes::all_media_types();
        match negotiate_media_index(accept, &media) {
            Some(_) => RouteResult::Route(()),
            None => RouteResult::Fail(ServerError::err406()),
        }
    }
    fn render(&self, accept: Option<&str>, value: Resp) -> Rendered {
        match value.render_union(accept) {
            Ok((status, mime, body, headers)) => (status, mime, union_body(body), headers),
            Err(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                None,
                crate::response::full_body(Bytes::from_static(b"internal serialization error")),
                Vec::new(),
            ),
        }
    }
    fn extract(&self, _st: &mut ExtractState<'_>) -> RouteResult<HNil> {
        RouteResult::Route(HNil)
    }
}

fn union_body(body: ArmBody) -> crate::response::ResponseBody {
    match body {
        ArmBody::Full(bytes) => crate::response::full_body(bytes),
        ArmBody::Stream(stream) => {
            use futures_util::StreamExt;
            use http_body::Frame;
            use http_body_util::{BodyExt, StreamBody};

            let framed = stream.map(|chunk| {
                chunk
                    .map(Frame::data)
                    .map_err(|e| -> crate::response::BoxError { std::io::Error::other(e).into() })
            });
            BodyExt::boxed_unsync(StreamBody::new(framed))
        }
    }
}

// --- Terminal: StreamVerb (chunked streaming body) ---

impl<M, const STATUS: u16, Fr, CType, T> ServerChain for StreamVerb<M, STATUS, Fr, CType, T>
where
    M: MethodMarker,
    Fr: Framing + 'static,
    CType: MediaType,
    T: MimeRender<CType> + Send + 'static,
    Self: Endpoint<Output = SourceStream<T>, Args = HNil>,
{
    fn validate_captures(
        &self,
        _: &[String],
        _: &mut usize,
        _: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        RouteResult::Route(())
    }
    fn request_content_types(&self) -> Option<Vec<Mime>> {
        None
    }
    fn accept_check(&self, accept: Option<&str>) -> RouteResult<()> {
        let media = [CType::media_type()];
        match negotiate_media_index(accept, &media) {
            Some(_) => RouteResult::Route(()),
            None => RouteResult::Fail(ServerError::err406()),
        }
    }
    fn render(&self, _accept: Option<&str>, value: SourceStream<T>) -> Rendered {
        use futures_util::StreamExt;
        use http_body::Frame;
        use http_body_util::{BodyExt, StreamBody};

        let status = StatusCode::from_u16(STATUS).expect("StreamVerb STATUS must be valid");
        let ct = CType::media_type();
        let framed =
            value
                .into_inner()
                .map(|item| match <T as MimeRender<CType>>::mime_render(&item) {
                    Ok(bytes) => Ok::<_, crate::response::BoxError>(Frame::data(Fr::frame(&bytes))),
                    Err(e) => Err(e.into()),
                });
        let body = BodyExt::boxed_unsync(StreamBody::new(framed));
        (status, Some(ct), body, Vec::new())
    }
    fn extract(&self, _st: &mut ExtractState<'_>) -> RouteResult<HNil> {
        RouteResult::Route(HNil)
    }
}

// --- Terminal: NoContentVerb ---

impl<M> ServerChain for NoContentVerb<M>
where
    M: MethodMarker,
{
    fn validate_captures(
        &self,
        _: &[String],
        _: &mut usize,
        _: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        RouteResult::Route(())
    }
    fn request_content_types(&self) -> Option<Vec<Mime>> {
        None
    }
    fn accept_check(&self, _accept: Option<&str>) -> RouteResult<()> {
        RouteResult::Route(()) // 204 performs no content negotiation
    }
    fn render(&self, _accept: Option<&str>, _value: NoContent) -> Rendered {
        (
            StatusCode::NO_CONTENT,
            None,
            crate::response::full_body(Bytes::new()),
            Vec::new(),
        )
    }
    fn extract(&self, _st: &mut ExtractState<'_>) -> RouteResult<HNil> {
        RouteResult::Route(HNil)
    }
}
