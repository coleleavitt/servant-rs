use bytes::Bytes;

use super::Framing;
use crate::content::{EventStream, MimeRender, MimeUnrender};

/// Server-Sent Event framing: each item is an SSE event block terminated by a
/// blank line. This is the streaming-client counterpart of Servant's
/// `Servant.API.ServerSentEvents` line parser.
#[derive(Debug, Clone, Copy, Default)]
pub struct EventStreamFraming;

impl Framing for EventStreamFraming {
    fn frame(rendered: &[u8]) -> Bytes {
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

/// Render a [`ServerEvent`] in the `text/event-stream` wire format.
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
        out.push('\n');
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
