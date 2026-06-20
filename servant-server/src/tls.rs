//! `rustls`-backed serving adapter.
//!
//! TLS stays outside the typed API description, but this adapter integrates a
//! `rustls::ServerConfig` with the existing hyper/tower serving path and marks
//! [`ConnectionInfo::secure`](crate::ConnectionInfo) as `true` for handlers that
//! use the `IsSecure` combinator.

use std::sync::Arc;

use hyper_util::rt::TokioIo;
use rustls::ServerConfig;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use crate::adapter::{ConnectionInfo, RouterService};

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

/// A `rustls` server configuration plus its declared client-auth policy.
#[derive(Clone)]
pub struct RustlsConfig {
    server_config: Arc<ServerConfig>,
    client_auth: TlsClientAuth,
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
        }
    }

    /// Record the client-certificate policy used by `server_config`.
    pub fn with_client_auth(mut self, client_auth: TlsClientAuth) -> Self {
        self.client_auth = client_auth;
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
}

/// Serve a router over HTTP/1 on top of accepted `rustls` TLS connections.
///
/// Each accepted connection is spawned onto the current Tokio runtime. TLS
/// handshake failures are ignored for that connection and the accept loop keeps
/// running. Requests handled through this adapter receive [`ConnectionInfo`] with
/// `secure = true` and the peer socket address, if available.
pub async fn serve_rustls_listener(
    listener: TcpListener,
    service: RouterService,
    tls: RustlsConfig,
) -> std::io::Result<()> {
    let acceptor = TlsAcceptor::from(tls.server_config.clone());

    loop {
        let (stream, _) = listener.accept().await?;
        let peer = stream.peer_addr().ok();
        let acceptor = acceptor.clone();
        let service = service.clone();

        tokio::spawn(async move {
            let Ok(stream) = acceptor.accept(stream).await else {
                return;
            };
            let io = TokioIo::new(stream);
            let hyper_svc =
                hyper::service::service_fn(move |mut req: http::Request<hyper::body::Incoming>| {
                    req.extensions_mut().insert(ConnectionInfo {
                        remote_addr: peer,
                        secure: true,
                    });
                    let service = service.clone();
                    async move { Ok::<_, std::convert::Infallible>(service.handle(req).await) }
                });
            let _ = hyper::server::conn::http1::Builder::new()
                .serve_connection(io, hyper_svc)
                .await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_auth_policy_is_explicit() {
        assert_eq!(TlsClientAuth::default(), TlsClientAuth::Off);
        assert_ne!(TlsClientAuth::Optional, TlsClientAuth::Required);
    }
}
