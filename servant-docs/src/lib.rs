//! `servant-docs` — a documentation interpretation of a servant-rs API.
//!
//! This crate walks the **same** [`servant`] API description value that the
//! server routes on and the client calls, producing a documentation model
//! ([`ApiDoc`]) that renders to Markdown ([`markdown`]). Nothing here duplicates
//! a route definition: the [`HasDocs`] trait is implemented per combinator and
//! recurses through each combinator's `next` field to the terminal verb, exactly
//! like `servant-server`'s `ServerChain` and `servant-client`'s client walk.
//!
//! ```
//! use servant::prelude::*;
//! use servant_docs::{HasDocs, markdown};
//!
//! // "users" :> Capture "id" u64 :> Get '[JSON] String
//! //   :<|>
//! // "hello" :> Get '[PlainText] String
//! let api = alt(
//!     path("users", capture::<u64, _>("id", get::<(Json,), String>())),
//!     path("hello", get::<(PlainText,), String>()),
//! );
//!
//! let doc = api.docs();
//! assert_eq!(doc.endpoints().len(), 2);
//! let md = markdown(&doc);
//! assert!(md.contains("## GET /users/:id"));
//! assert!(md.contains("## GET /hello"));
//! ```
//!
//! See `docs/DESIGN.md` §10 for the architecture and the intentional differences
//! from Haskell servant-docs (marked `[diff]` in the source).
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod markdown;
mod model;
mod walk;

pub use markdown::markdown;
pub use model::{ApiDoc, BodyDoc, EndpointDoc, FragmentDoc, ParamDoc, ParamKind, PathPart};
pub use walk::HasDocs;

#[cfg(test)]
mod tests {
    use servant::api::EmptyApi;
    use servant::prelude::*;

    use super::*;

    #[derive(serde::Serialize, serde::Deserialize)]
    struct User {
        id: u64,
        name: String,
    }

    // The multi-endpoint API used across the value-model and markdown tests.
    fn sample_api() -> impl HasDocs {
        alt(
            // GET /users/:id?verbose -> JSON User
            path(
                "users",
                capture::<u64, _>("id", query_flag("verbose", get::<(Json,), User>())),
            ),
            alt(
                // POST /users  (JSON body) -> JSON,PlainText, status 201
                path(
                    "users",
                    req_body::<(Json,), User, _>(verb::<Post, 201, (Json, PlainText), User>()),
                ),
                // GET /hello?name=... with a description -> plain text
                summary(
                    "Greet the caller",
                    description(
                        "Returns a friendly greeting.",
                        path(
                            "hello",
                            query_param::<String, _>("name", get::<(PlainText,), String>()),
                        ),
                    ),
                ),
            ),
        )
    }

    #[test]
    fn walk_produces_expected_endpoints() {
        let doc = sample_api().docs();
        let eps = doc.endpoints();
        assert_eq!(eps.len(), 3, "three distinct (path, method) endpoints");

        // 1) GET /users/:id with a flag and a u64 capture.
        let get_user = &eps[0];
        assert_eq!(get_user.method, http::Method::GET);
        assert_eq!(get_user.status, http::StatusCode::OK);
        assert_eq!(get_user.path.len(), 2);
        assert_eq!(get_user.path[0], PathPart::Static("users".into()));
        match &get_user.path[1] {
            PathPart::Capture { name, type_name } => {
                assert_eq!(name, "id");
                assert!(type_name.ends_with("u64"), "capture type: {type_name}");
            }
            other => panic!("expected capture, got {other:?}"),
        }
        assert_eq!(get_user.query_params.len(), 1);
        assert_eq!(get_user.query_params[0].name, "verbose");
        assert_eq!(get_user.query_params[0].kind, ParamKind::Flag);
        assert_eq!(get_user.response_types, vec![mime::APPLICATION_JSON]);
        assert!(get_user.request_body.is_none());

        // 2) POST /users with a JSON body, status 201, two response types.
        let post_user = &eps[1];
        assert_eq!(post_user.method, http::Method::POST);
        assert_eq!(post_user.status, http::StatusCode::CREATED);
        let body = post_user.request_body.as_ref().expect("has body");
        assert_eq!(body.content_types, vec![mime::APPLICATION_JSON]);
        assert!(body.type_name.ends_with("User"));
        assert_eq!(
            post_user.response_types,
            vec![mime::APPLICATION_JSON, mime::TEXT_PLAIN_UTF_8]
        );

        // 3) GET /hello?name with summary + description.
        let hello = &eps[2];
        assert_eq!(hello.method, http::Method::GET);
        assert_eq!(hello.path[0], PathPart::Static("hello".into()));
        assert_eq!(hello.summary.as_deref(), Some("Greet the caller"));
        assert_eq!(
            hello.description.as_deref(),
            Some("Returns a friendly greeting.")
        );
        assert_eq!(hello.query_params.len(), 1);
        assert_eq!(hello.query_params[0].name, "name");
        assert_eq!(hello.query_params[0].kind, ParamKind::Normal);
        assert_eq!(hello.response_types, vec![mime::TEXT_PLAIN_UTF_8]);
    }

