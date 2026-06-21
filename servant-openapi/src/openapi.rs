//! OpenAPI 3.0.3 document generation from the shared [`servant_docs`] model.
//!
//! This is an *interpretation* of the same [`servant_docs::ApiDoc`] the markdown
//! renderer consumes — it does not re-describe routes. [`to_openapi`] walks the
//! document's endpoints, groups them by templated path, and emits one OpenAPI
//! Operation per `(path, method)`.
//!
//! **[diff]** Haskell's servant-swagger derives structural JSON Schemas for
//! every type and emits `$ref`s into `components/schemas`. The servant-rs docs
//! model only carries Rust `type_name`s, so schemas are produced inline by the
//! name-based [`crate::schema_for`] mapping and no `components` section is
//! generated yet. See [`crate::schema`] for the rationale.

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};
use servant_docs::{ApiDoc, EndpointDoc, ParamKind, PathPart};

use crate::schema::schema_for;

/// The `info` block of the generated OpenAPI document.
///
/// Maps directly onto the OpenAPI [Info Object]. `description` is omitted from
/// the output when `None`.
///
/// [Info Object]: https://spec.openapis.org/oas/v3.0.3#info-object
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenApiInfo {
    /// The API title (`info.title`).
    pub title: String,
    /// The API version string (`info.version`), e.g. `"1.0.0"`.
    pub version: String,
    /// An optional longer description (`info.description`).
    pub description: Option<String>,
}

/// Errors produced by checked OpenAPI generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenApiError {
    /// Two documented endpoints declared the same `operationId`.
    DuplicateOperationId {
        /// The duplicate OpenAPI `operationId` value.
        operation_id: String,
    },
}

impl std::fmt::Display for OpenApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenApiError::DuplicateOperationId { operation_id } => {
                write!(f, "duplicate OpenAPI operationId `{operation_id}`")
            }
        }
    }
}

impl std::error::Error for OpenApiError {}

impl OpenApiInfo {
    /// Construct an [`OpenApiInfo`] with a title and version and no description.
    pub fn new(title: impl Into<String>, version: impl Into<String>) -> Self {
        OpenApiInfo {
            title: title.into(),
            version: version.into(),
            description: None,
        }
    }

    /// Set the optional `description`.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Render the `info` object.
    fn to_json(&self) -> Value {
        let mut info = Map::new();
        info.insert("title".into(), json!(self.title));
        info.insert("version".into(), json!(self.version));
        if let Some(desc) = &self.description {
            info.insert("description".into(), json!(desc));
        }
        Value::Object(info)
    }
}

/// Generate an [`servant_docs::HasDocs`] API's OpenAPI document directly.
///
/// Convenience wrapper over [`to_openapi`] that first reflects `api` into its
/// [`ApiDoc`] via [`servant_docs::HasDocs::docs`].
///
/// ```
/// use servant::prelude::*;
/// use servant_openapi::{OpenApiInfo, openapi_for};
///
/// let api = path("ping", get::<(PlainText,), String>());
/// let doc = openapi_for(&api, OpenApiInfo::new("Demo", "1.0.0"));
/// assert_eq!(doc["openapi"], "3.0.3");
/// ```
pub fn openapi_for<A: servant_docs::HasDocs>(api: &A, info: OpenApiInfo) -> Value {
    to_openapi(&api.docs(), info)
}

/// Generate an OpenAPI document and reject duplicate `operationId` metadata.
pub fn checked_openapi_for<A: servant_docs::HasDocs>(
    api: &A,
    info: OpenApiInfo,
) -> Result<Value, OpenApiError> {
    to_checked_openapi(&api.docs(), info)
}

