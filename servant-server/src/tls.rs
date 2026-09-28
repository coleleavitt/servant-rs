//! `rustls`-backed serving adapter.
//!
//! TLS stays outside the typed API description, but this adapter integrates a
//! `rustls::ServerConfig` with the existing hyper/tower serving path and marks
//! [`ConnectionInfo::secure`](crate::ConnectionInfo) as `true` for handlers that
//! use the `IsSecure` combinator.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use rustls::ServerConfig;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use crate::adapter::{ConnectionInfo, RouterService, serve_http1};
use crate::listener::{ConnectionLimits, accept_loop};

/// Default deadline for a client to finish the TLS handshake.
pub const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Client-certificate policy associated with a [`RustlsConfig`].
///
/// The actual verifier is carried inside the caller-provided
/// [`rustls::ServerConfig`]. This enum records the intended policy in an
/// explicit, testable form, mirroring the common `Off` / `Optional` /
/// `Required` TLS client-auth split used by production Rust web stacks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TlsClientAuth {
    /// Do not request client certificates.
    #[default]
    Off,
    /// Request client certificates but allow clients that do not present one.
    Optional,
    /// Require a valid client certificate during the TLS handshake.
    Required,
}

/// A `rustls` server configuration plus its declared client-auth policy and
/// the listener's connection limits.
#[derive(Clone)]
pub struct RustlsConfig {
    server_config: Arc<ServerConfig>,
    client_auth: TlsClientAuth,
    handshake_timeout: Duration,
    limits: ConnectionLimits,
}

impl RustlsConfig {
    /// Wrap an already-built [`rustls::ServerConfig`].
    ///
    /// Use the `rustls` builders to load certificates, keys, and any client
    /// certificate verifier, then pass the resulting config here.
    pub fn new(server_config: Arc<ServerConfig>) -> Self {
        RustlsConfig {
            server_config,
            client_auth: TlsClientAuth::Off,
            handshake_timeout: DEFAULT_HANDSHAKE_TIMEOUT,
            limits: ConnectionLimits::default(),
        }
    }

    /// Record the client-certificate policy used by `server_config`.
    pub fn with_client_auth(mut self, client_auth: TlsClientAuth) -> Self {
        self.client_auth = client_auth;
        self
    }

    /// Drop a connection whose TLS handshake has not finished within
    /// `timeout` (default [`DEFAULT_HANDSHAKE_TIMEOUT`]), so a client that
    /// connects and stalls cannot hold a connection slot.
    pub fn with_handshake_timeout(mut self, timeout: Duration) -> Self {
        self.handshake_timeout = timeout;
        self
    }

    /// Set the listener's connection limits (default
    /// [`ConnectionLimits::default`]).
    pub fn with_connection_limits(mut self, limits: ConnectionLimits) -> Self {
        self.limits = limits;
        self
    }

    /// The underlying `rustls` server configuration.
    pub fn server_config(&self) -> &Arc<ServerConfig> {
        &self.server_config
    }

    /// The declared client-certificate policy.
    pub fn client_auth(&self) -> TlsClientAuth {
        self.client_auth
    }

    /// The deadline for a client to finish the TLS handshake.
    pub fn handshake_timeout(&self) -> Duration {
        self.handshake_timeout
    }

    /// The listener's connection limits.
    pub fn connection_limits(&self) -> ConnectionLimits {
        self.limits
    }
}

/// Serve a router over HTTP/1 on top of accepted `rustls` TLS connections.
///
/// Each accepted connection is spawned onto the current Tokio runtime, up to
/// [`RustlsConfig::connection_limits`]; one accepted past the cap is closed at
/// once. A TLS handshake that fails or does not finish within
/// [`RustlsConfig::handshake_timeout`] drops that connection only. A failed
/// `accept` (out of file descriptors, a connection aborted before it was
/// accepted) is logged and retried after a short pause, so the loop runs until
/// its task is dropped and never returns an error; the `Result` is kept for
/// the signature's sake. Requests handled through this adapter receive
/// [`ConnectionInfo`] with `secure = true` and the peer socket address, if
/// available.
pub async fn serve_rustls_listener(
    listener: TcpListener,
    service: RouterService,
    tls: RustlsConfig,
) -> std::io::Result<()> {
    let acceptor = TlsAcceptor::from(tls.server_config.clone());
    let handshake_timeout = tls.handshake_timeout;
    match accept_loop(listener, tls.limits, move |stream, peer| {
        serve_tls_connection(
            acceptor.clone(),
            handshake_timeout,
            stream,
            peer,
            service.clone(),
        )
    })
    .await {}
}

/// Run the TLS handshake on `io` within `handshake_timeout`, then serve HTTP/1
/// on the encrypted stream.
async fn serve_tls_connection<I>(
    acceptor: TlsAcceptor,
    handshake_timeout: Duration,
    io: I,
    peer: Option<SocketAddr>,
    service: RouterService,
) where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let stream = match tokio::time::timeout(handshake_timeout, acceptor.accept(io)).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(e)) => {
            log::debug!("servant-server: TLS handshake with {peer:?} failed: {e}");
            return;
        }
        Err(_) => {
            log::debug!("servant-server: TLS handshake with {peer:?} timed out");
            return;
        }
    };
    serve_http1(
        stream,
        service,
        ConnectionInfo {
            remote_addr: peer,
            secure: true,
        },
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_auth_policy_is_explicit() {
        assert_eq!(TlsClientAuth::default(), TlsClientAuth::Off);
        assert_ne!(TlsClientAuth::Optional, TlsClientAuth::Required);
    }

    fn server_config() -> Arc<ServerConfig> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let key = rustls::pki_types::PrivateKeyDer::Pkcs8(
            rustls::pki_types::PrivatePkcs8KeyDer::from(certified.key_pair.serialize_der()),
        );
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![certified.cert.der().clone()], key)
            .unwrap();
        Arc::new(config)
    }

    #[tokio::test(start_paused = true)]
    async fn stalled_handshake_is_dropped_at_the_deadline() {
        use servant::prelude::*;
        use tokio::io::AsyncReadExt;

        let router = crate::serve(get::<(PlainText,), String>(), || async {
            Ok::<_, ServerError>(String::new())
        });
        let (server_side, mut client) = tokio::io::duplex(1024);
        let deadline = Duration::from_secs(10);
        let start = tokio::time::Instant::now();
        // The client connects and never sends a ClientHello.
        let server = tokio::spawn(serve_tls_connection(
            TlsAcceptor::from(server_config()),
            deadline,
            server_side,
            None,
            RouterService::new(router),
        ));

        let mut byte = [0u8; 1];
        let read = tokio::time::timeout(deadline * 6, client.read(&mut byte)).await;
        assert!(matches!(read, Ok(Ok(0))), "connection not closed: {read:?}");
        assert_eq!(start.elapsed(), deadline);
        server.await.unwrap();
    }

    #[test]
    fn listener_limits_are_configurable() {
        let config = RustlsConfig::new(server_config());
        assert_eq!(config.handshake_timeout(), DEFAULT_HANDSHAKE_TIMEOUT);
        assert_eq!(config.connection_limits(), ConnectionLimits::default());
        let limits = ConnectionLimits::default().with_max_connections(8);
        let config = config
            .with_handshake_timeout(Duration::from_secs(3))
            .with_connection_limits(limits);
        assert_eq!(config.handshake_timeout(), Duration::from_secs(3));
        assert_eq!(config.connection_limits().max_connections(), 8);
    }
}
