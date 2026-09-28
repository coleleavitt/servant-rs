//! The accept loop shared by the hyper serving adapters.
//!
//! A server's accept loop must outlive any one connection's trouble:
//!
//! - a failed `accept` (the process is out of file descriptors, `EMFILE`; a
//!   peer reset its connection before it was accepted, `ECONNABORTED`) is
//!   passing, so it is logged and retried after a short pause instead of ending
//!   the server;
//! - connections are capped ([`ConnectionLimits::max_connections`]): past the
//!   cap a new connection is closed at once rather than given a task, so a
//!   flood of clients cannot exhaust memory or file descriptors.

use std::convert::Infallible;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;

/// Default cap on connections served at once by a serving adapter.
pub const DEFAULT_MAX_CONNECTIONS: usize = 1024;

/// Default pause after a failed `accept` before the next one.
pub const DEFAULT_ACCEPT_ERROR_BACKOFF: Duration = Duration::from_millis(100);

/// Connection-level limits for the serving adapters
/// ([`serve_listener_with_limits`](crate::adapter::serve_listener_with_limits)
/// and, with the `rustls` feature, [`RustlsConfig`](crate::tls::RustlsConfig)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnectionLimits {
    max_connections: usize,
    accept_error_backoff: Duration,
}

impl Default for ConnectionLimits {
    fn default() -> Self {
        ConnectionLimits {
            max_connections: DEFAULT_MAX_CONNECTIONS,
            accept_error_backoff: DEFAULT_ACCEPT_ERROR_BACKOFF,
        }
    }
}

impl ConnectionLimits {
    /// The defaults: [`DEFAULT_MAX_CONNECTIONS`] connections and a
    /// [`DEFAULT_ACCEPT_ERROR_BACKOFF`] pause after a failed accept.
    pub fn new() -> Self {
        Self::default()
    }

    /// Serve at most `max_connections` connections at once; one accepted past
    /// the cap is closed immediately. Clamped to at least one (and at most
    /// Tokio's semaphore limit).
    pub fn with_max_connections(mut self, max_connections: usize) -> Self {
        self.max_connections = max_connections.clamp(1, Semaphore::MAX_PERMITS);
        self
    }

    /// Wait `backoff` after a failed `accept` before accepting again.
    pub fn with_accept_error_backoff(mut self, backoff: Duration) -> Self {
        self.accept_error_backoff = backoff;
        self
    }

    /// The cap on connections served at once.
    pub fn max_connections(&self) -> usize {
        self.max_connections
    }

    /// The pause after a failed `accept`.
    pub fn accept_error_backoff(&self) -> Duration {
        self.accept_error_backoff
    }
}

/// A source of accepted connections: a [`tokio::net::TcpListener`], or a fake
/// in tests.
pub(crate) trait Listener {
    type Io: Send + 'static;

    fn accept(
        &mut self,
    ) -> impl Future<Output = std::io::Result<(Self::Io, Option<SocketAddr>)>> + Send;
}

impl Listener for tokio::net::TcpListener {
    type Io = tokio::net::TcpStream;

    async fn accept(&mut self) -> std::io::Result<(Self::Io, Option<SocketAddr>)> {
        let (stream, peer) = tokio::net::TcpListener::accept(self).await?;
        Ok((stream, Some(peer)))
    }
}