/// Generate an OpenAPI 3.0.3 document from a [`servant_docs::ApiDoc`].
///
/// The result is a [`serde_json::Value`] with the shape
/// `{"openapi":"3.0.3","info":{..},"paths":{..}}`. Endpoints are grouped by
/// their templated path (captures rendered as `{name}`); each path maps the
/// lowercased HTTP method to an Operation object. See the module docs for the
/// `[diff]` notes on schema generation.
pub fn to_openapi(doc: &ApiDoc, info: OpenApiInfo) -> Value {
    // Group endpoints sharing a templated path so each path maps several HTTP
    // methods to their operations.
    //
    // **[diff]** The OpenAPI `paths` object is keyed by path string. Without
    // serde_json's `preserve_order` feature, [`Map`] sorts its keys, so the
    // emitted ordering is lexicographic rather than the API's left-to-right
    // order. This is purely cosmetic — JSON object member order is not
    // significant — and avoids forcing a workspace-wide feature flag.
    let mut paths: Map<String, Value> = Map::new();

    for endpoint in doc.endpoints() {
        let path_key = templated_path(&endpoint.path);
        let method = endpoint.method.as_str().to_ascii_lowercase();
        let operation = operation_for(endpoint);

        match paths.get_mut(&path_key) {
            Some(Value::Object(methods)) => {
                methods.insert(method, operation);
            }
            _ => {
                let mut methods = Map::new();
                methods.insert(method, operation);
                paths.insert(path_key, Value::Object(methods));
            }
        }
    }

    json!({
        "openapi": "3.0.3",
        "info": info.to_json(),
        "paths": Value::Object(paths),
    })
}

/// Generate an OpenAPI document from a docs model after metadata checks.
pub fn to_checked_openapi(doc: &ApiDoc, info: OpenApiInfo) -> Result<Value, OpenApiError> {
    check_operation_ids(doc)?;
    Ok(to_openapi(doc, info))
}

fn check_operation_ids(doc: &ApiDoc) -> Result<(), OpenApiError> {
    let mut seen = BTreeSet::new();
    for endpoint in doc.endpoints() {
        let Some(operation_id) = &endpoint.operation_id else {
            continue;
        };
        if !seen.insert(operation_id.clone()) {
            return Err(OpenApiError::DuplicateOperationId {
                operation_id: operation_id.clone(),
            });
        }
    }
    Ok(())
}

/// Render an endpoint path as an OpenAPI path template.
///
/// `Static(s)` contributes `/s`; both capture kinds contribute `/{name}`. An
/// empty path renders as `/`.
fn templated_path(parts: &[PathPart]) -> String {
    if parts.is_empty() {
        return "/".to_string();
    }
    let mut out = String::new();
    for part in parts {
        out.push('/');
        match part {
            PathPart::Static(s) => out.push_str(s),
            PathPart::Capture { name, .. } | PathPart::CaptureAll { name, .. } => {
                out.push('{');
                out.push_str(name);
                out.push('}');
            }
        }
    }
    out
}

/// Build the OpenAPI Operation object for a single endpoint.
fn operation_for(endpoint: &EndpointDoc) -> Value {
    let mut op = Map::new();

    if let Some(summary) = &endpoint.summary {
        op.insert("summary".into(), json!(summary));
    }
    if let Some(description) = &endpoint.description {
        op.insert("description".into(), json!(description));
    }
    if let Some(operation_id) = &endpoint.operation_id {
        op.insert("operationId".into(), json!(operation_id));
    }

    let parameters = parameters_for(endpoint);
    if !parameters.is_empty() {
        op.insert("parameters".into(), Value::Array(parameters));
    }

    if let Some(body) = &endpoint.request_body {
        op.insert("requestBody".into(), request_body_for(body));
    }

    op.insert("responses".into(), responses_for(endpoint));

    Value::Object(op)
}

/// Collect every parameter object for an endpoint: path captures, query
/// parameters, then request headers (in that order).
fn parameters_for(endpoint: &EndpointDoc) -> Vec<Value> {
    let mut params = Vec::new();

    // Path captures — always required.
    for part in &endpoint.path {
        match part {
            PathPart::Capture { name, type_name } | PathPart::CaptureAll { name, type_name } => {
                params.push(path_parameter(name, type_name));
            }
            PathPart::Static(_) => {}
        }
    }

    // Query parameters.
    for param in &endpoint.query_params {
        params.push(query_parameter(param));
    }

    // Request headers.
    for header in &endpoint.headers {
        params.push(header_parameter(header));
    }

    params
}

