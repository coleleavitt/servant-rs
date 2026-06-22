use serde_json::{Map, Value, json};
use servant::host::HostPortPolicy;
use servant_docs::{BodyDoc, EndpointDoc, HostDoc, SchemaDoc};

use crate::parameters::parameters_for;
use crate::schema::schema_for;

pub(crate) const HOST_EXTENSION: &str = "x-servant-host";
pub(crate) const HOST_ALTERNATIVES_EXTENSION: &str = "x-servant-host-alternatives";

/// Build the OpenAPI Operation object for a single endpoint.
pub(crate) fn operation_for(endpoint: &EndpointDoc) -> Value {
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

    if endpoint.query_string.is_some() {
        op.insert(
            "x-servant-query-string".into(),
            json!({
                "decodedOrderedPairs": true,
                "rawQuery": "available",
            }),
        );
    }

    if let Some(host) = &endpoint.host {
        op.insert(HOST_EXTENSION.into(), host_extension(host));
    }

    if let Some(body) = &endpoint.request_body {
        op.insert("requestBody".into(), request_body_for(body));
    }

    op.insert("responses".into(), responses_for(endpoint));

    Value::Object(op)
}

fn host_extension(host: &HostDoc) -> Value {
    let mut ext = Map::new();
    ext.insert("name".into(), json!(host.name));
    ext.insert("hostnameCaseInsensitive".into(), json!(true));
    match host.port_policy {
        HostPortPolicy::IgnoreRequestPort => {
            ext.insert("portPolicy".into(), json!("ignore-request-port"));
        }
        HostPortPolicy::RequireExplicitPort(port) => {
            ext.insert("portPolicy".into(), json!("explicit-port-must-match"));
            ext.insert("port".into(), json!(port));
        }
    }
    Value::Object(ext)
}

/// Build the `requestBody` object for a documented body.
fn request_body_for(body: &BodyDoc) -> Value {
    let mut content = Map::new();
    let schema = schema_for_doc(&body.schema);
    for media in &body.content_types {
        content.insert(media_key(media), json!({ "schema": schema.clone() }));
    }
    let mut request_body = Map::new();
    request_body.insert("required".into(), json!(true));
    request_body.insert("content".into(), Value::Object(content));
    if body.streaming {
        request_body.insert("x-servant-streaming-request-body".into(), json!(true));
    }
    Value::Object(request_body)
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
        let schema = endpoint
            .response_schema
            .as_ref()
            .map_or_else(|| json!({}), schema_for_doc);
        let mut content = Map::new();
        for media in &endpoint.response_types {
            content.insert(media_key(media), json!({ "schema": schema.clone() }));
        }
        response.insert("content".into(), Value::Object(content));
    }

    let mut responses = Map::new();
    responses.insert(status, Value::Object(response));
    Value::Object(responses)
}

fn schema_for_doc(schema: &SchemaDoc) -> Value {
    if let Some(name) = &schema.component_name {
        return json!({ "$ref": format!("#/components/schemas/{name}") });
    }
    schema
        .schema
        .clone()
        .unwrap_or_else(|| schema_for(schema.type_name))
}

/// The OpenAPI media-type key for a [`mime::Mime`]: its essence
/// (`type/subtype`), dropping parameters like `; charset=utf-8` so that, e.g.,
/// `text/plain; charset=utf-8` keys as `text/plain`.
fn media_key(media: &mime::Mime) -> String {
    format!("{}/{}", media.type_(), media.subtype())
}
