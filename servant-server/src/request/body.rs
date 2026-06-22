use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll};

use bytes::{Buf, Bytes};
use futures_core::Stream;
use futures_util::StreamExt;
use http_body::Body;
use http_body_util::BodyExt;
use http_body_util::combinators::UnsyncBoxBody;
use servant::error::ServerError;
use servant::stream::StreamBodyError;

use crate::response::BoxError;

type BoxRequestBody = UnsyncBoxBody<Bytes, BoxError>;

#[derive(Clone)]
pub struct RequestBody {
    state: Arc<Mutex<BodySlot>>,
    max_buffered_bytes: usize,
}

enum BodySlot {
    Streaming(Option<BoxRequestBody>),
    Buffered(Bytes),
    Taken,
}

impl RequestBody {
    pub fn new<B>(body: B, max_buffered_bytes: usize) -> Self
    where
        B: Body + Send + 'static,
        B::Data: Buf + Send + 'static,
        B::Error: Into<BoxError>,
    {
        let boxed = body
            .map_frame(|frame| frame.map_data(buf_to_bytes))
            .map_err(Into::into)
            .boxed_unsync();
        RequestBody {
            state: Arc::new(Mutex::new(BodySlot::Streaming(Some(boxed)))),
            max_buffered_bytes,
        }
    }

    pub async fn buffer(&self) -> Result<Bytes, ServerError> {
        let Some(body) = self.take_for_buffering()? else {
            return self.buffered();
        };
        let bytes = collect_limited(body, self.max_buffered_bytes).await?;
        *self.lock()? = BodySlot::Buffered(bytes.clone());
        Ok(bytes)
    }

    pub fn buffered(&self) -> Result<Bytes, ServerError> {
        match &*self.lock()? {
            BodySlot::Buffered(bytes) => Ok(bytes.clone()),
            BodySlot::Streaming(_) | BodySlot::Taken => {
                Err(ServerError::err500().with_body("request body is not buffered"))
            }
        }
    }

    pub fn take_stream(&self) -> Result<RequestBodyStream, ServerError> {
        let mut guard = self.lock()?;
        let slot = std::mem::replace(&mut *guard, BodySlot::Taken);
        match slot {
            BodySlot::Streaming(Some(body)) => Ok(RequestBodyStream { body: Some(body) }),
            BodySlot::Streaming(None) | BodySlot::Taken => {
                Err(ServerError::err500().with_body("request body was already consumed"))
            }
            BodySlot::Buffered(bytes) => {
                *guard = BodySlot::Buffered(bytes);
                Err(ServerError::err500().with_body("request body was already buffered"))
            }
        }
    }

    pub fn state_label(&self) -> &'static str {
        match self.lock() {
            Ok(guard) => match &*guard {
                BodySlot::Streaming(Some(_)) => "streaming",
                BodySlot::Streaming(None) | BodySlot::Taken => "taken",
                BodySlot::Buffered(_) => "buffered",
            },
            Err(_) => "poisoned",
        }
    }

    fn take_for_buffering(&self) -> Result<Option<BoxRequestBody>, ServerError> {
        let mut guard = self.lock()?;
        let slot = std::mem::replace(&mut *guard, BodySlot::Taken);
        match slot {
            BodySlot::Streaming(body) => Ok(body),
            BodySlot::Buffered(bytes) => {
                *guard = BodySlot::Buffered(bytes);
                Ok(None)
            }
            BodySlot::Taken => Err(ServerError::err500().with_body("request body was consumed")),
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, BodySlot>, ServerError> {
        self.state
            .lock()
            .map_err(|_| ServerError::err500().with_body("request body lock poisoned"))
    }
}

pub struct RequestBodyStream {
    body: Option<BoxRequestBody>,
}

impl Stream for RequestBodyStream {
    type Item = Result<Bytes, StreamBodyError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            let Some(body) = self.body.as_mut() else {
                return Poll::Ready(None);
            };
            match Pin::new(body).poll_frame(cx) {
                Poll::Ready(Some(Ok(frame))) => match frame.into_data() {
                    Ok(bytes) => return Poll::Ready(Some(Ok(bytes))),
                    Err(_) => continue,
                },
                Poll::Ready(Some(Err(error))) => {
                    self.body = None;
                    return Poll::Ready(Some(Err(StreamBodyError::Transport {
                        message: error.to_string(),
                    })));
                }
                Poll::Ready(None) => {
                    self.body = None;
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

fn buf_to_bytes(mut data: impl Buf) -> Bytes {
    data.copy_to_bytes(data.remaining())
}

async fn collect_limited(body: BoxRequestBody, max_bytes: usize) -> Result<Bytes, ServerError> {
    let mut stream = RequestBodyStream { body: Some(body) };
    let mut out = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(_) => return Err(ServerError::err400().with_body("could not read request body")),
        };
        let next_len = out
            .len()
            .checked_add(chunk.len())
            .ok_or_else(ServerError::err413)?;
        if next_len > max_bytes {
            return Err(ServerError::err413());
        }
        out.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(out))
}
