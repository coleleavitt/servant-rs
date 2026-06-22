//! `servant-server` — runtime routing, extraction, and adapters for servant-rs.
//!
//! Given a [`servant`] API description and matching handler(s), [`serve()`] builds
//! an inspectable [`Router`] whose semantics mirror Haskell Servant:
//!
//! - the routing tree and left-biased [`router::choice`] selection
//!   ([`router`]),
//! - the phase-ordered extraction pipeline with `Fail`/`FailFatal` and the
//!   best-error priority table ([`extract`], [`result`]),
//! - a [`tower_service::Service`] edge adapter over hyper ([`adapter`]).
//!
//! ```no_run
//! use servant::prelude::*;
//! use servant_server::adapter::RouterService;
//! use servant_server::serve;
//!
//! # async fn ex() {
//! // "hello" :> Get '[PlainText] String
//! let api = path("hello", get::<(PlainText,), String>());
//! let router = serve(api, || async { Ok::<_, ServerError>("hi".to_string()) });
//! let _service = RouterService::new(router);
//! # }
//! ```
#![forbid(unsafe_code)]

pub mod adapter;
pub mod context;
pub mod extract;
pub mod handler;
pub mod raw;
pub mod request;
pub mod response;
pub mod result;
pub mod router;
pub mod serve;
#[cfg(feature = "hyper")]
pub mod sse;
pub mod testing;
#[cfg(feature = "rustls")]
pub mod tls;

pub use adapter::{ConnectionInfo, RouterService};
pub use context::{AuthCheck, BasicAuthCheck, Context, NamedContext, ResourceProvider};
pub use extract::ServerChain;
pub use raw::RawRequest;
pub use result::RouteResult;
pub use router::Router;
pub use serve::{HasServer, RouterShape, layout, serve, serve_with_context};
#[cfg(feature = "hyper")]
pub use sse::{SseKeepAlive, sse_keep_alive};
pub use testing::{TestClient, TestRequest, TestResponse};
#[cfg(feature = "rustls")]
pub use tls::{RustlsConfig, TlsClientAuth, serve_rustls_listener};
