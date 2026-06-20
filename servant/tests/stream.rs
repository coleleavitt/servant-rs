use bytes::Bytes;
use servant::content::{MimeRender, MimeUnrender};
use servant::stream::{
    EventStreamFraming,
    Framing,
    NetstringFraming,
    NewlineFraming,
    NoFraming,
    ServerEvent,
};

#[test]
fn framings() {
    assert_eq!(NoFraming::frame(b"hi"), Bytes::from_static(b"hi"));
    assert_eq!(NewlineFraming::frame(b"hi"), Bytes::from_static(b"hi\n"));
    assert_eq!(NetstringFraming::frame(b"hi"), Bytes::from_static(b"2:hi,"));
}

#[test]
fn event_stream_framing_extracts_complete_events() {
    let mut buf = b"data: one\n\ndata: two\r\n\r\n".to_vec();
    assert_eq!(
        EventStreamFraming::deframe(&mut buf, false),
        Some(b"data: one".to_vec())
    );
    assert_eq!(
        EventStreamFraming::deframe(&mut buf, false),
        Some(b"data: two".to_vec())
    );
    assert!(buf.is_empty());
}

#[test]
fn sse_event_format() {
    let e = ServerEvent::data("hello")
        .with_event("greeting")
        .with_id("1");
    let bytes = e.mime_render().unwrap();
    assert_eq!(
        std::str::from_utf8(&bytes).unwrap(),
        "event: greeting\nid: 1\ndata: hello\n\n"
    );
}

#[test]
fn sse_comment_format() {
    let e = ServerEvent::comment("keep-alive");
    let bytes = e.mime_render().unwrap();
    assert_eq!(std::str::from_utf8(&bytes).unwrap(), ": keep-alive\n\n");
}

#[test]
fn sse_event_parse() {
    let event = ServerEvent::mime_unrender(
        b": comment\nevent: greeting\nid: 7\ndata: hello\ndata: world\nretry: 100\n",
    )
    .unwrap();

    assert_eq!(event.event.as_deref(), Some("greeting"));
    assert_eq!(event.id.as_deref(), Some("7"));
    assert_eq!(event.comment.as_deref(), Some("comment"));
    assert_eq!(event.data, "hello\nworld");
}
