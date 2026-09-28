# TLS termination and connection metadata

`servant-rs` keeps TLS termination outside the typed API/router core. This is
intentional: the router works over `http::Request` snapshots, while TLS is a
transport concern owned by the adapter that accepts sockets.

The server-side `IsSecure`, `HttpVersion`, and `RemoteHost` combinators read the
connection metadata stored in `RequestData`:

- `IsSecure` passes a `bool` to the handler.
- `HttpVersion` passes the request `http::Version`.
- `RemoteHost` passes `Option<SocketAddr>`.

The built-in plain-HTTP hyper adapter marks `is_secure = false`. With the
`rustls` feature enabled, `servant-server` also exposes a small Rustls adapter:

```rust,no_run
use std::sync::Arc;
use servant_server::{RustlsConfig, RouterService, TlsClientAuth, serve_rustls_listener};

# async fn example(
#     listener: tokio::net::TcpListener,
#     router: servant_server::Router,
#     server_config: rustls::ServerConfig,
# ) -> std::io::Result<()> {
let service = RouterService::new(router);
let tls = RustlsConfig::new(Arc::new(server_config))
    .with_client_auth(TlsClientAuth::Off);
serve_rustls_listener(listener, service, tls).await
# }
```

The adapter terminates TLS with `rustls`, serves HTTP/1 via hyper, and sets the
same connection metadata before dispatching to the router. Its client-auth
policy is explicit (`Off`, `Optional`, or `Required`); the actual certificate
verifier lives in the caller-provided `rustls::ServerConfig`.

The listener survives transient trouble and bounds what a client can hold:

- A failed `accept` (the process is out of file descriptors, a connection was
  aborted before it was accepted) is logged through the `log` facade and
  retried after a short pause (100 ms by default); it never ends the server.
- At most `ConnectionLimits::max_connections` connections (1024 by default)
  are served at once; a connection accepted past the cap is closed at once.
- A client must finish the TLS handshake within
  `RustlsConfig::handshake_timeout` (10 s by default) and send each request's
  headers within hyper's header-read timeout (30 s), or it is dropped.

```rust,no_run
# use std::{sync::Arc, time::Duration};
# use servant_server::{ConnectionLimits, RustlsConfig};
# fn example(server_config: rustls::ServerConfig) -> RustlsConfig {
RustlsConfig::new(Arc::new(server_config))
    .with_handshake_timeout(Duration::from_secs(5))
    .with_connection_limits(ConnectionLimits::new().with_max_connections(256))
# }
```

The plain-HTTP `adapter::serve_listener` shares the same accept loop;
`adapter::serve_listener_with_limits` takes explicit `ConnectionLimits`.

You can also terminate TLS with a reverse proxy, platform load balancer, or a
custom listener and then set the same connection metadata before dispatching to
the router. That preserves the Servant-style handler guarantee without forcing a
specific TLS deployment model into the typed API core.

For reverse-proxy deployments, prefer forwarding TLS state through trusted
middleware that sets connection metadata explicitly; do not trust arbitrary
`X-Forwarded-Proto` headers from the public internet without a trusted proxy
boundary.