//! Streaming responses, mirroring `Servant.API.Stream` and
//! `Servant.API.ServerSentEvents`.
//!
//! A streaming endpoint's handler returns a [`SourceStream<T>`] (a boxed async
//! stream of items). The server renders each item with the endpoint's content
//! type and wraps it with a [`Framing`] strategy, sending the result as a
//! chunked response body. Server-Sent Events reuse this machinery via the
//! [`ServerEvent`] item type, [`EventStreamFraming`], and the
//! [`EventStream`](crate::content::EventStream) content type.

use std::pin::Pin;

use bytes::Bytes;
use futures_core::Stream;

mod sse;

pub use sse::{EventStreamFraming, ServerEvent};

/// Default maximum decoded item frame accepted by request-body stream decoding.
pub const DEFAULT_MAX_DECODED_FRAME_BYTES: usize = 8 * 1024 * 1024;

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

/// Error surfaced as an item in a request [`crate::api::StreamBody`] handler
/// stream.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StreamBodyError {
    /// The HTTP body transport failed while the handler was polling it.
    #[error("request stream transport error: {message}")]
    Transport {
        /// Human-readable transport error message.
        message: String,
    },
    /// The frame delimiter or length prefix is malformed.
    #[error("malformed request stream frame: {message}")]
    MalformedFrame {
        /// Human-readable framing error message.
        message: String,
    },
    /// A decoded frame exceeded the configured per-frame cap.
    #[error("request stream frame exceeded {limit} bytes")]
    FrameTooLarge {
        /// Maximum allowed decoded frame size.
        limit: usize,
    },
    /// The frame was well-formed but could not be decoded as the declared item.
    #[error("request stream item decode error: {message}")]
    Decode {
        /// Human-readable item decode error message.
        message: String,
    },
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

    /// Decode one bounded request frame, distinguishing malformed frames from
    /// incomplete ones.
    fn deframe_limited(
        buf: &mut Vec<u8>,
        eof: bool,
        max_frame_bytes: usize,
    ) -> Result<Option<Vec<u8>>, StreamBodyError> {
        if buf.len() > max_frame_bytes {
            return Err(StreamBodyError::FrameTooLarge {
                limit: max_frame_bytes,
            });
        }
        match Self::deframe(buf, eof) {
            Some(frame) if frame.len() > max_frame_bytes => Err(StreamBodyError::FrameTooLarge {
                limit: max_frame_bytes,
            }),
            Some(frame) => Ok(Some(frame)),
            None if eof && !buf.is_empty() => Err(StreamBodyError::MalformedFrame {
                message: "incomplete trailing frame".to_string(),
            }),
            None => Ok(None),
        }
    }
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

    fn deframe_limited(
        buf: &mut Vec<u8>,
        eof: bool,
        max_frame_bytes: usize,
    ) -> Result<Option<Vec<u8>>, StreamBodyError> {
        let Some(colon) = buf.iter().position(|&b| b == b':') else {
            if buf.len() > max_frame_bytes {
                return Err(StreamBodyError::FrameTooLarge {
                    limit: max_frame_bytes,
                });
            }
            return if eof && !buf.is_empty() {
                Err(StreamBodyError::MalformedFrame {
                    message: "netstring length prefix has no colon".to_string(),
                })
            } else {
                Ok(None)
            };
        };
        let len = parse_netstring_len(&buf[..colon])?;
        if len > max_frame_bytes {
            return Err(StreamBodyError::FrameTooLarge {
                limit: max_frame_bytes,
            });
        }
        let data_start = colon
            .checked_add(1)
            .ok_or_else(|| malformed("netstring length overflow"))?;
        let data_end = data_start
            .checked_add(len)
            .ok_or_else(|| malformed("netstring length overflow"))?;
        let total = data_end
            .checked_add(1)
            .ok_or_else(|| malformed("netstring length overflow"))?;
        if buf.len() < total {
            return if eof {
                Err(StreamBodyError::MalformedFrame {
                    message: "incomplete netstring frame".to_string(),
                })
            } else {
                Ok(None)
            };
        }
        if buf.get(data_end).copied() != Some(b',') {
            return Err(StreamBodyError::MalformedFrame {
                message: "netstring frame is missing trailing comma".to_string(),
            });
        }
        let item = buf[data_start..data_end].to_vec();
        buf.drain(..total);
        Ok(Some(item))
    }
}

fn parse_netstring_len(raw: &[u8]) -> Result<usize, StreamBodyError> {
    if raw.is_empty() || !raw.iter().all(u8::is_ascii_digit) {
        return Err(malformed("netstring length prefix is not decimal"));
    }
    std::str::from_utf8(raw)
        .ok()
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| malformed("netstring length prefix is invalid"))
}

fn malformed(message: impl Into<String>) -> StreamBodyError {
    StreamBodyError::MalformedFrame {
        message: message.into(),
    }
}
