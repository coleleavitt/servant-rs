//! End-to-end streaming: the server streams framed items and the typed
//! `call_stream` client de-frames + decodes them back, over real hyper.

use futures_util::StreamExt;
use http::HeaderMap;
use servant::hlist::HNil;
use servant::prelude::*;
use servant_client::{
    BaseUrl,
    ClientError,
    ClientRequest,
    ClientResponse,
    HyperClient,
    RunClient,
    RunStreamingClient,
    StreamingResponse,
    client,
};
use servant_server::adapter::serve_listener;
use servant_server::{RouterService, serve};

macro_rules! nums_api {
    () => {
        path("nums", stream_get::<NewlineFraming, Json, u32>())
    };
}

#[tokio::test]
async fn streaming_client_round_trips_framed_items() {
    let router = serve(nums_api!(), || async {
        Ok::<_, ServerError>(SourceStream::new(futures_util::stream::iter(vec![
            10u32, 20, 30,
        ])))
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(serve_listener(listener, RouterService::new(router)));

    let transport = HyperClient::new(BaseUrl::http("127.0.0.1", addr.port()));
    let endpoint = client(nums_api!());
    let stream = endpoint.call_stream(&transport, HNil).await.unwrap();

    let items: Vec<Result<u32, String>> = stream.into_inner().collect().await;
    let nums: Vec<u32> = items.into_iter().map(|r| r.unwrap()).collect();
    assert_eq!(nums, vec![10, 20, 30]);
}

struct StaticStreamTransport {
    status: http::StatusCode,
    content_type: &'static str,
    chunks: Vec<bytes::Bytes>,
}

impl RunClient for StaticStreamTransport {
    async fn run_request(&self, _req: ClientRequest) -> Result<ClientResponse, ClientError> {
        Err(ClientError::ConnectionError(Box::new(
            std::io::Error::other("buffered calls are not used in streaming tests"),
        )))
    }
}

impl RunStreamingClient for StaticStreamTransport {
    async fn run_streaming(&self, _req: ClientRequest) -> Result<StreamingResponse, ClientError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static(self.content_type),
        );
        let chunks = self.chunks.clone().into_iter().map(Ok::<_, ClientError>);
        Ok(StreamingResponse {
            status: self.status,
            headers,
            body: Box::pin(futures_util::stream::iter(chunks)),
        })
    }
}

#[tokio::test]
async fn streaming_client_reports_incomplete_netstring_frame_at_eof() {
    let transport = StaticStreamTransport {
        status: http::StatusCode::OK,
        content_type: "application/json",
        chunks: vec![bytes::Bytes::from_static(b"3:12,")],
    };
    let endpoint = client(path("nums", stream_get::<NetstringFraming, Json, u32>()));

    let stream = endpoint.call_stream(&transport, HNil).await.unwrap();
    let items: Vec<Result<u32, String>> = stream.into_inner().collect().await;

    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].as_ref().unwrap_err(),
        "incomplete streaming response frame"
    );
}

#[tokio::test]
async fn streaming_client_reports_oversized_unframed_chunk() {
    let transport = StaticStreamTransport {
        status: http::StatusCode::OK,
        content_type: "application/json",
        chunks: vec![bytes::Bytes::from(vec![b'1'; 8 * 1024 * 1024 + 1])],
    };
    let endpoint = client(path("nums", stream_get::<NetstringFraming, Json, u32>()));

    let stream = endpoint.call_stream(&transport, HNil).await.unwrap();
    let items: Vec<Result<u32, String>> = stream.into_inner().collect().await;

    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].as_ref().unwrap_err(),
        "streaming response frame exceeded size limit"
    );
}
