//! `servant-openapi` — OpenAPI 3.0.3 generation from a servant-rs API.
//!
//! This crate is another *interpretation* of the **same** [`servant_docs`] API
//! description value the markdown documenter, the server, and the client all
//! share: nothing here re-declares routes. [`to_openapi`] walks a
//! [`servant_docs::ApiDoc`] and emits a valid OpenAPI 3.0.3 document as a
//! [`serde_json::Value`]; [`openapi_for`] is the convenience entry point that
//! reflects a [`servant_docs::HasDocs`] API first.
//!
//! ```
//! use servant::prelude::*;
//! use servant_openapi::{OpenApiInfo, openapi_for};
//!
//! // GET /users/{id}  ->  JSON
//! let api = path("users", capture::<u64, _>("id", get::<(Json,), String>()));
//! let doc = openapi_for(&api, OpenApiInfo::new("Users API", "1.0.0"));
//!
//! assert_eq!(doc["openapi"], "3.0.3");
//! assert!(doc["paths"]["/users/{id}"]["get"].is_object());
//! ```
//!
//! ## Schema generation (`[diff]`)
//!
//! Haskell servant-swagger derives a full JSON Schema for every type. The
//! servant-rs documentation model only carries Rust [`std::any::type_name`]
//! strings, so route generation maps those names to OpenAPI schemas with the
//! small, name-based [`schema_for`] helper (primitives → their primitive schema,
//! sequences → untyped arrays, everything else → a titled `object`). Direct
//! [`ToSchema`] users can derive nested structural schemas for request/response
//! models; there is no shared `components/schemas` section yet. See [`schema`]
//! and `docs/DESIGN.md`.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod openapi;
pub mod schema;

