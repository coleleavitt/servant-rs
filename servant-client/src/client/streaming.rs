use servant::api::{
    Capture,
    CaptureAll,
    Description,
    Endpoint,
    Header,
    Path,
    QueryFlag,
    QueryParam,
    QueryParams,
    ReqBody,
    StreamVerb,
    Summary,
};
use servant::content::{MediaType, MimeUnrender};
use servant::method::MethodMarker;
use servant::stream::{Framing, SourceStream};

use super::endpoint::{ClientEndpoint, HasClient};
use crate::request::{ClientError, ClientRequest, ClientResponse};
use crate::runclient::RunStreamingClient;

// ---------------------------------------------------------------------------
// Streaming client (for `StreamVerb` / SSE endpoints)
// ---------------------------------------------------------------------------

// A streaming endpoint builds its request like any other (method + accept) but
// is decoded via `ClientEndpoint::call_stream`, not the buffered `decode`.
impl<M, const STATUS: u16, Fr, CType, T> HasClient for StreamVerb<M, STATUS, Fr, CType, T>
where
    M: MethodMarker,
    CType: MediaType,
    Self: Endpoint<Output = SourceStream<T>, Args = servant::hlist::HNil>,
{
    fn build_request(&self, _args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        req.method = M::method();
        req.accept = vec![CType::media_type()];
        Ok(())
    }
    fn decode(&self, _resp: ClientResponse) -> Result<Self::Output, ClientError> {
        Err(ClientError::ConnectionError(
            "streaming endpoint: use `call_stream` instead of `call`".into(),
        ))
    }
}

/// Surfaces a streaming endpoint's framing, content type, and item type through
/// the chain (forwarded by every combinator to the terminal `StreamVerb`).
pub trait StreamInfo {
    /// The framing strategy.
    type Framing;
    /// The per-item content type marker.
    type CType;
    /// The streamed item type.
    type Item;
}

impl<M, const STATUS: u16, Fr, CType, T> StreamInfo for StreamVerb<M, STATUS, Fr, CType, T> {
    type Framing = Fr;
    type CType = CType;
    type Item = T;
}

macro_rules! forward_stream_info {
    ($ty:ident < $($g:ident),+ >) => {
        impl<$($g),+, Next: StreamInfo> StreamInfo for $ty<$($g),+, Next> {
            type Framing = Next::Framing;
            type CType = Next::CType;
            type Item = Next::Item;
        }
    };
    ($ty:ident) => {
        impl<Next: StreamInfo> StreamInfo for $ty<Next> {
            type Framing = Next::Framing;
            type CType = Next::CType;
            type Item = Next::Item;
        }
    };
}
forward_stream_info!(Path);
forward_stream_info!(Capture<A, S>);
forward_stream_info!(CaptureAll<A>);
forward_stream_info!(QueryParam<A, P, S>);
forward_stream_info!(QueryParams<A>);
forward_stream_info!(QueryFlag);
forward_stream_info!(Header<A, P, S>);
forward_stream_info!(ReqBody<CTypes, A, S>);
forward_stream_info!(Description);
forward_stream_info!(Summary);

/// De-frame and decode a streaming byte body into typed items.
fn deframe_decode<Fr, CType, Item>(
    body: std::pin::Pin<
        Box<dyn futures_core::Stream<Item = Result<bytes::Bytes, ClientError>> + Send>,
    >,
) -> impl futures_core::Stream<Item = Result<Item, String>> + Send
where
    Fr: Framing + 'static,
    CType: MediaType + 'static,
    Item: MimeUnrender<CType> + Send + 'static,
{
    use futures_util::StreamExt;

    struct State {
        body: std::pin::Pin<
            Box<dyn futures_core::Stream<Item = Result<bytes::Bytes, ClientError>> + Send>,
        >,
        buf: Vec<u8>,
        eof: bool,
    }

    futures_util::stream::unfold(
        State {
            body,
            buf: Vec::new(),
            eof: false,
        },
        |mut st| async move {
            loop {
                if let Some(frame) = Fr::deframe(&mut st.buf, st.eof) {
                    let item = <Item as MimeUnrender<CType>>::mime_unrender(&frame);
                    return Some((item, st));
                }
                if st.eof {
                    return None;
                }
                match st.body.next().await {
                    Some(Ok(chunk)) => st.buf.extend_from_slice(&chunk),
                    Some(Err(e)) => {
                        st.eof = true;
                        return Some((Err(e.to_string()), st));
                    }
                    None => st.eof = true,
                }
            }
        },
    )
}

impl<Api> ClientEndpoint<Api>
where
    Api: HasClient + StreamInfo,
    Api::Framing: Framing + 'static,
    Api::CType: MediaType + 'static,
    Api::Item: MimeUnrender<Api::CType> + Send + 'static,
{
    /// Call a streaming endpoint, returning a stream of decoded items. Each item
    /// is `Result<Item, String>` (a per-item decode error does not end the
    /// stream). Requires a [`RunStreamingClient`] transport.
    pub async fn call_stream<T: RunStreamingClient>(
        &self,
        transport: &T,
        args: Api::Args,
    ) -> Result<SourceStream<Result<Api::Item, String>>, ClientError> {
        let mut req = ClientRequest::new();
        self.chain
            .build_request(args, &mut req)
            .map_err(|message| ClientError::EncodeFailure { message })?;
        let resp = transport.run_streaming(req).await?;
        let stream = deframe_decode::<Api::Framing, Api::CType, Api::Item>(resp.body);
        Ok(SourceStream::new(stream))
    }
}
