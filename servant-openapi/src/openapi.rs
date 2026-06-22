//! OpenAPI 3.0.3 document generation from the shared [`servant_docs`] model.
//!
//! This is an *interpretation* of the same [`servant_docs::ApiDoc`] the markdown
//! renderer consumes — it does not re-describe routes. [`to_openapi`] walks the
//! document's endpoints, groups them by templated path, and emits one OpenAPI
//! Operation per `(path, method)`.
//!
//! **[diff]** Haskell's servant-swagger derives structural JSON Schemas for
//! every type and emits `$ref`s into `components/schemas`. The servant-rs
//! OpenAPI interpretation records [`SchemaDoc`] metadata for typed request and
//! response bodies when [`crate::ToSchema`] is available, registers compatible
//! named schemas under `components.schemas`, and keeps the name-based
//! [`crate::schema_for`] fallback for plain [`ApiDoc`] input.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};
use servant_docs::{ApiDoc, PathPart, SchemaDoc};

use crate::operation::{HOST_ALTERNATIVES_EXTENSION, HOST_EXTENSION, operation_for};
use crate::walk::HasOpenApi;

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
    /// Two structural schemas claimed the same component key with different bodies.
    DuplicateSchemaName {
        /// The duplicate OpenAPI component schema key.
        schema_name: String,
    },
}

impl std::fmt::Display for OpenApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenApiError::DuplicateOperationId { operation_id } => {
                write!(f, "duplicate OpenAPI operationId `{operation_id}`")
            }
            OpenApiError::DuplicateSchemaName { schema_name } => {
                write!(f, "duplicate OpenAPI schema component `{schema_name}`")
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

/// Generate a typed API's OpenAPI document directly.
///
/// Convenience wrapper over [`to_openapi`] that first reflects `api` into its
/// [`ApiDoc`] via [`HasOpenApi`], preserving structural schema metadata for
/// request and response body types that implement [`crate::ToSchema`].
///
/// ```
/// use servant::prelude::*;
/// use servant_openapi::{OpenApiInfo, openapi_for};
///
/// let api = path("ping", get::<(PlainText,), String>());
/// let doc = openapi_for(&api, OpenApiInfo::new("Demo", "1.0.0"));
/// assert_eq!(doc["openapi"], "3.0.3");
/// ```
pub fn openapi_for<A: HasOpenApi>(api: &A, info: OpenApiInfo) -> Value {
    to_openapi(&api.openapi_docs(), info)
}

/// Generate an OpenAPI document and reject duplicate checked metadata.
pub fn checked_openapi_for<A: HasOpenApi>(
    api: &A,
    info: OpenApiInfo,
) -> Result<Value, OpenApiError> {
    to_checked_openapi(&api.openapi_docs(), info)
}

/// Generate an OpenAPI 3.0.3 document from a [`servant_docs::ApiDoc`].
///
/// The result is a [`serde_json::Value`] with the shape
/// `{"openapi":"3.0.3","info":{..},"paths":{..}}`. Endpoints are grouped by
/// their templated path (captures rendered as `{name}`); each path maps the
/// lowercased HTTP method to an Operation object. Named structural body schemas
/// are emitted under `components.schemas`, while type-name-only schema metadata
/// falls back to inline [`crate::schema_for`] output.
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

    for endpoint in doc
        .endpoints()
        .iter()
        .filter(|endpoint| endpoint.raw.is_none())
    {
        let path_key = templated_path(&endpoint.path);
        let method = endpoint.method.as_str().to_ascii_lowercase();
        let operation = operation_for(endpoint);

        match paths.get_mut(&path_key) {
            Some(Value::Object(methods)) => {
                insert_method_operation(methods, method, operation);
            }
            _ => {
                let mut methods = Map::new();
                insert_method_operation(&mut methods, method, operation);
                paths.insert(path_key, Value::Object(methods));
            }
        }
    }

    let mut root = Map::new();
    root.insert("openapi".into(), json!("3.0.3"));
    root.insert("info".into(), info.to_json());
    root.insert("paths".into(), Value::Object(paths));

    let schemas = component_schemas(doc);
    if !schemas.is_empty() {
        root.insert(
            "components".into(),
            json!({ "schemas": Value::Object(schemas) }),
        );
    }

    Value::Object(root)
}

fn insert_method_operation(methods: &mut Map<String, Value>, method: String, operation: Value) {
    if let Some(existing) = methods.get_mut(&method) {
        append_same_method_alternative(existing, operation);
    } else {
        methods.insert(method, operation);
    }
}

fn append_same_method_alternative(existing: &mut Value, operation: Value) {
    let Value::Object(existing_op) = existing else {
        return;
    };
    let Value::Object(operation_op) = operation else {
        return;
    };

    if !existing_op.contains_key(HOST_ALTERNATIVES_EXTENSION) {
        existing_op.insert(
            HOST_ALTERNATIVES_EXTENSION.into(),
            Value::Array(vec![same_method_alternative(existing_op)]),
        );
    }

    if let Some(Value::Array(alternatives)) = existing_op.get_mut(HOST_ALTERNATIVES_EXTENSION) {
        alternatives.push(same_method_alternative(&operation_op));
    }
}

fn same_method_alternative(operation: &Map<String, Value>) -> Value {
    let mut alternative = Map::new();
    alternative.insert(
        "host".into(),
        operation
            .get(HOST_EXTENSION)
            .cloned()
            .unwrap_or(Value::Null),
    );
    alternative.insert("operation".into(), Value::Object(operation.clone()));
    Value::Object(alternative)
}

/// Generate an OpenAPI document from a docs model after metadata checks.
pub fn to_checked_openapi(doc: &ApiDoc, info: OpenApiInfo) -> Result<Value, OpenApiError> {
    check_operation_ids(doc)?;
    check_schema_components(doc)?;
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

fn check_schema_components(doc: &ApiDoc) -> Result<(), OpenApiError> {
    let mut seen = BTreeMap::new();
    for schema_doc in schema_docs(doc) {
        let Some(name) = &schema_doc.component_name else {
            continue;
        };
        let Some(schema) = &schema_doc.schema else {
            continue;
        };
        if let Some(existing) = seen.get(name) {
            if existing != schema {
                return Err(OpenApiError::DuplicateSchemaName {
                    schema_name: name.clone(),
                });
            }
        } else {
            seen.insert(name.clone(), schema.clone());
        }
    }
    Ok(())
}

fn component_schemas(doc: &ApiDoc) -> Map<String, Value> {
    let mut components = Map::new();
    for schema_doc in schema_docs(doc) {
        let Some(name) = &schema_doc.component_name else {
            continue;
        };
        let Some(schema) = &schema_doc.schema else {
            continue;
        };
        components
            .entry(name.clone())
            .or_insert_with(|| schema.clone());
    }
    components
}

fn schema_docs(doc: &ApiDoc) -> impl Iterator<Item = &SchemaDoc> {
    doc.endpoints().iter().flat_map(|endpoint| {
        endpoint
            .request_body
            .as_ref()
            .map(|body| &body.schema)
            .into_iter()
            .chain(endpoint.response_schema.as_ref())
    })
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