pub use openapi::{
    OpenApiError,
    OpenApiInfo,
    checked_openapi_for,
    openapi_for,
    to_checked_openapi,
    to_openapi,
};
pub use schema::{ToSchema, schema_for};
/// Derive [`ToSchema`] for a struct: each field becomes an OpenAPI property
/// (schema inferred from its Rust type name); non-`Option` fields are required.
pub use servant_macros::ToSchema;

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use servant::prelude::*;
    use servant_docs::HasDocs;

    use super::*;

    #[derive(Serialize, Deserialize)]
    struct User {
        id: u64,
        name: String,
    }

    #[derive(Serialize, Deserialize)]
    struct NewItem {
        name: String,
    }

    #[derive(Serialize, Deserialize)]
    struct Item {
        id: u64,
        name: String,
    }

    /// A multi-endpoint API exercising captures, query parameters of each kind,
    /// a request body, a non-default success status, multiple response media
    /// types, headers, summaries, and descriptions.
    fn sample_api() -> impl HasDocs {
        alt(
            // GET /users/{id}?verbose -> JSON User
            path(
                "users",
                capture::<u64, _>("id", query_flag("verbose", get::<(Json,), User>())),
            ),
            alt(
                // POST /items  (JSON body) -> JSON Item, status 201
                path(
                    "items",
                    req_body::<(Json,), NewItem, _>(verb::<Post, 201, (Json,), Item>()),
                ),
                // GET /search?tag (list) &q (normal) -> JSON, PlainText
                path(
                    "search",
                    query_params::<String, _>(
                        "tag",
                        query_param::<String, _>("q", get::<(Json, PlainText), Vec<Item>>()),
                    ),
                ),
            ),
        )
    }

    fn sample_doc() -> serde_json::Value {
        let info =
            OpenApiInfo::new("Sample API", "1.2.3").with_description("A sample servant-rs API.");
        openapi_for(&sample_api(), info)
    }

    #[test]
    fn top_level_shape() {
        let doc = sample_doc();
        assert_eq!(doc["openapi"], "3.0.3");
        assert_eq!(doc["info"]["title"], "Sample API");
        assert_eq!(doc["info"]["version"], "1.2.3");
        assert_eq!(doc["info"]["description"], "A sample servant-rs API.");
        assert!(doc["paths"].is_object());
    }

    #[test]
    fn description_omitted_when_absent() {
        let doc = openapi_for(&sample_api(), OpenApiInfo::new("X", "0.1.0"));
        assert!(doc["info"].get("description").is_none());
    }

    #[test]
    fn get_users_path_capture() {
        let doc = sample_doc();
        let get = &doc["paths"]["/users/{id}"]["get"];
        assert!(get.is_object(), "GET /users/{{id}} missing: {doc:#}");

        // First parameter is the path capture.
        let p0 = &get["parameters"][0];
        assert_eq!(p0["in"], "path");
        assert_eq!(p0["name"], "id");
        assert_eq!(p0["required"], true);
        assert_eq!(p0["schema"]["type"], "integer");
    }

    #[test]
    fn flag_query_param_rendering() {
        let doc = sample_doc();
        let params = doc["paths"]["/users/{id}"]["get"]["parameters"]
            .as_array()
            .expect("parameters array");
        let flag = params
            .iter()
            .find(|p| p["name"] == "verbose")
            .expect("verbose flag param");
        assert_eq!(flag["in"], "query");
        assert_eq!(flag["required"], false);
        assert_eq!(flag["allowEmptyValue"], true);
        assert_eq!(flag["schema"]["type"], "boolean");
    }

    #[test]
    fn normal_and_list_query_params() {
        let doc = sample_doc();
        let params = doc["paths"]["/search"]["get"]["parameters"]
            .as_array()
            .expect("parameters array");

        let normal = params
            .iter()
            .find(|p| p["name"] == "q")
            .expect("q normal param");
        assert_eq!(normal["in"], "query");
        assert_eq!(normal["required"], false);
        assert_eq!(normal["schema"]["type"], "string");

        let list = params
            .iter()
            .find(|p| p["name"] == "tag")
            .expect("tag list param");
        assert_eq!(list["in"], "query");
        assert_eq!(list["required"], false);
        assert_eq!(list["style"], "form");
        assert_eq!(list["explode"], true);
        assert_eq!(list["schema"]["type"], "array");
    }

    #[test]
    fn post_request_body_and_status() {
        let doc = sample_doc();
        let post = &doc["paths"]["/items"]["post"];
        assert!(post.is_object(), "POST /items missing: {doc:#}");

        let body = &post["requestBody"];
        assert_eq!(body["required"], true);
        assert!(
            body["content"]["application/json"]["schema"].is_object(),
            "requestBody json schema missing: {post:#}"
        );

        // Non-default success status appears as the response key.
        assert!(post["responses"]["201"].is_object());
        assert!(post["responses"]["201"]["content"]["application/json"].is_object());
    }

    #[test]
    fn multiple_response_media_types() {
        let doc = sample_doc();
        let responses = &doc["paths"]["/search"]["get"]["responses"]["200"];
        assert_eq!(responses["description"], "");
        assert!(responses["content"]["application/json"].is_object());
        assert!(responses["content"]["text/plain"].is_object());
    }

    #[test]
    fn header_parameter_rendering() {
        let api = path(
            "secret",
            header::<String, _>("X-Token", get::<(Json,), String>()),
        );
        let doc = openapi_for(&api, OpenApiInfo::new("H", "0.1.0"));
        let params = doc["paths"]["/secret"]["get"]["parameters"]
            .as_array()
            .expect("parameters");
        let h = params
            .iter()
            .find(|p| p["name"] == "X-Token")
            .expect("X-Token header");
        assert_eq!(h["in"], "header");
        assert_eq!(h["required"], false);
        assert_eq!(h["schema"]["type"], "string");
    }

    #[test]
    fn no_content_verb_has_empty_response() {
        let api = path("ping", no_content::<Delete>());
        let doc = openapi_for(&api, OpenApiInfo::new("P", "0.1.0"));
        let resp = &doc["paths"]["/ping"]["delete"]["responses"]["204"];
        assert_eq!(resp["description"], "");
        assert!(resp.get("content").is_none(), "204 should have no content");
    }

    #[test]
    fn summary_and_description_on_operation() {
        let api = summary(
            "Greet",
            description(
                "Returns a greeting.",
                path("hello", get::<(PlainText,), String>()),
            ),
        );
        let doc = openapi_for(&api, OpenApiInfo::new("G", "0.1.0"));
        let op = &doc["paths"]["/hello"]["get"];
        assert_eq!(op["summary"], "Greet");
        assert_eq!(op["description"], "Returns a greeting.");
    }

    #[test]
    fn capture_all_renders_in_path_template() {
        let api = path(
            "files",
            capture_all::<String, _>("rest", get::<(Json,), Vec<String>>()),
        );
        let doc = openapi_for(&api, OpenApiInfo::new("F", "0.1.0"));
        let op = &doc["paths"]["/files/{rest}"]["get"];
        assert!(op.is_object(), "missing /files/{{rest}}: {doc:#}");
        assert_eq!(op["parameters"][0]["in"], "path");
        assert_eq!(op["parameters"][0]["required"], true);
    }

    #[test]
    fn all_paths_present() {
        // serde_json::Map without `preserve_order` sorts keys, so assert on set
        // membership rather than ordering (see `to_openapi`'s `[diff]` note).
        let doc = sample_doc();
        let paths = doc["paths"].as_object().expect("paths object");
        assert_eq!(paths.len(), 3);
        assert!(paths.contains_key("/users/{id}"));
        assert!(paths.contains_key("/items"));
        assert!(paths.contains_key("/search"));
    }

    #[test]
    fn empty_api_has_empty_paths() {
        let api = servant::api::EmptyApi;
        let doc = openapi_for(&api, OpenApiInfo::new("E", "0.1.0"));
        assert_eq!(doc["openapi"], "3.0.3");
        assert!(doc["paths"].as_object().expect("paths").is_empty());
    }

    #[test]
    fn query_string_is_explicit_openapi_extension_not_fake_parameter() {
        let api = path(
            "search",
            query_string(query_param::<String, _>("fixed", get::<(Json,), Item>())),
        );
        let doc = openapi_for(&api, OpenApiInfo::new("Q", "0.1.0"));
        let op = &doc["paths"]["/search"]["get"];

        assert_eq!(op["x-servant-query-string"]["decodedOrderedPairs"], true);
        assert_eq!(op["x-servant-query-string"]["rawQuery"], "available");

        let params = op["parameters"].as_array().expect("parameters");
        assert_eq!(params.len(), 1, "QueryString must not fake a parameter");
        assert_eq!(params[0]["name"], "fixed");
    }
}
