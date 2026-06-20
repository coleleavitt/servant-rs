//! Streaming responses, mirroring `Servant.API.Stream` and
//! `Servant.API.ServerSentEvents`.
//!
//! A streaming endpoint's handler returns a [`SourceStream<T>`] (a boxed async
//! stream of items). The server renders each item with the endpoint's content
//! type and wraps it with a [`Framing`] strategy, sending the result as a
//! chunked response body. Server-Sent Events reuse this machinery via the
//! [`ServerEvent`] item type, [`EventStreamFraming`], and the [`EventStream`]
//! content type.

use std::pin::Pin;

use bytes::Bytes;
use futures_core::Stream;

use crate::content::{EventStream, MimeRender, MimeUnrender};

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
/// Server-Sent Event framing: each item is an SSE event block terminated by a
/// blank line. This is the streaming-client counterpart of Servant's
/// `Servant.API.ServerSentEvents` line parser.
#[derive(Debug, Clone, Copy, Default)]
pub struct EventStreamFraming;

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
        let data_start = colon.checked_add(1)?;
        let data_end = data_start.checked_add(len)?;
        let total = data_end.checked_add(1)?;
        if buf.len() < total {
            return None;
        }
        if buf.get(data_end).copied()? != b',' {
            return None;
        }
        let item = buf[data_start..data_end].to_vec();
        buf.drain(..total);
        Some(item)
    }
}

impl Framing for EventStreamFraming {
    fn frame(rendered: &[u8]) -> Bytes {
        // `ServerEvent` rendering already terminates the event with a blank
        // line. Keep the framing transparent on encode so custom SSE item types
        // can fully control their wire representation.
        Bytes::copy_from_slice(rendered)
    }

    fn deframe(buf: &mut Vec<u8>, eof: bool) -> Option<Vec<u8>> {
        if let Some((item_end, drain_end)) = find_event_end(buf) {
            let item = buf[..item_end].to_vec();
            buf.drain(..drain_end);
            return Some(item);
        }
        if eof && !buf.is_empty() {
            Some(std::mem::take(buf))
        } else {
            None
        }
    }
}

fn find_event_end(buf: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    while i < buf.len() {
        match buf[i] {
            b'\n' if i + 1 < buf.len() && buf[i + 1] == b'\n' => return Some((i, i + 2)),
            b'\n' if i + 2 < buf.len() && buf[i + 1] == b'\r' && buf[i + 2] == b'\n' => {
                return Some((i, i + 3));
            }
            b'\r' if i + 1 < buf.len() && buf[i + 1] == b'\r' => return Some((i, i + 2)),
            b'\r'
                if i + 3 < buf.len()
                    && buf[i + 1] == b'\n'
                    && buf[i + 2] == b'\r'
                    && buf[i + 3] == b'\n' =>
            {
                return Some((i, i + 4));
            }
            _ => i += 1,
        }
    }
    None
}

/// A single Server-Sent Event.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerEvent {
    /// Comment lines (`: ...`), commonly used as SSE keep-alive heartbeats.
    pub comment: Option<String>,
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
            comment: None,
            event: None,
            id: None,
            data: data.into(),
        }
    }
    /// An SSE comment block, typically used as a keep-alive heartbeat.
    pub fn comment(comment: impl Into<String>) -> Self {
        ServerEvent {
            comment: Some(comment.into()),
            event: None,
            id: None,
            data: String::new(),
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
/// with [`EventStreamFraming`] this produces an incrementally parseable SSE
/// stream.
impl MimeRender<EventStream> for ServerEvent {
    fn mime_render(&self) -> Result<Bytes, String> {
        let mut out = String::new();
        if let Some(comment) = &self.comment {
            for line in comment.split('\n') {
                out.push_str(": ");
                out.push_str(line);
                out.push('\n');
            }
        }
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
        if !self.data.is_empty()
            || (self.comment.is_none() && self.event.is_none() && self.id.is_none())
        {
            for line in self.data.split('\n') {
                out.push_str("data: ");
                out.push_str(line);
                out.push('\n');
            }
        }
        out.push('\n'); // blank line terminates the event
        Ok(Bytes::from(out.into_bytes()))
    }
}

impl MimeUnrender<EventStream> for ServerEvent {
    fn mime_unrender(bytes: &[u8]) -> Result<Self, String> {
        let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");

        let mut event = None;
        let mut id = None;
        let mut comments = Vec::new();
        let mut data = Vec::new();

        for line in normalized.lines() {
            if line.is_empty() {
                continue;
            }
            if let Some(comment) = line.strip_prefix(':') {
                comments.push(comment.strip_prefix(' ').unwrap_or(comment).to_string());
                continue;
            }
            let (field, value) = match line.split_once(':') {
                Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
                None => (line, ""),
            };
            match field {
                "event" => event = Some(value.to_string()),
                "id" => id = Some(value.to_string()),
                "data" => data.push(value.to_string()),
                "retry" => {}
                _ => {}
            }
        }

        Ok(ServerEvent {
            comment: if comments.is_empty() {
                None
            } else {
                Some(comments.join("\n"))
            },
            event,
            id,
            data: data.join("\n"),
        })
    }
}
