use servant::api::{Endpoint, StreamBody};
use servant::content::{MediaType, MimeRender};
use servant::hlist::HCons;
use servant::stream::{Framing, SourceStream, StreamBodyError};

use super::super::endpoint::HasClient;
use crate::request::{
    ClientError,
    ClientRequest,
    ClientResponse,
    RequestByteStream,
    StreamingRequestBody,
};

impl<Fr, CType, T, Next> HasClient for StreamBody<Fr, CType, T, Next>
where
    Fr: Framing + 'static,
    CType: MediaType + 'static,
    T: MimeRender<CType> + Send + 'static,
    Next: HasClient,
    Self: Endpoint<
            Args = HCons<SourceStream<Result<T, StreamBodyError>>, Next::Args>,
            Output = Next::Output,
        >,
{
    fn build_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        let HCons { head, tail } = args;
        let stream = encode_stream_body::<Fr, CType, T>(head);
        req.set_streaming_body(StreamingRequestBody::new(CType::media_type(), stream));
        self.next.build_request(tail, req)
    }

    fn decode(&self, resp: ClientResponse) -> Result<Self::Output, ClientError> {
        self.next.decode(resp)
    }
}

fn encode_stream_body<Fr, CType, T>(
    source: SourceStream<Result<T, StreamBodyError>>,
) -> RequestByteStream
where
    Fr: Framing + 'static,
    CType: MediaType + 'static,
    T: MimeRender<CType> + Send + 'static,
{
    use futures_util::StreamExt;

    let stream = source.into_inner().map(|item| {
        let value = item.map_err(|error| ClientError::EncodeFailure {
            message: error.to_string(),
        })?;
        let rendered = <T as MimeRender<CType>>::mime_render(&value)
            .map_err(|message| ClientError::EncodeFailure { message })?;
        Ok(Fr::frame(&rendered))
    });
    Box::pin(stream)
}
