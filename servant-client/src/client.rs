//! The client interpretation: walk the API description to build a request from
//! the handler argument list, then decode the response.
//!
//! Mirrors `Servant.Client.Core.HasClient`. `build_request` consumes the same
//! `Args` HList the server produces (so client and server cannot drift) and the
//! decode step enforces the endpoint's declared status and content types.

use std::str::FromStr;
use std::sync::Arc;

use http::{HeaderName, HeaderValue, StatusCode};
use mime::Mime;
use servant::api::{
    Alt,
    Capture,
    CaptureAll,
    Description,
    Endpoint,
    Header,
    NoContentVerb,
    Path,
    QueryFlag,
    QueryParam,
    QueryParams,
    ReqBody,
    StreamVerb,
    Summary,
    UVerb,
    Verb,
    VerbWithHeaders,
};
use servant::content::{
    AllMime,
    AllMimeRender,
    AllMimeUnrender,
    MediaType,
    MimeUnrender,
    NoContent,
};
use servant::hlist::HCons;
use servant::http_data::ToHttpApiData;
use servant::method::MethodMarker;
use servant::modifiers::{ArgShape, CaptureShape, Required};
use servant::stream::{Framing, SourceStream};
use servant::uverb::UnionDecode;

use crate::request::{ClientError, ClientRequest, ClientResponse};
use crate::runclient::{RunClient, RunStreamingClient};

/// The client interpretation of a single endpoint chain.
pub trait HasClient: Endpoint {
    /// Build the request by consuming the argument list in combinator order.
    /// Fails only if a request body cannot be encoded into its content type.
    fn build_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String>;
    /// Decode the response into the endpoint's output (checking status + type).
    fn decode(&self, resp: ClientResponse) -> Result<Self::Output, ClientError>;
}

// --- Terminal: Verb ---

impl<M, const STATUS: u16, CTypes, A> HasClient for Verb<M, STATUS, CTypes, A>
where
    M: MethodMarker,
    CTypes: AllMime + AllMimeUnrender<A>,
    Self: Endpoint<Output = A, Args = servant::hlist::HNil>,
{
    fn build_request(&self, _args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        req.method = M::method();
        req.accept = CTypes::all_media_types();
        Ok(())
    }
    fn decode(&self, resp: ClientResponse) -> Result<Self::Output, ClientError> {
        let expected = StatusCode::from_u16(STATUS).expect("valid status");
        if resp.status != expected {
            return Err(ClientError::FailureResponse { response: resp });
        }
        decode_body::<CTypes, A>(resp)
    }
}

// --- Terminal: VerbWithHeaders ---

impl<M, const STATUS: u16, CTypes, A> HasClient for VerbWithHeaders<M, STATUS, CTypes, A>
where
    M: MethodMarker,
    CTypes: AllMime + AllMimeUnrender<A>,
    Self: Endpoint<Output = servant::response::Headers<A>, Args = servant::hlist::HNil>,
{
    fn build_request(&self, _args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        req.method = M::method();
        req.accept = CTypes::all_media_types();
        Ok(())
    }
    fn decode(&self, resp: ClientResponse) -> Result<Self::Output, ClientError> {
        let expected = StatusCode::from_u16(STATUS).expect("valid status");
        if resp.status != expected {
            return Err(ClientError::FailureResponse { response: resp });
        }
        // Capture the response headers, then decode the body into the value.
        let headers: Vec<(http::HeaderName, http::HeaderValue)> = resp
            .headers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let value = decode_body::<CTypes, A>(resp)?;
        let mut out = servant::response::Headers::new(value);
        for (k, v) in headers {
            out = out.header(k, v);
        }
        Ok(out)
    }
}

// --- Terminal: UVerb (union response, decoded by status) ---

impl<M, CTypes, Resp> HasClient for UVerb<M, CTypes, Resp>
where
    M: MethodMarker,
    CTypes: AllMime,
    Resp: UnionDecode<CTypes>,
    Self: Endpoint<Output = Resp, Args = servant::hlist::HNil>,
{
    fn build_request(&self, _args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        req.method = M::method();
        req.accept = CTypes::all_media_types();
        Ok(())
    }
    fn decode(&self, resp: ClientResponse) -> Result<Self::Output, ClientError> {
        let ct = resp
            .headers
            .get(http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<Mime>().ok());
        match Resp::decode_union(resp.status, &resp.headers, ct.as_ref(), &resp.body) {
            Some(Ok(v)) => Ok(v),
            Some(Err(message)) => Err(ClientError::DecodeFailure {
                message,
                response: resp,
            }),
            None => Err(ClientError::FailureResponse { response: resp }),
        }
    }
}

