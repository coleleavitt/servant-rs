//! Server-Sent Events helpers for long-lived event streams.

use std::time::Duration;

use futures_util::StreamExt;
use servant::stream::{ServerEvent, SourceStream};

/// Periodically inject SSE comment events into a [`SourceStream<ServerEvent>`].
///
/// SSE comments are ignored by browsers and most clients, but keep proxies and
/// load balancers from treating an otherwise-idle event stream as dead. The
/// wrapped stream ends as soon as the application stream ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseKeepAlive {
    interval: Duration,
    comment: String,
}

impl SseKeepAlive {
    /// Create a keep-alive injector that emits `: keep-alive` comments every
    /// `interval` while the application stream is idle.
    ///
    /// # Panics
    ///
    /// Panics if `interval` is zero.
    pub fn new(interval: Duration) -> Self {
        assert!(
            !interval.is_zero(),
            "SSE keep-alive interval must be non-zero"
        );
        SseKeepAlive {
            interval,
            comment: "keep-alive".to_string(),
        }
    }

    /// Override the comment text used for keep-alive events.
    pub fn comment(mut self, comment: impl Into<String>) -> Self {
        self.comment = comment.into();
        self
    }

    /// Wrap `stream`, injecting heartbeat comments until `stream` ends.
    pub fn stream(self, stream: SourceStream<ServerEvent>) -> SourceStream<ServerEvent> {
        let events = stream.into_inner();
        let mut ticks = tokio::time::interval(self.interval);
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let comment = self.comment;

        SourceStream::new(futures_util::stream::unfold(
            (events, ticks, comment, false),
            |(mut events, mut ticks, comment, primed)| async move {
                if !primed {
                    ticks.tick().await;
                }

                tokio::select! {
                    event = events.next() => event.map(|event| (event, (events, ticks, comment, true))),
                    _ = ticks.tick() => {
                        let keep_alive = ServerEvent::comment(comment.clone());
                        Some((keep_alive, (events, ticks, comment, true)))
                    }
                }
            },
        ))
    }
}

/// Convenience wrapper for [`SseKeepAlive::new`] followed by [`SseKeepAlive::stream`].
pub fn sse_keep_alive(
    stream: SourceStream<ServerEvent>,
    interval: Duration,
) -> SourceStream<ServerEvent> {
    SseKeepAlive::new(interval).stream(stream)
}
