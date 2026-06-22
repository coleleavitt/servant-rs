use bytes::Bytes;
use futures_util::StreamExt;
use servant::hlist::hlist1;
use servant::prelude::*;
use servant::stream::StreamBodyError;
use servant_client::{ClientError, ClientRequest, ClientResponse, RunClient, client};

struct BufferedOnlyTransport;

impl RunClient for BufferedOnlyTransport {
    async fn run_request(&self, _req: ClientRequest) -> Result<ClientResponse, ClientError> {
        Ok(ClientResponse {
            status: http::StatusCode::OK,
            headers: http::HeaderMap::new(),
            body: Bytes::new(),
        })
    }
}

struct DrainStreamingTransport {
    expected_body: Bytes,
}

impl RunClient for DrainStreamingTransport {
    fn supports_streaming_request_body(&self) -> bool {
        true
    }

    async fn run_request(&self, mut req: ClientRequest) -> Result<ClientResponse, ClientError> {
        let body = req
            .streaming_body
            .take()
            .expect("streaming request body should be present");
        assert_eq!(body.media_type(), &mime::APPLICATION_JSON);
        let mut stream = body.take_stream()?;
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            bytes.extend_from_slice(&chunk?);
        }
        assert_eq!(Bytes::from(bytes), self.expected_body);
        Ok(ClientResponse {
            status: http::StatusCode::OK,
            headers: text_plain_headers(),
            body: Bytes::from_static(b"drained"),
        })
    }
}

fn text_plain_headers() -> http::HeaderMap {
    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    headers
}

#[tokio::test]
async fn streaming_request_transport_required_error() {
    // Given: a generated client for a request StreamBody endpoint and a
    // transport that only supports buffered request bodies.
    let endpoint = client(path(
        "sum",
        stream_body::<NetstringFraming, Json, u64, _>(verb::<Post, 200, (PlainText,), String>()),
    ));
    let source = SourceStream::new(futures_util::stream::iter(vec![Ok(1u64)]));

    // When: the endpoint is called through the non-streaming transport.
    let error = endpoint
        .call(&BufferedOnlyTransport, hlist1(source))
        .await
        .expect_err("streaming request body should require transport support");

    // Then: the unsupported path is explicit and not a silent empty request.
    assert!(error.to_string().contains("streaming request"));
}

#[tokio::test]
async fn streaming_request_sets_primary_content_type_and_frames_items() {
    // Given: a generated StreamBody client and a streaming-capable transport
    // that drains the produced request bytes.
    let endpoint = client(path(
        "sum",
        stream_body::<NetstringFraming, Json, u64, _>(verb::<Post, 200, (PlainText,), String>()),
    ));
    let source = SourceStream::new(futures_util::stream::iter(vec![Ok(7u64), Ok(11u64)]));

    // When: the endpoint is called through the draining transport.
    let response = endpoint
        .call(
            &DrainStreamingTransport {
                expected_body: Bytes::from_static(b"1:7,2:11,"),
            },
            hlist1(source),
        )
        .await
        .expect("streaming request drains");

    // Then: the response proves the transport consumed the generated stream.
    assert_eq!(response, "drained");
}

#[tokio::test]
async fn streaming_request_source_item_error_surfaces_encode_failure() {
    // Given: a generated StreamBody client whose source produces an item error.
    let endpoint = client(path(
        "sum",
        stream_body::<NetstringFraming, Json, u64, _>(verb::<Post, 200, (PlainText,), String>()),
    ));
    let source = SourceStream::new(futures_util::stream::iter(vec![Err(
        StreamBodyError::Decode {
            message: "bad item".to_string(),
        },
    )]));

    // When: a streaming-capable transport polls the generated request body.
    let error = endpoint
        .call(
            &DrainStreamingTransport {
                expected_body: Bytes::new(),
            },
            hlist1(source),
        )
        .await
        .expect_err("source item error should fail request encoding");

    // Then: the failure is surfaced as an encode error.
    assert!(matches!(error, ClientError::EncodeFailure { .. }));
}