/// Accept connections forever, running `serve(io, peer)` for each on its own
/// task while holding one of `limits.max_connections` permits. Never returns.
pub(crate) async fn accept_loop<L, F, Fut>(
    mut listener: L,
    limits: ConnectionLimits,
    mut serve: F,
) -> Infallible
where
    L: Listener,
    F: FnMut(L::Io, Option<SocketAddr>) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    let permits = Arc::new(Semaphore::new(limits.max_connections));
    loop {
        let (io, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(e) => {
                log::warn!("servant-server: accept failed: {e}; retrying");
                // Out of file descriptors, say: wait for some to close rather than spin.
                tokio::time::sleep(limits.accept_error_backoff).await;
                continue;
            }
        };
        let Ok(permit) = permits.clone().try_acquire_owned() else {
            // Debug, not warn: in a flood this runs once per refused connection.
            log::debug!(
                "servant-server: {} connections open; closing the one from {peer:?}",
                limits.max_connections
            );
            drop(io);
            continue;
        };
        let connection = serve(io, peer);
        tokio::spawn(async move {
            connection.await;
            drop(permit);
        });
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use tokio::io::{AsyncReadExt, DuplexStream};
    use tokio::sync::{mpsc, oneshot};

    use super::*;

    /// A listener fed by the test: each item is the next `accept` result.
    struct FakeListener(mpsc::UnboundedReceiver<io::Result<DuplexStream>>);

    impl Listener for FakeListener {
        type Io = DuplexStream;

        async fn accept(&mut self) -> io::Result<(DuplexStream, Option<SocketAddr>)> {
            match self.0.recv().await {
                Some(next) => next.map(|io| (io, None)),
                // The test is done with the listener: accept nothing more.
                None => std::future::pending().await,
            }
        }
    }

    fn connection() -> (DuplexStream, DuplexStream) {
        tokio::io::duplex(64)
    }

    /// True when the server side of `client` is dropped (closed). Time is
    /// paused, so the guard fires at once if the runtime would otherwise idle.
    async fn closed(client: &mut DuplexStream) -> bool {
        let mut byte = [0u8; 1];
        let read = tokio::time::timeout(Duration::from_secs(3600), client.read(&mut byte));
        matches!(read.await, Ok(Ok(0)))
    }

    #[tokio::test(start_paused = true)]
    async fn accept_error_does_not_end_the_loop() {
        let (feed, accepts) = mpsc::unbounded_channel();
        let (served_tx, mut served) = mpsc::unbounded_channel();
        let server = tokio::spawn(accept_loop(
            FakeListener(accepts),
            ConnectionLimits::default(),
            move |io: DuplexStream, _peer| {
                let served_tx = served_tx.clone();
                async move {
                    served_tx.send(()).unwrap();
                    drop(io);
                }
            },
        ));

        feed.send(Err(io::Error::from_raw_os_error(24))).unwrap(); // EMFILE
        feed.send(Err(io::Error::from(io::ErrorKind::ConnectionAborted)))
            .unwrap();
        let (server_side, _client) = connection();
        feed.send(Ok(server_side)).unwrap();

        let served = tokio::time::timeout(Duration::from_secs(3600), served.recv()).await;
        assert!(
            matches!(served, Ok(Some(()))),
            "the connection after the failed accepts was not served"
        );
        assert!(!server.is_finished(), "the accept loop ended");
        server.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn accept_error_backs_off_before_retrying() {
        let (feed, accepts) = mpsc::unbounded_channel();
        let (served_tx, mut served) = mpsc::unbounded_channel();
        let backoff = Duration::from_millis(100);
        let server = tokio::spawn(accept_loop(
            FakeListener(accepts),
            ConnectionLimits::default().with_accept_error_backoff(backoff),
            move |_io: DuplexStream, _peer| {
                let served_tx = served_tx.clone();
                async move { served_tx.send(tokio::time::Instant::now()).unwrap() }
            },
        ));

        let start = tokio::time::Instant::now();
        feed.send(Err(io::Error::from_raw_os_error(24))).unwrap();
        let (server_side, _client) = connection();
        feed.send(Ok(server_side)).unwrap();

        let served_at = tokio::time::timeout(Duration::from_secs(3600), served.recv())
            .await
            .expect("the connection after the failed accept was not served")
            .unwrap();
        assert!(served_at - start >= backoff, "retried without backing off");
        server.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn connections_over_the_cap_are_closed() {
        let (feed, accepts) = mpsc::unbounded_channel();
        let (held_tx, mut held) = mpsc::unbounded_channel::<oneshot::Sender<()>>();
        let server = tokio::spawn(accept_loop(
            FakeListener(accepts),
            ConnectionLimits::default().with_max_connections(1),
            move |io: DuplexStream, _peer| {
                let held_tx = held_tx.clone();
                async move {
                    // Hold the connection (and its permit) until the test releases it.
                    let (release, released) = oneshot::channel();
                    held_tx.send(release).unwrap();
                    let _ = released.await;
                    drop(io);
                }
            },
        ));

        let (first, mut first_client) = connection();
        feed.send(Ok(first)).unwrap();
        let release_first = held.recv().await.unwrap();

        let (second, mut second_client) = connection();
        feed.send(Ok(second)).unwrap();
        assert!(
            closed(&mut second_client).await,
            "a connection past the cap was not closed"
        );

        // Closing the first connection frees its place.
        release_first.send(()).unwrap();
        assert!(closed(&mut first_client).await);
        let (third, _third_client) = connection();
        feed.send(Ok(third)).unwrap();
        let _release_third = held.recv().await.expect("the freed place is reused");
        server.abort();
    }

    #[test]
    fn limits_are_clamped_to_at_least_one_connection() {
        let limits = ConnectionLimits::new().with_max_connections(0);
        assert_eq!(limits.max_connections(), 1);
        assert_eq!(
            ConnectionLimits::new().max_connections(),
            DEFAULT_MAX_CONNECTIONS
        );
    }
}
