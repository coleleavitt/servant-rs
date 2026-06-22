use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_util::StreamExt;
use http::StatusCode;
use http_body::{Body, Frame, SizeHint};
use http_body_util::BodyExt;
use servant::prelude::*;
use servant::stream::StreamBodyError;
use servant_server::{RouterService, serve};
use tokio::sync::{mpsc, watch};

pub struct ChannelBody {
    rx: mpsc::Receiver<Result<Bytes, &'static str>>,
    poll_count: Arc<AtomicUsize>,
    drop_count: Arc<AtomicUsize>,
}

impl ChannelBody {
    pub fn new() -> (mpsc::Sender<Result<Bytes, &'static str>>, Self, BodyProbe) {
        let (tx, rx) = mpsc::channel(8);
        let probe = BodyProbe::default();
        (
            tx,
            ChannelBody {
                rx,
                poll_count: probe.poll_count.clone(),
                drop_count: probe.drop_count.clone(),
            },
            probe,
        )
    }
}

impl Body for ChannelBody {
    type Data = Bytes;
    type Error = std::io::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        self.poll_count.fetch_add(1, Ordering::SeqCst);
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(Ok(bytes))) => Poll::Ready(Some(Ok(Frame::data(bytes)))),
            Poll::Ready(Some(Err(message))) => {
                Poll::Ready(Some(Err(std::io::Error::other(message))))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::default()
    }
}

impl Drop for ChannelBody {
    fn drop(&mut self) {
        self.drop_count.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Clone, Default)]
pub struct BodyProbe {
    poll_count: Arc<AtomicUsize>,
    drop_count: Arc<AtomicUsize>,
}

impl BodyProbe {
    pub fn polls(&self) -> usize {
        self.poll_count.load(Ordering::SeqCst)
    }

    pub fn drops(&self) -> usize {
        self.drop_count.load(Ordering::SeqCst)
    }
}

pub async fn collect_text(
    response: http::Response<servant_server::response::ResponseBody>,
) -> (StatusCode, String) {
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("test response body collects")
        .to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

pub fn stream_sum_service(first_item_seen: watch::Sender<usize>) -> RouterService {
    let api = path(
        "sum",
        stream_body::<NetstringFraming, Json, u64, _>(verb::<Post, 200, (PlainText,), String>()),
    );
    RouterService::new(serve(
        api,
        move |body: SourceStream<Result<u64, StreamBodyError>>| {
            let first_item_seen = first_item_seen.clone();
            async move {
                let mut stream = body.into_inner();
                let mut sum = 0u64;
                while let Some(item) = stream.next().await {
                    let value =
                        item.map_err(|error| ServerError::err400().with_body(error.to_string()))?;
                    if sum == 0 {
                        let _ = first_item_seen.send(1);
                    }
                    sum += value;
                }
                Ok::<_, ServerError>(sum.to_string())
            }
        },
    ))
}