    #[test]
    fn capture_all_renders_in_path() {
        let api = path(
            "files",
            capture_all::<String, _>("rest", get::<(Json,), Vec<String>>()),
        );
        let doc = api.docs();
        let ep = &doc.endpoints()[0];
        assert_eq!(ep.path.len(), 2);
        match &ep.path[1] {
            PathPart::CaptureAll { name, .. } => assert_eq!(name, "rest"),
            other => panic!("expected capture-all, got {other:?}"),
        }
        let md = markdown(&doc);
        assert!(md.contains("## GET /files/:rest"), "md was:\n{md}");
    }

    #[test]
    fn no_content_verb_has_no_response_types() {
        let api = path("ping", no_content::<Delete>());
        let doc = api.docs();
        let ep = &doc.endpoints()[0];
        assert_eq!(ep.method, http::Method::DELETE);
        assert_eq!(ep.status, http::StatusCode::NO_CONTENT);
        assert!(ep.response_types.is_empty());
        let md = markdown(&doc);
        assert!(md.contains("- Status code 204"), "md was:\n{md}");
        assert!(md.contains("- No response body"), "md was:\n{md}");
    }

    #[test]
    fn header_is_recorded() {
        let api = path(
            "secret",
            header::<String, _>("X-Token", get::<(Json,), String>()),
        );
        let doc = api.docs();
        assert_eq!(doc.endpoints()[0].headers, vec!["X-Token".to_string()]);
        let md = markdown(&doc);
        assert!(
            md.contains("sensitive to the value of the **X-Token** HTTP header"),
            "md was:\n{md}"
        );
    }

    #[test]
    fn alt_is_left_biased_in_order() {
        // Two distinct endpoints: left must come first.
        let api = alt(
            path("a", get::<(Json,), u8>()),
            path("b", get::<(Json,), u8>()),
        );
        let doc = api.docs();
        assert_eq!(doc.endpoints().len(), 2);
        assert_eq!(doc.endpoints()[0].path[0], PathPart::Static("a".into()));
        assert_eq!(doc.endpoints()[1].path[0], PathPart::Static("b".into()));
    }

    #[test]
    fn same_path_method_merges_left_biased() {
        // Same (path, method) on both sides: one merged endpoint, left's status
        // wins, response content types are unioned in left-then-right order.
        let api = alt(
            path("x", verb::<Get, 200, (Json,), u8>()),
            path("x", verb::<Get, 418, (PlainText,), u8>()),
        );
        let doc = api.docs();
        assert_eq!(doc.endpoints().len(), 1, "merged into one endpoint");
        let ep = &doc.endpoints()[0];
        assert_eq!(ep.status, http::StatusCode::OK, "left status wins");
        assert_eq!(
            ep.response_types,
            vec![mime::APPLICATION_JSON, mime::TEXT_PLAIN_UTF_8]
        );
    }

    #[test]
    fn empty_api_documents_nothing() {
        let doc = EmptyApi.docs();
        assert!(doc.endpoints().is_empty());
        assert_eq!(markdown(&doc), "\n");
    }

    #[test]
    fn markdown_golden_sections() {
        let doc = sample_api().docs();
        let md = markdown(&doc);

        // Endpoint headings in API order.
        assert!(md.contains("## GET /users/:id"), "md:\n{md}");
        assert!(md.contains("## POST /users"), "md:\n{md}");
        assert!(md.contains("## GET /hello"), "md:\n{md}");

        // Section markers.
        assert!(md.contains("### Captures:"), "md:\n{md}");
        assert!(md.contains("### GET Parameters:"), "md:\n{md}");
        assert!(md.contains("### Request:"), "md:\n{md}");
        assert!(md.contains("### Response:"), "md:\n{md}");

        // Notes (summary + description) for /hello.
        assert!(md.contains("### Greet the caller"), "md:\n{md}");
        assert!(md.contains("Returns a friendly greeting."), "md:\n{md}");

        // Flag rendering.
        assert!(md.contains("This parameter is a **flag**."), "md:\n{md}");

        // Request body content types for POST /users.
        assert!(md.contains("- Supported content types are:"), "md:\n{md}");
        assert!(md.contains("    - `application/json`"), "md:\n{md}");

        // Statuses.
        assert!(md.contains("- Status code 200"), "md:\n{md}");
        assert!(md.contains("- Status code 201"), "md:\n{md}");
    }

    #[test]
    fn fragment_metadata_is_recorded_in_docs_model_and_markdown() {
        let api = path(
            "article",
            fragment::<String, _>("section anchor", get::<(Json,), User>()),
        );
        let doc = api.docs();
        let ep = &doc.endpoints()[0];
        let fragment = ep.fragment.as_ref().expect("fragment metadata");

        assert_eq!(fragment.type_name, std::any::type_name::<String>());
        assert_eq!(fragment.description, "section anchor");

        let md = markdown(&doc);
        assert!(md.contains("### Fragment:"), "md:\n{md}");
        assert!(
            md.contains("- *String*: section anchor"),
            "fragment line missing:\n{md}"
        );
    }

    #[test]
    fn operation_id_metadata_is_recorded_in_docs_model_and_markdown() {
        let api = operation_id("getArticle", path("article", get::<(Json,), User>()));
        let doc = api.docs();
        let ep = &doc.endpoints()[0];

        assert_eq!(ep.operation_id.as_deref(), Some("getArticle"));

        let md = markdown(&doc);
        assert!(md.contains("OperationId: getArticle"), "md:\n{md}");
    }
}
