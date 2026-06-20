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

You can also terminate TLS with a reverse proxy, platform load balancer, or a
custom listener and then set the same connection metadata before dispatching to
the router. That preserves the Servant-style handler guarantee without forcing a
specific TLS deployment model into the typed API core.

For reverse-proxy deployments, prefer forwarding TLS state through trusted
middleware that sets connection metadata explicitly; do not trust arbitrary
`X-Forwarded-Proto` headers from the public internet without a trusted proxy
boundary.