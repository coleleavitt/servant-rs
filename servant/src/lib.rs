//! `servant` — the core typed API description for servant-rs.
//!
//! One handler-free API description, built from the combinators in [`api`],
//! drives every interpretation (server routing, typed clients, links, docs)
//! without duplicating route definitions. This crate provides:
//!
//! - [`api`] — combinator types (`Path`, `Capture`, `QueryParam`, `Header`,
//!   `ReqBody`, `Verb`, `Alt`, …) and the [`api::HasArgs`] / [`api::Endpoint`]
//!   traits that compute a handler's argument list and response shape from the
//!   description;
//! - [`content`] — media-type markers and content negotiation (`MimeRender`,
//!   `MimeUnrender`, `Accept`/`Content-Type` matching);
//! - [`modifiers`] — `Required`/`Optional` × `Strict`/`Lenient` argument shaping;
//! - [`http_data`] — scalar URL/header rendering and parsing;
//! - [`error`] — the structured [`error::ServerError`];
//! - [`link`] — the link value model and escaping;
//! - [`hlist`] / [`func`] — the heterogeneous argument list and the
//!   tuple-to-positional handler bridge.
//!
//! See `docs/DESIGN.md` for the full architecture and intentional differences
//! from Haskell Servant.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod api;
pub mod auth;
pub mod content;
pub mod error;
pub mod func;
pub mod haslink;
pub mod hlist;
pub mod http_data;
pub mod link;
pub mod method;
pub mod modifiers;
pub mod redact;
pub mod response;
pub mod stream;
pub mod uverb;

pub use error::ServerError;
/// Derive a record-routes API: `#[derive(NamedApi)]` on a struct of endpoint
/// fields generates `into_api()` (an `Alt` tree) and a `<Name>Handlers` record.
pub use servant_macros::NamedApi;

/// Commonly used items for defining an API and its handlers.
pub mod prelude {
    pub use crate::api::{
        Endpoint,
        HasArgs,
        alt,
        auth_protect,
        basic_auth,
        capture,
        capture_all,
        capture_lenient,
        description,
        get,
        get_with_headers,
        header,
        http_version,
        is_secure,
        no_content,
        path,
        post,
        query_flag,
        query_param,
        query_params,
        remote_host,
        req_body,
        sse_get,
        stream_get,
        stream_verb,
        summary,
        uverb,
        vault,
        verb,
        verb_with_headers,
        with_named_context,
        with_resource,
    };
    pub use crate::auth::{BasicAuthData, BasicAuthResult};
    pub use crate::content::{
        EventStream,
        FormUrlEncoded,
        Json,
        MediaType,
        MimeRender,
        MimeUnrender,
        NoContent,
        OctetStream,
        PlainText,
    };
    pub use crate::error::ServerError;
    pub use crate::haslink::{HasLink, LinkEndpoint, MakeLink, links};
    pub use crate::hlist::{HCons, HList, HNil, hlist1};
    pub use crate::http_data::{FromHttpApiData, ToHttpApiData};
    pub use crate::method::{Delete, Get, Patch, Post, Put};
    pub use crate::modifiers::{Lenient, Optional, ParseError, Required, Strict};
    pub use crate::response::Headers;
    pub use crate::stream::{
        NetstringFraming,
        NewlineFraming,
        NoFraming,
        ServerEvent,
        SourceStream,
    };
    pub use crate::uverb::{Union2, Union3, Union4, WithStatus, WithStatusHeaders};
}
