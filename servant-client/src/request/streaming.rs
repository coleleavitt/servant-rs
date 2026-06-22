use std::pin::Pin;
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use futures_core::Stream;
use mime::Mime;

use super::ClientError;

/// Encoded byte chunks for a one-shot streaming request body.
pub type RequestByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, ClientError>> + Send>>;

/// A one-shot streaming request body and its declared content type.
#[derive(Clone)]
pub struct StreamingRequestBody {
    media_type: Mime,
    stream: Arc<Mutex<Option<RequestByteStream>>>,
}

impl StreamingRequestBody {
    /// Build a streaming request body from already encoded/framed byte chunks.
    pub fn new(media_type: Mime, stream: RequestByteStream) -> Self {
        StreamingRequestBody {
            media_type,
            stream: Arc::new(Mutex::new(Some(stream))),
        }
    }

    /// The request `Content-Type`.
    pub fn media_type(&self) -> &Mime {
        &self.media_type
    }

    /// Take the one-shot stream so a streaming-capable transport can send it.
    pub fn take_stream(&self) -> Result<RequestByteStream, ClientError> {
        let mut guard = self.stream.lock().map_err(|_| {
            ClientError::ConnectionError("streaming request body state was poisoned".into())
        })?;
        guard.take().ok_or_else(|| {
            ClientError::ConnectionError("streaming request body was already consumed".into())
        })
    }
}