// --- Terminal: NoContentVerb ---

impl<M> HasClient for NoContentVerb<M>
where
    M: MethodMarker,
    Self: Endpoint<Output = NoContent, Args = servant::hlist::HNil>,
{
    fn build_request(&self, _args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        req.method = M::method();
        Ok(())
    }
    fn decode(&self, resp: ClientResponse) -> Result<Self::Output, ClientError> {
        if resp.status != StatusCode::NO_CONTENT {
            return Err(ClientError::FailureResponse { response: resp });
        }
        Ok(NoContent)
    }
}

/// Decode a response body for content-type list `L` into `A`, distinguishing the
/// Servant `ClientError` variants. A missing `Content-Type` defaults to
/// `application/octet-stream` (client-side default, separate from the server's).
fn decode_body<L: AllMimeUnrender<A>, A>(resp: ClientResponse) -> Result<A, ClientError> {
    let ct: Mime = match resp.headers.get(http::header::CONTENT_TYPE) {
        None => mime::APPLICATION_OCTET_STREAM,
        Some(v) => match v.to_str().ok().and_then(|s| s.parse::<Mime>().ok()) {
            Some(m) => m,
            None => return Err(ClientError::InvalidContentTypeHeader { response: resp }),
        },
    };
    match L::unrender(&ct, &resp.body) {
        Some(Ok(v)) => Ok(v),
        Some(Err(message)) => Err(ClientError::DecodeFailure {
            message,
            response: resp,
        }),
        None => Err(ClientError::UnsupportedContentType {
            media_type: ct,
            response: resp,
        }),
    }
}

// --- forwarding helpers ---

macro_rules! forward_decode {
    () => {
        fn decode(&self, resp: ClientResponse) -> Result<Self::Output, ClientError> {
            self.next.decode(resp)
        }
    };
}

impl<Next: HasClient> HasClient for Path<Next>
where
    Self: Endpoint<Output = Next::Output, Args = Next::Args>,
{
    fn build_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        req.append_path(&self.segment);
        self.next.build_request(args, req)
    }
    forward_decode!();
}

macro_rules! metadata_client {
    ($ty:ident) => {
        impl<Next: HasClient> HasClient for $ty<Next>
        where
            Self: Endpoint<Output = Next::Output, Args = Next::Args>,
        {
            fn build_request(
                &self,
                args: Self::Args,
                req: &mut ClientRequest,
            ) -> Result<(), String> {
                self.next.build_request(args, req)
            }
            forward_decode!();
        }
    };
}
metadata_client!(Description);
metadata_client!(Summary);

impl<A, S, Next> HasClient for Capture<A, S, Next>
where
    A: ToHttpApiData,
    S: CaptureShape<A>,
    Next: HasClient,
    Self: Endpoint<Args = HCons<<S as CaptureShape<A>>::Out, Next::Args>, Output = Next::Output>,
{
    fn build_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        let HCons { head, tail } = args;
        match <S as CaptureShape<A>>::into_value(head) {
            Some(a) => req.append_path(&a.to_url_piece()),
            None => req.append_path(""),
        }
        self.next.build_request(tail, req)
    }
    forward_decode!();
}

impl<A, Next> HasClient for CaptureAll<A, Next>
where
    A: ToHttpApiData,
    Next: HasClient,
    Self: Endpoint<Args = HCons<Vec<A>, Next::Args>, Output = Next::Output>,
{
    fn build_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        let HCons { head, tail } = args;
        for a in &head {
            req.append_path(&a.to_url_piece());
        }
        self.next.build_request(tail, req)
    }
    forward_decode!();
}

impl<A, P, S, Next> HasClient for QueryParam<A, P, S, Next>
where
    A: ToHttpApiData,
    (P, S): ArgShape<A>,
    Next: HasClient,
    Self: Endpoint<Args = HCons<<(P, S) as ArgShape<A>>::Out, Next::Args>, Output = Next::Output>,
{
    fn build_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        let HCons { head, tail } = args;
        if let Some(a) = <(P, S) as ArgShape<A>>::into_value(head) {
            req.append_query(&self.name, Some(a.to_query_param()));
        }
        self.next.build_request(tail, req)
    }
    forward_decode!();
}

