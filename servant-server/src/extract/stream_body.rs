use mime::Mime;
use servant::api::{Endpoint, StreamBody};
use servant::content::{MediaType, MimeUnrender};
use servant::hlist::HCons;
use servant::stream::{Framing, SourceStream, StreamBodyError};

use super::chain::{Rendered, RequestBodyMode, ServerChain, cons_tail};
use super::state::ExtractState;
use crate::result::RouteResult;

impl<Fr, CType, T, Next> ServerChain for StreamBody<Fr, CType, T, Next>
where
    Fr: Framing + Send + 'static,
    CType: MediaType + Send + 'static,
    T: MimeUnrender<CType> + Send + 'static,
    Next: ServerChain,
    Self: Endpoint<
            Args = HCons<SourceStream<Result<T, StreamBodyError>>, Next::Args>,
            Output = Next::Output,
        >,
{
    fn validate_captures(
        &self,
        captures: &[String],
        index: &mut usize,
        capture_all: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        self.next.validate_captures(captures, index, capture_all)
    }

    fn request_content_types(&self) -> Option<Vec<Mime>> {
        Some(vec![CType::media_type()])
    }

    fn request_body_mode(&self) -> Option<RequestBodyMode> {
        Some(RequestBodyMode::Streaming)
    }

    fn host_check(&self, req: &crate::request::RequestData) -> RouteResult<()> {
        self.next.host_check(req)
    }

    fn accept_check(&self, accept: Option<&str>) -> RouteResult<()> {
        self.next.accept_check(accept)
    }

    fn pre_body_check(&self, state: &mut ExtractState<'_>) -> RouteResult<()> {
        self.next.pre_body_check(state)
    }

    fn render(&self, accept: Option<&str>, value: Self::Output) -> Rendered {
        self.next.render(accept, value)
    }

    fn extract(&self, state: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        let body = match state.req.take_body_stream() {
            Ok(body) => body,
            Err(error) => return RouteResult::FailFatal(error),
        };
        let stream = crate::stream_body::decode_stream::<Fr, CType, T>(body);
        cons_tail(stream, &self.next, state)
    }
}
