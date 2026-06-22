#[tokio::test]
async fn generated_client_streams_request_body_to_server() {
    // Given: a generated client targeting a real hyper server StreamBody route.
    let (seen_tx, mut seen_rx) = tokio::sync::watch::channel(0usize);
    let service = stream_sum_service(seen_tx);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test listener binds");
    let addr = listener.local_addr().expect("test listener has address");
    tokio::spawn(servant_server::adapter::serve_listener(listener, service));

    let api = path(
        "sum",
        stream_body::<NetstringFraming, Json, u64, _>(verb::<Post, 200, (PlainText,), String>()),
    );
    let endpoint = servant_client::client(api);
    let transport =
        servant_client::HyperClient::new(servant_client::BaseUrl::http("127.0.0.1", addr.port()));
    let (item_tx, item_rx) = tokio::sync::mpsc::channel(2);
    let source = SourceStream::new(futures_util::stream::unfold(item_rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    }));

    // When: the first item is made available but the source stream remains open.
    let task = tokio::spawn(async move {
        endpoint
            .call(&transport, servant::hlist::hlist1(source))
            .await
    });
    item_tx
        .send(Ok(2u64))
        .await
        .expect("first source item sends");
    tokio::time::timeout(std::time::Duration::from_millis(500), seen_rx.changed())
        .await
        .expect("server should receive first item before source completion")
        .expect("watch channel remains open");

    // Then: the request body was streamed through the generated client before
    // the complete source was collected.
    assert_eq!(*seen_rx.borrow(), 1);

    item_tx
        .send(Ok(3u64))
        .await
        .expect("second source item sends");
    drop(item_tx);
    let sum = task
        .await
        .expect("client task completes")
        .expect("generated streaming request succeeds");
    assert_eq!(sum, "5");
}
