use serde::{Deserialize, Serialize};
use servant::prelude::*;
use servant_docs::HasDocs;
use servant_openapi::{
    OpenApiError,
    OpenApiInfo,
    ToSchema,
    checked_openapi_for,
    openapi_for,
    to_openapi,
};

#[derive(Serialize, Deserialize, ToSchema)]
struct NewItem {
    name: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
struct Item {
    id: u64,
    name: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
struct CreatePet {
    name: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
struct Pet {
    id: u64,
    name: String,
}

#[test]
fn exact_openapi_output_for_metadata_paths_body_and_media_types() {
    let api = summary(
        "Create item",
        description(
            "Creates an item from a JSON request body.",
            path(
                "items",
                req_body::<(Json,), NewItem, _>(verb::<Post, 201, (Json, PlainText), Item>()),
            ),
        ),
    );
    let doc = openapi_for(
        &api,
        OpenApiInfo::new("Regression API", "2026.06")
            .with_description("Exact generated-shape regression coverage."),
    );

    assert_eq!(
        doc,
        serde_json::json!({
            "openapi": "3.0.3",
            "info": {
                "title": "Regression API",
                "version": "2026.06",
                "description": "Exact generated-shape regression coverage."
            },
            "paths": {
                "/items": {
                    "post": {
                        "summary": "Create item",
                        "description": "Creates an item from a JSON request body.",
                        "requestBody": {
                            "required": true,
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "$ref": "#/components/schemas/NewItem"
                                    }
                                }
                            }
                        },
                        "responses": {
                            "201": {
                                "description": "",
                                "content": {
                                    "application/json": {
                                        "schema": {
                                            "$ref": "#/components/schemas/Item"
                                        }
                                    },
                                    "text/plain": {
                                        "schema": {
                                            "$ref": "#/components/schemas/Item"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "components": {
                "schemas": {
                    "Item": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "integer" },
                            "name": { "type": "string" }
                        },
                        "required": ["id", "name"]
                    },
                    "NewItem": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" }
                        },
                        "required": ["name"]
                    }
                }
            }
        })
    );
}

#[test]
fn operation_id_emits_openapi_operation_id() {
    let api = operation_id(
        "createItem",
        path("items", verb::<Post, 201, (Json,), Item>()),
    );
    let doc = openapi_for(&api, OpenApiInfo::new("Operations", "1.0.0"));

    assert_eq!(doc["paths"]["/items"]["post"]["operationId"], "createItem");
}

#[test]
fn operation_id_duplicate_returns_checked_error() {
    let api = alt(
        operation_id("duplicateId", path("left", get::<(Json,), Item>())),
        operation_id("duplicateId", path("right", get::<(Json,), Item>())),
    );

    let err = checked_openapi_for(&api, OpenApiInfo::new("Operations", "1.0.0"))
        .expect_err("duplicate operationId should be rejected");
    assert_eq!(
        err,
        OpenApiError::DuplicateOperationId {
            operation_id: "duplicateId".to_string()
        }
    );

    let compatible = openapi_for(&api, OpenApiInfo::new("Operations", "1.0.0"));
    assert_eq!(
        compatible["paths"]["/left"]["get"]["operationId"],
        "duplicateId"
    );
    assert_eq!(
        compatible["paths"]["/right"]["get"]["operationId"],
        "duplicateId"
    );
}

#[test]
fn schema_components_for_request_and_response() {
    // Given: request and response Rust types with structural ToSchema metadata.
    let api = operation_id(
        "createPet",
        summary(
            "Create pet",
            description(
                "Creates a pet from a JSON request body.",
                path(
                    "pets",
                    req_body::<(Json,), CreatePet, _>(verb::<Post, 201, (Json,), Pet>()),
                ),
            ),
        ),
    );

    // When: an OpenAPI document is generated from the typed API description.
    let doc = openapi_for(&api, OpenApiInfo::new("Pets", "1.0.0"));

    // Then: component schemas are emitted once and operations reference them.
    let schemas = doc["components"]["schemas"]
        .as_object()
        .expect("components.schemas");
    let keys: Vec<_> = schemas.keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["CreatePet", "Pet"]);
    assert_eq!(schemas["CreatePet"]["properties"]["name"]["type"], "string");
    assert_eq!(schemas["Pet"]["properties"]["id"]["type"], "integer");

    let post = &doc["paths"]["/pets"]["post"];
    assert_eq!(post["operationId"], "createPet");
    assert_eq!(post["summary"], "Create pet");
    assert_eq!(
        post["description"],
        "Creates a pet from a JSON request body."
    );
    assert_eq!(
        post["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/CreatePet"
    );
    assert_eq!(
        post["responses"]["201"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/Pet"
    );
}

#[test]
fn primitive_schema_fallback_remains_inline_for_docs_model_type_names() {
    // Given: the Markdown/docs interpretation, which intentionally carries only
    // Rust type-name metadata for schemas.
    let api = path(
        "count",
        req_body::<(Json,), u64, _>(post::<(Json,), String>()),
    );
    let docs = api.docs();

    // When: the OpenAPI lowering consumes that existing docs model directly.
    let doc = to_openapi(&docs, OpenApiInfo::new("Fallback", "1.0.0"));

    // Then: primitive schemas still use the documented type-name fallback and
    // do not invent component schemas.
    let post = &doc["paths"]["/count"]["post"];
    assert_eq!(
        post["requestBody"]["content"]["application/json"]["schema"]["type"],
        "integer"
    );
    assert_eq!(
        post["responses"]["200"]["content"]["application/json"]["schema"],
        serde_json::json!({})
    );
    assert!(doc.get("components").is_none(), "{doc:#}");
}

#[test]
fn checked_openapi_rejects_duplicate_schema_or_operation_ids() {
    // Given: duplicate operationId metadata.
    let duplicate_operation = alt(
        operation_id("duplicateId", path("left", get::<(Json,), Item>())),
        operation_id("duplicateId", path("right", get::<(Json,), Item>())),
    );

    // When: checked generation runs.
    let operation_err = checked_openapi_for(
        &duplicate_operation,
        OpenApiInfo::new("Operations", "1.0.0"),
    )
    .expect_err("duplicate operationId should be rejected");

    // Then: the duplicate operationId is rejected instead of silently merging.
    assert_eq!(
        operation_err,
        OpenApiError::DuplicateOperationId {
            operation_id: "duplicateId".to_string()
        }
    );

    // Given: two different Rust types that would claim the same component name.
    mod request {
        use serde::{Deserialize, Serialize};
        use servant_openapi::ToSchema;

        #[derive(Serialize, Deserialize, ToSchema)]
        pub(super) struct Shared {
            pub(super) name: String,
        }
    }
    mod response {
        use serde::{Deserialize, Serialize};
        use servant_openapi::ToSchema;

        #[derive(Serialize, Deserialize, ToSchema)]
        pub(super) struct Shared {
            pub(super) id: u64,
        }
    }
    let duplicate_schema = path(
        "shared",
        req_body::<(Json,), request::Shared, _>(post::<(Json,), response::Shared>()),
    );

    // When: checked generation sees incompatible schemas for one component key.
    let schema_err = checked_openapi_for(&duplicate_schema, OpenApiInfo::new("Schemas", "1.0.0"))
        .expect_err("duplicate schema component name should be rejected");

    // Then: the duplicate component name is rejected rather than silently merged.
    assert_eq!(
        schema_err,
        OpenApiError::DuplicateSchemaName {
            schema_name: "Shared".to_string()
        }
    );
}
