//! Streaming responses, mirroring `Servant.API.Stream` and
//! `Servant.API.ServerSentEvents`.
//!
//! A streaming endpoint's handler returns a [`SourceStream<T>`] (a boxed async
//! stream of items). The server renders each item with the endpoint's content
//! type and wraps it with a [`Framing`] strategy, sending the result as a
//! chunked response body. Server-Sent Events reuse this machinery via the
//! [`ServerEvent`] item type and the [`EventStream`](crate::content::EventStream)
//! content type.

use std::pin::Pin;

use bytes::Bytes;
use futures_core::Stream;

use crate::content::{EventStream, MimeRender};

/// A boxed async stream of response items.
pub struct SourceStream<T> {
    inner: Pin<Box<dyn Stream<Item = T> + Send>>,
}

impl<T> SourceStream<T> {
    /// Build from any `Send` stream of `T`.
    pub fn new(stream: impl Stream<Item = T> + Send + 'static) -> Self {
        SourceStream {
            inner: Box::pin(stream),
        }
    }

    /// Consume into the underlying pinned stream.
    pub fn into_inner(self) -> Pin<Box<dyn Stream<Item = T> + Send>> {
        self.inner
    }
}

/// How each rendered stream item is delimited on the wire.
pub trait Framing {
    /// Frame one already-rendered item (encode direction).
    fn frame(rendered: &[u8]) -> Bytes;

    /// Extract one complete de-framed item from the front of `buf` (decode
    /// direction, for streaming clients), removing its bytes. `eof` signals the
    /// stream has ended so any trailing partial frame can be flushed. Returns
    /// `None` when no complete item is available yet.
    fn deframe(buf: &mut Vec<u8>, eof: bool) -> Option<Vec<u8>>;
}

/// No delimiter — items are concatenated as-is.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoFraming;
/// Each item followed by a newline (`\n`).
#[derive(Debug, Clone, Copy, Default)]
pub struct NewlineFraming;
/// Netstring framing: `<len>:<item>,` (Servant's `NetstringFraming`).
#[derive(Debug, Clone, Copy, Default)]
pub struct NetstringFraming;

impl Framing for NoFraming {
    fn frame(rendered: &[u8]) -> Bytes {
        Bytes::copy_from_slice(rendered)
    }
    fn deframe(buf: &mut Vec<u8>, eof: bool) -> Option<Vec<u8>> {
        // No delimiter: item boundaries are unrecoverable. At EOF, surface the
        // whole accumulated body as a single item.
        if eof && !buf.is_empty() {
            Some(std::mem::take(buf))
        } else {
            None
        }
    }
}
impl Framing for NewlineFraming {
    fn frame(rendered: &[u8]) -> Bytes {
        let mut out = Vec::with_capacity(rendered.len() + 1);
        out.extend_from_slice(rendered);
        out.push(b'\n');
        Bytes::from(out)
    }
    fn deframe(buf: &mut Vec<u8>, eof: bool) -> Option<Vec<u8>> {
        if let Some(i) = buf.iter().position(|&b| b == b'\n') {
            let item: Vec<u8> = buf.drain(..=i).take(i).collect(); // exclude the '\n'
            Some(item)
        } else if eof && !buf.is_empty() {
            Some(std::mem::take(buf)) // trailing unterminated item
        } else {
            None
        }
    }
}
impl Framing for NetstringFraming {
    fn frame(rendered: &[u8]) -> Bytes {
        let mut out = format!("{}:", rendered.len()).into_bytes();
        out.extend_from_slice(rendered);
        out.push(b',');
        Bytes::from(out)
    }
    fn deframe(buf: &mut Vec<u8>, _eof: bool) -> Option<Vec<u8>> {
        // `<len>:<data>,`
        let colon = buf.iter().position(|&b| b == b':')?;
        let len: usize = std::str::from_utf8(&buf[..colon]).ok()?.parse().ok()?;
        let total = colon + 1 + len + 1; // digits + ':' + data + ','
        if buf.len() < total {
            return None;
        }
        let item = buf[colon + 1..colon + 1 + len].to_vec();
        buf.drain(..total);
        Some(item)
    }
}

/// A single Server-Sent Event.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerEvent {
    /// The `event:` type (optional).
    pub event: Option<String>,
    /// The `id:` field (optional).
    pub id: Option<String>,
    /// The `data:` payload (may be multi-line).
    pub data: String,
}

impl ServerEvent {
    /// An event carrying only data.
    pub fn data(data: impl Into<String>) -> Self {
        ServerEvent {
            event: None,
            id: None,
            data: data.into(),
        }
    }
    /// Set the event type.
    pub fn with_event(mut self, event: impl Into<String>) -> Self {
        self.event = Some(event.into());
        self
    }
    /// Set the id.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }
}

/// Render a [`ServerEvent`] in the `text/event-stream` wire format. Combined
/// with [`NoFraming`] (each event already ends in a blank line) this produces a
/// valid SSE stream.
impl MimeRender<EventStream> for ServerEvent {
    fn mime_render(&self) -> Result<Bytes, String> {
        let mut out = String::new();
        if let Some(e) = &self.event {
            out.push_str("event: ");
            out.push_str(e);
            out.push('\n');
        }
        if let Some(i) = &self.id {
            out.push_str("id: ");
            out.push_str(i);
            out.push('\n');
        }
        for line in self.data.split('\n') {
            out.push_str("data: ");
            out.push_str(line);
            out.push('\n');
        }
        out.push('\n'); // blank line terminates the event
        Ok(Bytes::from(out.into_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framings() {
        assert_eq!(NoFraming::frame(b"hi"), Bytes::from_static(b"hi"));
        assert_eq!(NewlineFraming::frame(b"hi"), Bytes::from_static(b"hi\n"));
        assert_eq!(NetstringFraming::frame(b"hi"), Bytes::from_static(b"2:hi,"));
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
}
