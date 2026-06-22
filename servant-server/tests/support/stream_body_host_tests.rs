#[tokio::test]
async fn stream_body_host_wrapped_basic_auth_401_before_body_handoff() {
    // Given: Host transparently wraps StreamBody and BasicAuth, and the request
    // has a matching Host header but no credentials.
    let handler_calls = Arc::new(AtomicUsize::new(0));
    let api = host(
        "api.example.com",
        path(
            "sum",
            stream_body::<NetstringFraming, Json, u64, _>(basic_auth::<User, _>(
                "servant",
                verb::<Post, 200, (PlainText,), String>(),
            )),
        ),
    );
    assert_eq!(
        ServerChain::request_body_mode(&api),
        Some(RequestBodyMode::Streaming)
    );
    let context = Context::new().with(BasicAuthCheck::new(|_data: &BasicAuthData| {
        BasicAuthResult::Authorized(User)
    }));
    let service = RouterService::new(serve_with_context(
        api,
        {
            let handler_calls = handler_calls.clone();
            move |_body: SourceStream<Result<u64, StreamBodyError>>, _user: User| {
                handler_calls.fetch_add(1, Ordering::SeqCst);
                async { Ok::<_, ServerError>("unreachable".to_string()) }
            }
        },
        context,
    ));
    let (_body_tx, body, probe) = ChannelBody::new();
    let request = http::Request::builder()
        .method("POST")
        .uri("/sum")
        .header(http::header::HOST, "api.example.com")
        .header("content-type", "application/json")
        .header("accept", "text/plain")
        .body(body)
        .expect("test request builds");

    // When: the matching-host request is served.
    let response = service.handle(request).await;

    // Then: BasicAuth rejects before handler handoff or body polling.
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(handler_calls.load(Ordering::SeqCst), 0);
    assert_eq!(probe.polls(), 0);
}

#[tokio::test]
async fn stream_body_host_wrapped_req_body_still_buffers() {
    // Given: Host transparently wraps an ordinary buffered ReqBody route.
    let api = host(
        "api.example.com",
        path(
            "sum",
            req_body::<(Json,), u64, _>(verb::<Post, 200, (PlainText,), String>()),
        ),
    );
    assert_eq!(
        ServerChain::request_body_mode(&api),
        Some(RequestBodyMode::Buffered)
    );
    let service = RouterService::new(serve(api, |value: u64| async move {
        Ok::<_, ServerError>(value.to_string())
    }));
    let request = http::Request::builder()
        .method("POST")
        .uri("/sum")
        .header(http::header::HOST, "api.example.com")
        .header("content-type", "application/json")
        .header("accept", "text/plain")
        .body(Full::new(Bytes::from_static(b"7")))
        .expect("test request builds");

    // When: the matching-host request is served.
    let (status, body) = collect_text(service.handle(request).await).await;

    // Then: the body is buffered and decoded like an unwrapped ReqBody.
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "7");
}