/// A path parameter: `in: "path"`, `required: true`.
fn path_parameter(name: &str, type_name: &str) -> Value {
    json!({
        "name": name,
        "in": "path",
        "required": true,
        "schema": schema_for(type_name),
    })
}

/// A query parameter, keyed off its [`ParamKind`].
///
/// **[diff]** The docs model does not carry required-ness for query parameters,
/// so every query parameter is emitted with `required: false`, matching
/// Servant's optional-by-default query semantics.
fn query_parameter(param: &servant_docs::ParamDoc) -> Value {
    let mut obj = Map::new();
    obj.insert("name".into(), json!(param.name));
    obj.insert("in".into(), json!("query"));
    obj.insert("required".into(), json!(false));

    match param.kind {
        ParamKind::Flag => {
            // A valueless flag: a boolean that may appear with no value.
            obj.insert("allowEmptyValue".into(), json!(true));
            obj.insert("schema".into(), json!({ "type": "boolean" }));
        }
        ParamKind::List => {
            // A repeated parameter: form-style, exploded array.
            obj.insert("style".into(), json!("form"));
            obj.insert("explode".into(), json!(true));
            let mut schema = json!({ "type": "array", "items": {} });
            attach_type_title(&mut schema, param.type_name);
            obj.insert("schema".into(), schema);
        }
        ParamKind::Normal => {
            let mut schema = schema_for(param.type_name);
            attach_type_title(&mut schema, param.type_name);
            obj.insert("schema".into(), schema);
        }
    }

    Value::Object(obj)
}

/// A request header parameter: `in: "header"`, `required: false`.
fn header_parameter(name: &str) -> Value {
    json!({
        "name": name,
        "in": "header",
        "required": false,
        "schema": { "type": "string" },
    })
}

/// Record the Rust `type_name` on a schema's `title` when it is not already set
/// and the type name is informative. Keeps the source Rust type visible in the
/// generated spec without inventing a structural schema.
fn attach_type_title(schema: &mut Value, type_name: &str) {
    if type_name.is_empty() {
        return;
    }
    if let Value::Object(map) = schema {
        map.entry("title")
            .or_insert_with(|| json!(crate::schema::short_name(type_name)));
    }
}

/// Build the `requestBody` object for a documented body.
fn request_body_for(body: &servant_docs::BodyDoc) -> Value {
    let mut content = Map::new();
    let schema = schema_for(body.type_name);
    for media in &body.content_types {
        content.insert(media_key(media), json!({ "schema": schema.clone() }));
    }
    json!({
        "required": true,
        "content": Value::Object(content),
    })
}

/// Build the `responses` object: a single entry keyed by the declared status.
///
/// When `response_types` is empty (a no-content / 204 verb), the response has a
/// description but no `content`.
fn responses_for(endpoint: &EndpointDoc) -> Value {
    let status = endpoint.status.as_u16().to_string();

    let mut response = Map::new();
    response.insert("description".into(), json!(""));

    if !endpoint.response_types.is_empty() {
        // No documented response Rust type in the model, so use an untyped
        // object schema; the media types are what the endpoint negotiates.
        let mut content = Map::new();
        for media in &endpoint.response_types {
            content.insert(media_key(media), json!({ "schema": {} }));
        }
        response.insert("content".into(), Value::Object(content));
    }

    let mut responses = Map::new();
    responses.insert(status, Value::Object(response));
    Value::Object(responses)
}

/// The OpenAPI media-type key for a [`mime::Mime`]: its essence
/// (`type/subtype`), dropping parameters like `; charset=utf-8` so that, e.g.,
/// `text/plain; charset=utf-8` keys as `text/plain`.
fn media_key(media: &mime::Mime) -> String {
    format!("{}/{}", media.type_(), media.subtype())
}
