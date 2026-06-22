//! The typed API description: combinator types plus the traits that compute a
//! handler's argument list ([`HasArgs`]) and an endpoint's response shape
//! ([`Endpoint`]) from that description.
//!
//! Mirrors Servant's combinators (`:>`, `:<|>`, `Capture`, `QueryParam`,
//! `Header`, `ReqBody`, `Verb`, …). Two deliberate Rust differences:
//!
//! - The structure is encoded in the **type** while runtime strings (path
//!   segments, capture/parameter/header names) live in the **value**, because
//!   Rust cannot put `&'static str` literals in type position on stable.
//! - Combinators nest forward through their `next` field: `path("a",
//!   capture::<u64>("id", get::<(Json,), T>()))` is Servant's
//!   `"a" :> Capture "id" u64 :> Get '[JSON] T`.
//!
//! The combinator set is **sealed** (a closed world owned by this crate) so the
//! interpretation traits can be implemented for every combinator without
//! orphan-rule friction; codecs and value types stay open for extension.

mod args;
mod builder_methods;
mod combinators;
mod constructors;
mod endpoint;
mod sealed;

pub use args::HasArgs;
pub use combinators::{
    Alt,
    AuthProtect,
    BasicAuth,
    Capture,
    CaptureAll,
    DeepQuery,
    Description,
    EmptyApi,
    Fragment,
    Header,
    Host,
    HttpVersion,
    IsSecure,
    NoContentVerb,
    OperationId,
    Path,
    QueryFlag,
    QueryParam,
    QueryParams,
    QueryString,
    Raw,
    RawM,
    RemoteHost,
    ReqBody,
    StreamBody,
    StreamVerb,
    Summary,
    UVerb,
    Vault,
    Verb,
    VerbWithHeaders,
    WithNamedContext,
    WithResource,
};
pub use constructors::{
    alt,
    auth_protect,
    basic_auth,
    capture,
    capture_all,
    capture_lenient,
    deep_query,
    description,
    fragment,
    get,
    get_with_headers,
    header,
    host,
    http_version,
    is_secure,
    no_content,
    operation_id,
    path,
    post,
    query_flag,
    query_param,
    query_params,
    query_string,
    raw,
    raw_m,
    remote_host,
    req_body,
    sse_get,
    stream_body,
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
pub use endpoint::Endpoint;

#[cfg(test)]
mod tests {
    use http::{Method, StatusCode};

    use super::*;
    use crate::content::Json;
    use crate::hlist::{HCons, HList, HNil};
    use crate::modifiers::{Optional, ParseError, Strict};

    // A compile-time assertion that two types are equal.
    fn assert_args<E: HasArgs>(_e: &E, _expected: E::Args)
    where
        E::Args: Sized,
    {
    }

    #[test]
    fn arg_list_order_and_shapes() {
        // "users" :> Capture u64 :> QueryParam<String> :> Get
        let api = path(
            "users",
            capture::<u64, _>("id", query_param::<String, _>("q", get::<(Json,), u8>())),
        );
        // Args = HCons<u64, HCons<Option<String>, HNil>>
        assert_args(
            &api,
            HCons {
                head: 7u64,
                tail: HCons {
                    head: Some("x".to_string()),
                    tail: HNil,
                },
            },
        );
        type Api = Path<
            Capture<
                u64,
                Strict,
                QueryParam<String, Optional, Strict, Verb<crate::method::Get, 200, (Json,), u8>>,
            >,
        >;
        assert_eq!(<<Api as HasArgs>::Args as HList>::LEN, 2);
        assert_eq!(api.method(), Method::GET);
        assert_eq!(api.status(), StatusCode::OK);
    }

    #[test]
    fn required_lenient_query_changes_arg_shape() {
        // Last-wins: optional default -> required -> lenient => Result wrapped, required
        let api = path(
            "x",
            query_param::<u32, _>("n", get::<(Json,), u8>())
                .required()
                .lenient(),
        );
        // Args head should be Result<u32, ParseError>
        assert_args(
            &api,
            HCons {
                head: Ok::<u32, ParseError>(1),
                tail: HNil,
            },
        );
    }

    #[test]
    fn capture_lenient_wraps_result() {
        let api = capture_lenient::<u32, _>("id", get::<(Json,), u8>());
        assert_args(
            &api,
            HCons {
                head: Ok::<u32, ParseError>(1),
                tail: HNil,
            },
        );
    }

    #[test]
    fn verb_status_and_method() {
        let e = verb::<crate::method::Post, 201, (Json,), u8>();
        assert_eq!(e.method(), Method::POST);
        assert_eq!(e.status(), StatusCode::CREATED);
        let n = no_content::<crate::method::Delete>();
        assert_eq!(n.status(), StatusCode::NO_CONTENT);
    }
}
