//! End-to-end streaming: the server streams framed items and the typed
//! `call_stream` client de-frames + decodes them back, over real hyper.

use futures_util::StreamExt;
use servant::hlist::HNil;
use servant::prelude::*;
use servant_client::{BaseUrl, HyperClient, client};
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
