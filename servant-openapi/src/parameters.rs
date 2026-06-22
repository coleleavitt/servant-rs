use serde_json::{Map, Value, json};
use servant_docs::{DeepQueryDoc, EndpointDoc, ParamDoc, ParamKind, PathPart};

use crate::schema::schema_for;

pub(crate) fn parameters_for(endpoint: &EndpointDoc) -> Vec<Value> {
    let mut params = Vec::new();

    for part in &endpoint.path {
        match part {
            PathPart::Capture { name, type_name } | PathPart::CaptureAll { name, type_name } => {
                params.push(path_parameter(name, type_name));
            }
            PathPart::Static(_) => {}
        }
    }

    for param in &endpoint.query_params {
        params.push(query_parameter(param));
    }
    for deep_query in &endpoint.deep_queries {
        params.push(deep_query_parameter(deep_query));
    }

    for header in &endpoint.headers {
        params.push(header_parameter(header));
    }

    params
}

fn path_parameter(name: &str, type_name: &str) -> Value {
    json!({
        "name": name,
        "in": "path",
        "required": true,
        "schema": schema_for(type_name),
    })
}

fn query_parameter(param: &ParamDoc) -> Value {
    let mut obj = Map::new();
    obj.insert("name".into(), json!(param.name));
    obj.insert("in".into(), json!("query"));
    obj.insert("required".into(), json!(false));

    match param.kind {
        ParamKind::Flag => {
            obj.insert("allowEmptyValue".into(), json!(true));
            obj.insert("schema".into(), json!({ "type": "boolean" }));
        }
        ParamKind::List => {
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

fn deep_query_parameter(param: &DeepQueryDoc) -> Value {
    json!({
        "name": param.name,
        "in": "query",
        "required": false,
        "style": "deepObject",
        "explode": true,
        "schema": schema_for(param.type_name),
    })
}

fn header_parameter(name: &str) -> Value {
    json!({
        "name": name,
        "in": "header",
        "required": false,
        "schema": { "type": "string" },
    })
}

fn attach_type_title(schema: &mut Value, type_name: &str) {
    if type_name.is_empty() {
        return;
    }
    if let Value::Object(map) = schema {
        map.entry("title")
            .or_insert_with(|| json!(crate::schema::short_name(type_name)));
    }
}