impl<A, Next> HasClient for QueryParams<A, Next>
where
    A: ToHttpApiData,
    Next: HasClient,
    Self: Endpoint<Args = HCons<Vec<A>, Next::Args>, Output = Next::Output>,
{
    fn build_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        let HCons { head, tail } = args;
        for a in &head {
            req.append_query(&self.name, Some(a.to_query_param()));
        }
        self.next.build_request(tail, req)
    }
    forward_decode!();
}

impl<Next> HasClient for QueryFlag<Next>
where
    Next: HasClient,
    Self: Endpoint<Args = HCons<bool, Next::Args>, Output = Next::Output>,
{
    fn build_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        let HCons { head, tail } = args;
        if head {
            req.append_query(&self.name, None);
        }
        self.next.build_request(tail, req)
    }
    forward_decode!();
}

impl<A, P, S, Next> HasClient for Header<A, P, S, Next>
where
    A: ToHttpApiData,
    (P, S): ArgShape<A>,
    Next: HasClient,
    Self: Endpoint<Args = HCons<<(P, S) as ArgShape<A>>::Out, Next::Args>, Output = Next::Output>,
{
    fn build_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        let HCons { head, tail } = args;
        if let Some(a) = <(P, S) as ArgShape<A>>::into_value(head) {
            if let (Ok(name), Ok(value)) = (
                HeaderName::from_str(&self.name),
                HeaderValue::from_str(&a.to_header()),
            ) {
                req.add_header(name, value);
            }
        }
        self.next.build_request(tail, req)
    }
    forward_decode!();
}

impl<CTypes, A, S, Next> HasClient for ReqBody<CTypes, A, S, Next>
where
    CTypes: AllMime + AllMimeRender<A>,
    (Required, S): ArgShape<A>,
    Next: HasClient,
    Self: Endpoint<
            Args = HCons<<(Required, S) as ArgShape<A>>::Out, Next::Args>,
            Output = Next::Output,
        >,
{
    fn build_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        let HCons { head, tail } = args;
        if let Some(a) = <(Required, S) as ArgShape<A>>::into_value(head) {
            // The request body is sent in the FIRST (primary) content type;
            // propagate a serialization failure as an error (never panic).
            let (mime, bytes) = CTypes::render_primary(&a)?;
            req.set_body(bytes, mime);
        }
        self.next.build_request(tail, req)
    }
    forward_decode!();
}

// --- The client tree ---

/// A callable client for one endpoint.
pub struct ClientEndpoint<Api> {
    chain: Arc<Api>,
}

impl<Api> ClientEndpoint<Api>
where
    Api: HasClient,
{
    /// Execute this endpoint over `transport` with the given argument list.
    pub async fn call<T: RunClient>(
        &self,
        transport: &T,
        args: Api::Args,
    ) -> Result<Api::Output, ClientError> {
        let mut req = ClientRequest::new();
        self.chain
            .build_request(args, &mut req)
            .map_err(|message| ClientError::EncodeFailure { message })?;
        let resp = transport.run_request(req).await?;
        self.chain.decode(resp)
    }
}

/// Build a typed client value from an API description: a [`ClientEndpoint`] for a
/// single endpoint, or a nested tuple mirroring the [`Alt`] structure.
pub trait MakeClient {
    /// The resulting client value.
    type Client;
    /// Construct it.
    fn make_client(self) -> Self::Client;
}

impl<Api> MakeClient for Api
where
    Api: HasClient,
{
    type Client = ClientEndpoint<Api>;
    fn make_client(self) -> Self::Client {
        ClientEndpoint {
            chain: Arc::new(self),
        }
    }
}

impl<L, R> MakeClient for Alt<L, R>
where
    L: MakeClient,
    R: MakeClient,
{
    type Client = (L::Client, R::Client);
    fn make_client(self) -> Self::Client {
        (self.left.make_client(), self.right.make_client())
    }
}

/// Build a typed client from an API description.
pub fn client<Api: MakeClient>(api: Api) -> Api::Client {
    api.make_client()
}

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
